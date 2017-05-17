use errors::*;
use metainfo::{MetaInfo, PieceHash, SizeInfo};
use ring::digest;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use util::ReadWire;

/// Value representing that a piece has been verified.
pub struct Verified {
    pub piece: u64,
}

pub struct DataStore {
    file: fs::File,
    size_info: SizeInfo,
}

impl DataStore {
    pub fn create_or_open<P: AsRef<Path>>(metainfo: &MetaInfo, path: P) -> Result<Self> {
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .chain_err(|| "datastore file could not be opened")?;
        f.set_len(metainfo.size_info.total_size())?;
        Ok(DataStore {
               file: f,
               size_info: metainfo.size_info.clone(),
           })
    }

    pub fn write_block(&mut self, piece: u64, offset: u64, block: &[u8]) -> Result<()> {
        self.size_info
            .check_range(piece, offset, block.len() as u64)?;
        let x = self.size_info.absolute_offset(piece, offset);
        self.file.seek(SeekFrom::Start(x))?;
        self.file.write_all(block)?;
        Ok(())
    }

    pub fn verify_piece(&mut self, piece: u64, expected: PieceHash) -> Result<Option<Verified>> {
        self.size_info.check_piece(piece)?;
        let x = self.size_info.absolute_offset(piece, 0);
        let read_length = self.size_info.piece_size(piece);
        self.file.seek(SeekFrom::Start(x))?;
        let buf = self.file.read_n(read_length)?;
        let dig = digest::digest(&digest::SHA1, &buf);
        if dig.as_ref() == expected.hash {
            Ok(Some(Verified { piece: piece }))
        } else {
            Ok(None)
        }
    }
}
