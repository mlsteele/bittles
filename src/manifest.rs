use datastore::Verified;
use errors::*;
use fillable::*;
use metainfo::*;
use serde_cbor;
use slog::Logger;
use std;
use std::cmp;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use util::write_atomic;

/// Manifest describes the state of what parts of a torrent have been downloaded and verified.
/// A manifest is associated with a single torrent.
/// It is safe for it to be behind the state of the disk, but unsafe for it to be ahead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    info_hash: InfoHash,
    size_info: SizeInfo,
    /// Whether each piece has been verified
    verified: Vec<bool>,
    /// Which parts of each piece have been downloaded
    present: Vec<Fillable>,
}

impl fmt::Display for Manifest {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        writeln!(f, "Manifest {{")?;
        writeln!(f, "    info_hash: {:?}", self.info_hash)?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl Manifest {
    pub fn new(info: MetaInfo) -> Self {
        let num_pieces = info.num_pieces();

        let mut present: Vec<Fillable> = std::iter::repeat(Fillable::new(info.size_info.non_last_piece_size()))
            .take(num_pieces - 1)
            .collect();
        present.push(Fillable::new(info.size_info.last_piece_size()));

        Self {
            info_hash: info.info_hash.clone(),
            size_info: info.size_info.clone(),
            verified: vec![false; num_pieces],
            present: present,
        }
    }

    /// Sanity check the after a load.
    fn check(&self) -> Result<()> {
        if self.verified.len() as u64 != self.size_info.num_pieces() {
            bail!("wrong sized verified list");
        }
        if self.present.len() as u64 != self.size_info.num_pieces() {
            bail!("wrong sized present list");
        }
        Ok(())
    }

    /// Record the addition of a block.
    /// Can span multiple pieces.
    /// Returns an error if it dives off the end of the file.
    /// Returns a list of pieces newly filled.
    pub fn add_block(&mut self, piece: u64, offset: u64, length: u64) -> Result<Vec<u64>> {
        let mut newly_filled: Vec<u64> = Vec::new();
        // TODO get back and verify this. Make a test.

        // println!("add_block({}, {}, {}) to {:?}", piece, offset, length, self);

        self.size_info.check_range(piece, offset, length)?;

        // find the last affected piece
        let last_piece = self.size_info.piece_at_point(piece, offset + length - 1);
        // println!("\tThe last affected piece is {}", last_piece);

        // Fill the first piece
        {
            // println!("\tWill add interval from {} to {}", offset, )
            let nf = self.present[piece as usize]
                .add(offset,
                     cmp::min(offset + length, self.size_info.piece_size(piece)))?;
            if nf {
                newly_filled.push(piece)
            }
        }
        // Fill the in-between pieces
        for i in piece + 1..last_piece {
            let p = &mut self.present[i as usize];
            let nf = !p.is_full();
            p.fill();
            if nf {
                newly_filled.push(i)
            }
        }
        // Fill the last piece
        if last_piece != piece {
            // local means local to the piece
            let local_end = {
                let last_piece_start = self.size_info.absolute_offset(last_piece, 0);
                let absolute_end = self.size_info.absolute_offset(piece, offset + length);
                absolute_end - last_piece_start
            };
            let nf = self.present[last_piece as usize].add(0, local_end)?;
            if nf {
                newly_filled.push(last_piece)
            }
        }
        Ok(newly_filled)
    }

    /// Remove a piece
    pub fn remove_piece(&mut self, piece: u64) -> Result<()> {
        self.size_info.check_piece(piece)?;
        self.present[piece as usize].clear();
        Ok(())
    }

    /// Record that a piece was verified.
    pub fn mark_verified(&mut self, verified: Verified) -> Result<()> {
        self.size_info.check_piece(verified.piece)?;
        self.verified[verified.piece as usize] = true;
        Ok(())
    }

    pub fn is_full(&self, piece: u64) -> Result<bool> {
        self.size_info.check_piece(piece)?;
        Ok(self.present[piece as usize].is_full())
    }

    /// List of pieces that still need to be verified.
    pub fn needs_verify(&self) -> Vec<u64> {
        self.verified
            .iter()
            .enumerate()
            .filter(|&(_i, v)| !v)
            .map(|(i, _v)| i as u64)
            .collect()
    }

    /// Whether all data has been verified.
    pub fn all_verified(&self) -> bool {
        return self.needs_verify().len() == 0;
    }

    /// Get the next desired block.
    /// This is the first block that has not been added.
    pub fn next_desired_block(&self) -> Option<BlockRequest> {
        for i in 0..self.size_info.num_pieces() as usize {
            let p = &self.present[i];
            if !p.is_full() {
                if let Some(x) = self.present[i].first_unfilled() {
                    if x < self.present[i].size() {
                        let left = self.present[i].size() - x;
                        let max_length = 1 << 14;
                        return Some(BlockRequest {
                                        piece: i as u64,
                                        offset: x as u64,
                                        length: cmp::min(left as u64, max_length),
                                    });
                    }
                }
            }
        }
        None
    }

    pub fn progress_bar(&self) -> String {
        let present = &self.present;
        let verified = &self.verified;
        present
            .iter()
            .zip(verified)
            .map(|(p, v)| match (!p.is_empty(), *v) {
                     (false, false) => "_",
                     (true, false) => ".",
                     (true, true) => "=",
                     (false, true) => "!",
                 })
            .collect::<Vec<_>>()
            .concat()
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
        let ph = PieceHash { hash: [0; PIECE_HASH_SIZE] };
        let info = MetaInfo {
            announce: std::string::String::new(),
            info_hash: InfoHash { hash: [0; INFO_HASH_SIZE] },
            leader_piece_length: 6,
            piece_hashes: vec![ph.clone(), ph.clone(), ph.clone()],
            file_info: FileInfo::Single {
                name: "".to_owned(),
                length: 0,
            },
        };
        let mut manifest = Manifest::new(info);
        let r = manifest.add_block(0, 4, 11); // add into the middle
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

/// A wrapper around Manifest that can save and load from a file.
pub struct ManifestWithFile {
    pub manifest: Manifest,
    path: PathBuf,
}

impl ManifestWithFile {
    pub fn load_or_new<P: AsRef<Path>>(log: Logger, info: MetaInfo, path: P) -> Result<Self> {
        match Self::load_from_path(info.clone(), path.as_ref().clone()) {
            Ok(x) => {
                debug!(log, "manifest loaded from file");
                Ok(x)
            }
            Err(err) => {
                debug!(log, "manifest created anew because: {}", err);
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
            bail!("loaded mismatched manifest info hash");
        }
        manifest.check()?;
        Ok(Self {
               manifest: manifest,
               path: path.as_ref().to_owned(),
           })
    }

    pub fn store(&self, log: &Logger) -> Result<()> {
        debug!(log, "saving manifest: {:?}", self.path);
        let temp_path = {
            let mut fname: String = self.path
                .file_name()
                .and_then(|x| x.to_str())
                .ok_or("missing file name")?
                .to_owned();
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

pub struct BlockRequest {
    /// Piece index
    pub piece: u64, // (index)
    /// Offset within the piece
    pub offset: u64, // (begin)
    /// Length in bytes
    pub length: u64,
}
