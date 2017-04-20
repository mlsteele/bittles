use bip_bencode::{BencodeRef, BRefAccess};
use ring::digest;
use std::error::Error;

pub const INFOHASH_SIZE: usize = 20;
pub type InfoHash = [u8; INFOHASH_SIZE];

pub fn make_info_hash(src: &[u8]) -> Result<InfoHash,Box<Error>> {
    let dig = digest::digest(&digest::SHA1, src);
    let mut ret = [0; INFOHASH_SIZE];
    ret.copy_from_slice(dig.as_ref());
    Ok(ret)
}

// MetaInfo of a torrent.
// Loaded from a torrent file.
#[derive(Debug, Clone)]
pub struct MetaInfo {
    pub announce: String,
    pub info_hash: InfoHash,
}

impl MetaInfo {
    pub fn new(src: BencodeRef) -> Result<Self,Box<Error>> {
        let d = src.dict().ok_or("MetaInfo src not dict")?;
        Ok(MetaInfo {
            announce: d.lookup("announce".as_bytes())
                .ok_or("missing 'announce'")?
                .str().ok_or("'announce' not a string")?.to_string(),
            info_hash: make_info_hash(d.lookup("info".as_bytes())
                .ok_or("missing 'info'")?.buffer())?,
        })
    }
}
