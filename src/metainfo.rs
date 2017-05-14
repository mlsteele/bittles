use bip_bencode::{BencodeRef, BRefAccess, BDictAccess};
use errors::*;
use ring::digest;
use std::fmt;
use std::str;
use std;
use itertools::Itertools;

pub const INFO_HASH_SIZE: usize = 20;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoHash {
    pub hash: [u8; PIECE_HASH_SIZE],
}

impl fmt::Debug for InfoHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        write!(f, "{:x}", self.hash.iter().format(""))
    }
}

pub const PIECE_HASH_SIZE: usize = 20;
#[derive(Clone)]
pub struct PieceHash {
    pub hash: [u8; PIECE_HASH_SIZE],
}

impl fmt::Debug for PieceHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        write!(f, "{:x}", self.hash.iter().format(""))
    }
}

pub fn make_info_hash(src: &[u8]) -> Result<InfoHash> {
    let dig = digest::digest(&digest::SHA1, src);
    let mut ret = [0; INFO_HASH_SIZE];
    ret.copy_from_slice(dig.as_ref());
    Ok(InfoHash { hash: ret })
}

// MetaInfo of a torrent.
// Loaded from a torrent file.
#[derive(Debug, Clone)]
pub struct MetaInfo {
    pub announce: String,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<PieceHash>,
    pub file_info: FileInfo,
    pub size_info: SizeInfo,
}

#[derive(Debug, Clone)]
pub enum FileInfo {
    Single { name: String, length: u64 },
    Multi {
        name: String,
        files: Vec<SubFileInfo>,
    },
}

#[derive(Debug, Clone)]
pub struct SubFileInfo {
    path: String,
    length: u64,
}


impl FileInfo {
    /// Size in bytes of the whole torrent.
    pub fn total_size(&self) -> u64 {
        use FileInfo::*;
        let res: u64 = match *self {
            Single { length, .. } => length,
            Multi { ref files, .. } => files.iter().map(|x| x.length).sum(),
        };
        res
    }
}

impl fmt::Display for MetaInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        writeln!(f, "MetaInfo {{")?;
        writeln!(f, "    announce: {:?}", self.announce)?;
        writeln!(f, "    info_hash: {:?}", self.info_hash)?;
        writeln!(f, "    piece_hashes: {:?} hashes", self.piece_hashes.len())?;
        writeln!(f, "    total size: {} bytes", self.size_info.total_size())?;
        writeln!(f, "    pieces: {}", self.size_info.num_pieces())?;
        writeln!(f,
                 "    piece_length: {}...{}",
                 self.size_info.non_last_piece_size(),
                 self.size_info.last_piece_size())?;
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl MetaInfo {
    pub fn new(src: BencodeRef) -> Result<Self> {
        let d = src.dict().ok_or_err("MetaInfo src not dict")?;
        let info = d.lookup("info".as_bytes())
            .ok_or_err("missing 'info'")?
            .dict()
            .ok_or_err("'info' not a dict")?;

        // for (k,_) in d.to_list() {
        //     println!("key: {}", str::from_utf8(k)?);
        // }

        // length in bytes of each piece
        let piece_length = info.lookup("piece length".as_bytes())
            .ok_or_err("missing 'piece length'")?
            .int()
            .ok_or_err("'piece length' not an int")? as u32;
        if piece_length <= 0 {
            bail!("piece_length {} < 0", piece_length);
        }

        // concatenation of each piece hash
        let pieces_hashes_concat = info.lookup("pieces".as_bytes())
            .ok_or_err("missing 'pieces'")?
            .bytes()
            .ok_or_err("'piece' not bytes")?;
        if pieces_hashes_concat.len() % PIECE_HASH_SIZE != 0 {
            bail!("piece hashes length {} not divisible by {}",
                  pieces_hashes_concat.len(),
                  PIECE_HASH_SIZE);
        }
        let piece_hashes = pieces_hashes_concat.chunks(PIECE_HASH_SIZE)
            .map(|a| {
                let mut h = [0; PIECE_HASH_SIZE];
                h.copy_from_slice(a);
                PieceHash { hash: h }
            })
            .collect::<Vec<_>>();

        let n_pieces = piece_hashes.len();

        let file_info = Self::load_file_info(info)?;

        let res = MetaInfo {
            announce: d.lookup("announce".as_bytes())
                .ok_or_err("missing 'announce'")?
                .str()
                .ok_or_err("'announce' not a string")?
                .to_string(),
            info_hash: make_info_hash(d.lookup("info".as_bytes())
                .ok_or_err("missing 'info'")?
                .buffer())?,
            piece_hashes: piece_hashes,
            file_info: file_info.clone(),
            size_info: SizeInfo {
                total_size: file_info.total_size(),
                num_pieces: n_pieces as u64,
                leader_piece_length: piece_length as u64,
            },
        };

        if ((res.num_pieces() as u64) * res.size_info.leader_piece_length as u64) < res.file_info.total_size() {
            bail!("pieces and total size don't match");
        }
        if ((res.num_pieces() as u64 - 1) * res.size_info.leader_piece_length as u64) > res.file_info.total_size() {
            bail!("pieces and total size don't match");
        }
        if res.size_info.total_size() != res.file_info.total_size() {
            bail!("file_info and size_info disagree on size");
        }

        Ok(res)
    }

    fn load_file_info(info: &BDictAccess<BencodeRef>) -> Result<FileInfo> {
        let name = info.lookup("name".as_bytes())
            .ok_or_err("missing 'info.name'")?
            .str()
            .ok_or_err("'info.name' not str")?;
        let length = info.lookup("length".as_bytes())
            .ok_or_err("missing 'info.length'")?
            .int()
            .ok_or_err("'info.length' not int")?;
        Ok(FileInfo::Single {
            name: name.to_owned(),
            length: length as u64,
        })
    }

    pub fn num_pieces(&self) -> usize {
        return self.piece_hashes.len();
    }
}

