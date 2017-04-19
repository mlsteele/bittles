use std::error::Error;
use bip_bencode::{BencodeRef, BRefAccess};

// MetaInfo of a torrent.
// Loaded from a torrent file.
#[derive(Debug)]
pub struct MetaInfo {
    announce: String,
}

impl MetaInfo {
    pub fn new(src: BencodeRef) -> Result<Self,Box<Error>> {
        let d = src.dict().ok_or("MetaInfo src not dict")?;
        Ok(MetaInfo {
            announce: d.lookup("announce".as_bytes())
                .ok_or("missing 'announce'")?
                .str().ok_or("'announce' not a string")?.to_string(),
        })
    }
}
