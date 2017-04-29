use error::{Error,Result};
use std::cmp;
use std::fmt;
use std;
use fillable::*;
use metainfo::*;
use std::path::{Path,PathBuf};
use util::write_atomic;
use std::fs;
use serde_cbor;

/// Manifest describes the state of what parts of a torrent have been downloaded and verified.
/// A manifest is associated with a single torrent.
/// It is safe for it to be behind the state of the disk, but unsafe for it to be ahead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    info_hash: InfoHash,
    num_pieces: usize,
    piece_length: u32,
    /// Whether each piece has been verified
    verified: Vec<bool>,
    /// Which parts of each piece have been downloaded
    present: Vec<Fillable>,
}

impl fmt::Display for Manifest {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        writeln!(f, "Manifest {{")?;
        writeln!(f, "    info_hash: {:?}", self.info_hash)?;
        writeln!(f, "    piece_length: {:?} bytes", self.piece_length)?;
        //writeln!(f, "    num_pieces: {:?} pieces", self.num_pieces)?;
        //writeln!(f, "    total size: {} bytes", self.total_size())?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl Manifest {
    pub fn new(info: MetaInfo) -> Self {
        let num_pieces = info.num_pieces();
        let piece_length = info.piece_length;
        Self {
            info_hash: info.info_hash.clone(),
            num_pieces: num_pieces,
            piece_length: piece_length,
            verified: vec![false; num_pieces],
            present: std::iter::repeat(Fillable::new(piece_length)).take(num_pieces).collect(),
        }
    }

    /// Sanity check the after a load.
    fn check(&self) -> Result<()> {
        if self.verified.len() != self.num_pieces {
            return Err(Error::new_str("wrong sized verified list"));
        }
        if self.present.len() != self.num_pieces {
            return Err(Error::new_str("wrong sized present list"));
        }
        Ok(())
    }

    /// Record the addition of a block.
    /// Can span multiple pieces.
    /// Returns an error if it dives off the end of the file.
    pub fn add_block(&mut self, piece: usize, offset: u32, length: u32) -> Result<()> {
        //println!("add_block({}, {}, {}) to {:?}", piece, offset, length, self);

        // find the last affected piece
        let last_byte: u64 = (piece as u64 * self.piece_length as u64) + (offset + length) as u64;
        let last_piece: usize = if last_byte % (self.piece_length as u64) == 0 {
                (last_byte / (self.piece_length as u64) - 1) as usize
            } else {
                (last_byte / self.piece_length as u64) as usize
            };
        self.check_piece(last_piece)?;
        //println!("\tThe last affected piece is {}", last_piece);

        // Fill the first piece
        {
            //println!("\tWill add interval from {} to {}", offset, )
            self.present[piece].add(offset, cmp::min(offset + length, self.piece_length))?;
        }
        // Fill the in-between pieces
        for i in piece+1..last_piece {
            self.present[i].fill();
        }
        // Fill the last piece
        if piece != last_piece {
            let upto = if (offset + length) % self.piece_length == 0 {
                    self.piece_length
                } else {
                    (offset + length) % self.piece_length
                };
            self.present[last_piece].add(0, upto)?;
        }
        Ok(())
    }

    /// Remove a piece
    pub fn remove_piece(&mut self, piece: usize) -> Result<()> {
        self.check_piece(piece)?;
        self.present[piece].clear();
        Ok(())
    }

    /// Record that a piece was verified.
    pub fn verify(&mut self, piece: usize) -> Result<()> {
        self.check_piece(piece)?;
        self.verified[piece] = true;
        Ok(())
    }

    pub fn is_full(&self, piece: usize) -> Result<bool> {
        self.check_piece(piece)?;
        Ok(self.present[piece].is_full())
    }

    /// Check that a piece number is in bounds.
    fn check_piece(&self, piece: usize) -> Result<()> {
        #[allow(unused_comparisons)]
        match 0 <= piece && piece < self.num_pieces {
            true => Ok(()),
            false => Err(Error::new_str(
                &format!("piece out of bounds !({} < {})", piece, self.num_pieces))),
        }
    }
}

#[cfg(test)]
mod tests {
    use manifest::*;
    use metainfo::*;

    #[test]
    fn test_add_block() {
        // Reference: add_block(0, 4, 11) with piece_length = 6
        // 0             1             2
        // - - - - * * | * * * * * * | * * * - - -
        let ph = PieceHash{hash: [0; PIECE_HASH_SIZE]};
        let info = MetaInfo {
            announce: std::string::String::new(),
            info_hash: InfoHash{hash: [0; INFO_HASH_SIZE]},
            piece_length: 6,
            piece_hashes: vec![ph.clone(), ph.clone(), ph.clone()],
            file_info: FileInfo::Single { name: "".to_owned(), length: 0 },
        };
        let mut manifest = Manifest::new(info);
        let r=manifest.add_block(0, 4, 11); // add into the middle
        println!("{:?}", r);
        assert!(r.is_ok()); // add into the middle
        assert!(!manifest.is_full(0).unwrap());
        assert!(manifest.is_full(1).unwrap());
        assert!(!manifest.is_full(2).unwrap());

        assert!(manifest.add_block(0, 0, 4).is_ok()); // fill in the beginning
        assert!(manifest.is_full(0).unwrap());
        assert!(manifest.is_full(1).unwrap());
        assert!(!manifest.is_full(2).unwrap());

        assert!(manifest.add_block(1, 0, 5).is_ok()); // no op
        assert!(manifest.is_full(0).unwrap());
        assert!(manifest.is_full(1).unwrap());
        assert!(!manifest.is_full(2).unwrap());

        assert!(manifest.add_block(2, 0, 6).is_ok()); // fill in the end
        assert!(manifest.is_full(0).unwrap());
        assert!(manifest.is_full(1).unwrap());
        assert!(manifest.is_full(2).unwrap());

    }
}

pub struct ManifestWithFile {
    pub manifest: Manifest,
    path: PathBuf,
}

impl ManifestWithFile {
    pub fn load_or_new<P: AsRef<Path>>(info: MetaInfo, path: P) -> Result<Self> {
        match Self::load_from_path(info.clone(), path.as_ref().clone()) {
            Ok(x) => {
                println!("manifest loaded from file");
                Ok(x)
            },
            Err(err) => {
                println!("manifest created anew because: {:?}", err);
                Ok(Self::new(info, path))
            }
        }
    }

    fn new<P: AsRef<Path>>(info: MetaInfo, path: P) -> Self {
        Self {
            manifest: Manifest::new(info),
            path: path.as_ref().to_owned(),
        }
    }

    fn load_from_path<P: AsRef<Path>>(info: MetaInfo, path: P) -> Result<Self> {
        let f = fs::File::open(&path)?;
        let manifest: Manifest = serde_cbor::de::from_reader(f)?;
        if manifest.info_hash != info.info_hash {
            return Err(Error::Generic("loaded mismatched manifest info hash".to_owned()));
        }
        manifest.check()?;
        Ok(Self {
            manifest: manifest,
            path: path.as_ref().to_owned(),
        })
    }

    pub fn store(&self) -> Result<()> {
        println!("saving manifest: {:?}", self.path);
        let temp_path = {
            let mut fname: String = self.path.file_name()
                .and_then(|x|x.to_str())
                .ok_or(Error::Generic("missing file name".to_owned()))?.to_owned();
            fname.push_str(".swp");
            self.path.with_file_name(fname)
        };
        write_atomic(&self.path, temp_path, |writer| {
            serde_cbor::ser::to_writer_sd(writer, &self.manifest)?;
            Ok(())
        })?;
        Ok(())
    }

}
