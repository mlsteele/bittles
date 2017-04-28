use error::{Error,Result};
use metainfo::{MetaInfo,PieceHash};
use ring::digest;
use std::fs;
use std::io::{Seek,SeekFrom,Write};
use std::path::Path;
use util::ReadWire;

pub struct DataStore {
    file: fs::File,
    num_pieces: u64,
    piece_size: u32,
}

impl DataStore {
    pub fn create_or_open<P: AsRef<Path>>(metainfo: &MetaInfo, path: P) -> Result<Self> {
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        f.set_len(metainfo.total_size() as u64)?;
        Ok(DataStore {
            file: f,
            num_pieces: metainfo.num_pieces(),
            piece_size: metainfo.piece_length as u32,
        })
    }

    pub fn write_block(&mut self, piece: u32, offset: u32, block: &[u8]) -> Result<()> {
        self.check_piece(piece)?;
        // last affected piece
        let last_piece = piece + (offset + (block.len() as u32)) / self.piece_size;
        self.check_piece(last_piece)?;
        let x = self.point(piece, offset);
        self.file.seek(SeekFrom::Start(x))?;
        self.file.write_all(block)?;
        Ok(())
    }

    pub fn verify_piece(&mut self, piece: u32, expected: PieceHash) -> Result<bool> {
        self.check_piece(piece)?;
        let x = self.point(piece, 0);
        self.file.seek(SeekFrom::Start(x))?;
        let buf = self.file.read_n(self.piece_size as u64)?;
        let dig = digest::digest(&digest::SHA1, &buf);
        if dig.as_ref() == expected.hash {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check that a piece number is in bounds.
    fn check_piece(&self, piece: u32) -> Result<()> {
        match piece < self.num_pieces as u32 {
            true => Ok(()),
            false => Err(Error::new_str(
                &format!("piece out of bounds !({} < {})", piece, self.num_pieces))),
        }
    }

    /// Calculate the byte offset. Does not check bounds.
    fn point(&self, piece: u32, offset: u32) -> u64 {
        (piece as u64) * (self.piece_size as u64) + (offset as u64)
    }
}
