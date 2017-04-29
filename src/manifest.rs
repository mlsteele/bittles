use error::{Error,Result};
use std::io;
use std::cmp;
use std::fmt;
use std;
use metainfo::*;

/// Manifest describes the state of what parts of a torrent have been downloaded and verified.
/// A manifest is associated with a single torrent.
/// It is safe for it to be behind the state of the disk, but unsafe for it to be ahead.
#[derive(Debug)]
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
            self.present[last_piece].add(0, upto);
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
        match 0 <= piece && piece < self.num_pieces {
            true => Ok(()),
            false => Err(Error::new_str(
                &format!("piece out of bounds !({} < {})", piece, self.num_pieces))),
        }
    }
}

/// Fillable is a range from [0,size) that can be filled by subranges.
#[derive(Clone, Debug)]
struct Fillable {
    size: u32,
    contents: Vec<Interval>,
}
impl Fillable {
    fn new(size: u32) -> Self {
        Fillable { size: size, contents: Vec::new() }
    }

    fn is_full(&self) -> bool {
        let r = self.contents.len() == 1 &&
            self.contents[0].start == 0 &&
            self.contents[0].end == self.size;
        //println!("is_full {:?} = {}", self, r);
        return r;
    }

    /// Fill the range [a, b)
    // Cases:
    //   [ [0,3) ] <- [1,2) // completely inclosed in 1 interval already
    //   [ [0,3) ] <- [4,5) // completely separate
    //   [ [0,3) ] <- [1,6) // overlap on left side
    //   [ [0,3) [4,5) ] <- [3,4) // overlap 2 existing intervals
    //   [ [1,3) ] <- [0,1) // overlap on right side


    // [0,1] [3,4] <- [1,2] returns 0
    // [0,1] [3,4] <- [2,3] returns 1
    fn add(&mut self, a: u32, b: u32) -> Result<()> {
        //println!("\tadd({}, {}) to {:?}", a, b, self);
        let mut place = 0;
        let mut found = false;
        for idx in 0..self.contents.len() {
            if a <= self.contents[idx].end {
                place = idx;
                //println!("\t\tPlace is {}", place);
                found = true;
                break;
            }
        }
        if !found {
            // the new interval belongs on the end
            //println!("\t\tAdding Interval({},{}) to the end", a,b);
            self.contents.push(Interval::new(a,b));
            return self.check_rep();
            //return Ok(());
        }
        // now self.contents only contains the left_side
        let mut right_side = self.contents.split_off(place);

        // Is the new interval totally before? (no combining)
        if b < right_side[0].start {
            //println!("\t\tThe new interval ({},{}) will go before place.", a,b);
            self.contents.push(Interval::new(a,b));
            self.contents.extend(right_side.into_iter());
            return self.check_rep();
            //return Ok(());
        }

        // Ok; the new interval needs to be combined with the one at right_side[0]
        // which is guaranteed to exist because it's at place.
        //println!("\t\tWill modify {:?}", right_side[0]);
        right_side[0].start = cmp::min(a, right_side[0].start);
        right_side[0].end = cmp::max(b, right_side[0].end);
        //println!("\t\tNow it's {:?}", right_side[0]);

        // Now check if it needs to be combined with right_side[1]
        if right_side.len() > 1 {
            if right_side[0].end >= right_side[1].start {
                //println!("\t\tWill combine with neighbor {:?}", right_side[1]);
                right_side[0].end = right_side[1].end;
                right_side.remove(1);
            }
        }
        self.contents.extend(right_side.into_iter());
        return self.check_rep();
        //return Ok(());


    }

    /// Fill the whole thing
    fn fill(&mut self) -> () {
        self.contents = vec![Interval::new(0, self.size)];
    }

    fn clear(&mut self) -> () {
        self.contents = Vec::new();
    }

    fn check_rep(&self) -> Result<()> {
        if self.contents.len() == 0 { Ok(()) }
        else if self.contents.len() == 1 { self.contents[0].check_rep() }
        else {
            let mut res: Result<()> = Ok(());
            //self.contents.into_iter().map(|i| i.check_rep()).fold(
            //    Ok(()), |acc, &r| if r.is_err { r } else { acc })
            let mut last: Option<&Interval> = None;
            for i in self.contents.iter() {
                let res_i = i.check_rep();
                if res_i.is_err() {
                    res = res_i;
                    break;
                }
                if let Some(l) = last {
                    if l.end >= i.start {
                        res = Err(Error::new_str(&format!(
                            "Interval starting at {} cannot follow interval ending at {}",
                            i.start, l.end)));
                        break;
                    }
                }
                last = Some(i)
            }
            res
        }
    }
}

#[derive(Clone, Debug)]
struct Interval {
    start: u32, // inclusive
    end:   u32, // exclusive
}

impl Interval {
    fn new(a: u32, b: u32) -> Self {
        Self {start: a, end: b}
    }
    fn check_rep(&self) -> Result<()> {
        match self.start < self.end {
            false => Err(Error::new_str(&format!(
                "Invalid interval ({}, {})", self.start, self.end))),
            true => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use manifest::*;
    use metainfo::*;

    #[test]
    fn test_fillable_add() {
        // [ [0,3) ] <- [1,2) // completely inclosed in 1 interval already
        let mut f = Fillable::new(3);
        assert!(f.add(0,3).is_ok());
        assert!(f.add(1,2).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);

        // [ [0,3) ] <- [4,5) // completely separate
        f = Fillable::new(5);
        assert!(f.add(0,3).is_ok());
        assert!(f.add(4,5).is_ok());
        assert!(f.contents.len() == 2);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);
        assert!(f.contents[1].start == 4);
        assert!(f.contents[1].end == 5);

        // [ [0,3) ] <- [1,6) // overlap on left side
        f = Fillable::new(6);
        assert!(f.add(0,3).is_ok());
        assert!(f.add(1,6).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 6);

        // [ [0,3) [4,5) ] <- [3,4) // overlap 2 existing intervals
        f = Fillable::new(5);
        assert!(f.add(0,3).is_ok());
        assert!(f.add(4,5).is_ok());
        assert!(f.add(3,4).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 5);

        // [ [1,3) ] <- [0,1) // overlap on right side
        f = Fillable::new(3);
        assert!(f.add(1,3).is_ok());
        assert!(f.add(0,1).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);
    }

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
