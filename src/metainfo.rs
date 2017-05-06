use bip_bencode::{BencodeRef, BRefAccess, BDictAccess};
use error::{Error, Result};
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
    /// Size in bytes of each piece
    pub piece_length: u32,
    pub piece_hashes: Vec<PieceHash>,
    pub file_info: FileInfo,
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

impl fmt::Display for MetaInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        writeln!(f, "MetaInfo {{")?;
        writeln!(f, "    announce: {:?}", self.announce)?;
        writeln!(f, "    info_hash: {:?}", self.info_hash)?;
        writeln!(f, "    piece_length: {:?} bytes", self.piece_length)?;
        writeln!(f, "    piece_hashes: {:?} hashes", self.piece_hashes.len())?;
        writeln!(f, "    total size: {} bytes", self.total_size())?;
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
            return Err(Error::new_str(&format!("piece_length {} < 0", piece_length)));
        }

        // concatenation of each piece hash
        let pieces_hashes_concat = info.lookup("pieces".as_bytes())
            .ok_or_err("missing 'pieces'")?
            .bytes()
            .ok_or_err("'piece' not bytes")?;
        if pieces_hashes_concat.len() % PIECE_HASH_SIZE != 0 {
            return Err(Error::new_str(&format!("piece hashes length {} not divisible by {}",
                                               pieces_hashes_concat.len(),
                                               PIECE_HASH_SIZE)));
        }
        let piece_hashes = pieces_hashes_concat.chunks(PIECE_HASH_SIZE)
            .map(|a| {
                let mut h = [0; PIECE_HASH_SIZE];
                h.copy_from_slice(a);
                PieceHash { hash: h }
            })
            .collect::<Vec<_>>();

        let res = MetaInfo {
            announce: d.lookup("announce".as_bytes())
                .ok_or_err("missing 'announce'")?
                .str()
                .ok_or_err("'announce' not a string")?
                .to_string(),
            info_hash: make_info_hash(d.lookup("info".as_bytes())
                .ok_or_err("missing 'info'")?
                .buffer())?,
            piece_length: piece_length,
            piece_hashes: piece_hashes,
            file_info: Self::load_file_info(info)?,
        };

        if ((res.num_pieces() as u64) * res.piece_length as u64) < res.total_size() {
            return Err(Error::new_str("pieces and total size don't match"));
        }
        if ((res.num_pieces() as u64 - 1) * res.piece_length as u64) > res.total_size() {
            return Err(Error::new_str("pieces and total size don't match"));
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

    /// Size in bytes of the whole torrent.
    pub fn total_size(&self) -> u64 {
        use FileInfo::*;
        let res: u64 = match &self.file_info {
            &Single { length, .. } => length,
            &Multi { ref files, .. } => files.iter().map(|x| x.length).sum(),
        };
        res
    }
}

trait ExtendedOption<T> {
    fn ok_or_err(self, &str) -> Result<T>;
}

impl<T> ExtendedOption<T> for Option<T> {
    fn ok_or_err(self, description: &str) -> Result<T> {
        match self {
            Some(v) => Ok(v),
            None => Err(Error::new_str(description)),
        }
    }
}