/// SizeInfo provides common calculations for dealing with
/// the pieces and size of torrent data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeInfo {
    /// Total size in bytes.
    total_size: u64,
    /// Number of pieces
    num_pieces: u64,
    /// Size in bytes of each piece except the last.
    leader_piece_length: u64,
}

impl SizeInfo {
    pub fn num_pieces(&self) -> u64 {
        self.num_pieces
    }

    /// Total size of the data in bytes.
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Calculate the byte offset. Does not check bounds.
    pub fn absolute_offset(&self, piece: u64, offset: u64) -> u64 {
        piece * self.leader_piece_length + offset
    }

    /// Size of a piece. Does not check bounds.
    pub fn piece_size(&self, piece: u64) -> u64 {
        if piece == self.num_pieces - 1 {
            self.total_size - ((self.num_pieces - 1) * self.leader_piece_length)
        } else {
            self.leader_piece_length
        }
    }

    pub fn non_last_piece_size(&self) -> u64 {
        self.leader_piece_length
    }

    pub fn last_piece_size(&self) -> u64 {
        self.total_size - ((self.num_pieces - 1) * self.leader_piece_length)
    }

    /// Return the piece number of the piece at the specified offset.
    /// Does not check bounds.
    pub fn piece_at_point(&self, piece: u64, offset: u64) -> u64 {
        piece + (offset / self.leader_piece_length)
    }

    /// Check that the piece index is in bounds.
    pub fn check_piece(&self, piece: u64) -> Result<()> {
        match piece < self.num_pieces {
            true => Ok(()),
            false => bail!("piece out of bounds !({} < {})", piece, self.num_pieces),
        }
    }

    /// Check that a point falls inside the bounds.
    pub fn check_point(&self, piece: u64, offset: u64) -> Result<()> {
        if self.absolute_offset(piece, offset) >= self.total_size {
            bail!("point out of bounds {} {}", piece, offset);
        } else {
            Ok(())
        }
    }

    /// Check that the range range falls inside the bounds.
    pub fn check_range(&self, piece: u64, offset: u64, length: u64) -> Result<()> {
        if self.absolute_offset(piece, offset + length) > self.total_size {
            bail!("range out of bounds {} {} {}", piece, offset, length);
        } else {
            Ok(())
        }
    }
}

trait ExtendedOption<T> {
    fn ok_or_err(self, &str) -> Result<T>;
}

impl<T> ExtendedOption<T> for Option<T> {
    fn ok_or_err(self, description: &str) -> Result<T> {
        match self {
            Some(v) => Ok(v),
            None => bail!("{}", description),
        }
    }
}
