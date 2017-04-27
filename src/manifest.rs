use errors::Error;
use io;

/// A manifest describes which bytes have been downloaded and verified
/// for a single infohash.
pub struct Manifest {
    info_hash: InfoHash,
    num_pieces: usize,
    piece_lengh: usize,
    /// Whether each piece has been verified
    verified: Vec<bool>,
    /// Which parts of each piece have been downloaded
    present: Vec<Ranges>,
}

impl Manifest {
    pub fn new(info: MetaInfo) -> Self {
        let num_pieces = info.num_pieces();
        Self {
            info_hash: info.info_hash.clone(),
            num_pieces: num_pieces,
            piece_length: info.piece_length,
            verified: vec![false; ]
        }
    }

    pub fn load<T: io::Read>(stream: T) -> Result<Self> {
        return Err(Error::todo());
        // check(res)?
    }

    pub fn store<T: io::Write>(stream: T) -> Result<()> {
        return Err(Error::todo());
    }

    /// Sanity check the after a load.
    fn check(&self) -> Result<()> {
        if self.verified.len() != self.num_pieces {
            return Err(Error::new_str("wrong sized verified list"));
        }
        if self.present.len() != self.num_pieces {
            return Err(Error::new_str("wrong sized present list"));
        }
    }

    /// Record the addition of a block.
    /// Can span multiple pieces.
    /// Returns an error if it dives off the end of the file.
    pub fn add_block(&mut self, piece: u32, offset: u32, length: u32) -> Result<()> {
        // TODO this is probably wrong.
        // last affected piece
        let last_piece = piece + (offset + length) / piece_length;
        self.check_piece(last_piece)?;
        {
            self.present[piece].add(offset, cmp::min(offset + length, self.piece_length))?;
        }
        for i+1 in piece..last_piece {
            self.present[i].fill();
        }
        if piece != last_piece {
            self.present[last_piece].add(0, length - (self.piece_length * (last_piece - piece)) );
        }
    }

    /// Remove a piece
    pub fn remove_piece(&mut self, piece: u32) -> Result<()> {
        self.check_piece(piece)?;
        self.present[piece].clear();
    }

    /// Record that a piece was verified.
    pub fn verify(&self, piece: u32) -> Result<()> {
        self.check_piece(piece)?;
        self.verified[piece] = true
    }

    /// Check that a piece number is in bounds.
    fn check_piece(&self, piece: u32) -> Result<()> {
        match 0 < piece < self.num_pieces {
            true => Ok(()),
            false => Err(Error::new_str(
                &format!("piece out of bounds !({} < {})", piece, self.num_pieces))),
        }
    }
}
