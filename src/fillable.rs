use errors::*;
use std::cmp;

/// Fillable is a range from [0,size) that can be filled by subranges.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Fillable {
    size: u64,
    contents: Vec<Interval>,
}

impl Fillable {
    pub fn new(size: u64) -> Self {
        Fillable {
            size: size,
            contents: Vec::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        let r = self.contents.len() == 1 && self.contents[0].start == 0 && self.contents[0].end == self.size;
        // println!("is_full {:?} = {}", self, r);
        return r;
    }

    pub fn is_empty(&self) -> bool {
        self.contents.len() == 0
    }

    pub fn size(&self) -> u64 {
        return self.size;
    }

    pub fn has(&self, n: u64) -> bool {
        for i in self.contents.iter() {
            if n < i.end {
                return i.start <= n;
            }
        }
        return false;
    }

    /// Fill the range [a, b)
    /// Returns whether this piece is _newly_ filled.
    pub fn add(&mut self, a: u64, b: u64) -> Result<bool> {
        let already_full = self.is_full();
        self.add_helper(a, b)?;
        Ok(self.is_full() && !already_full)
    }

    fn add_helper(&mut self, a: u64, b: u64) -> Result<()> {
        // println!("\tadd({}, {}) to {:?}", a, b, self);
        let mut place = 0;
        let mut found = false;
        for idx in 0..self.contents.len() {
            if a <= self.contents[idx].end {
                place = idx;
                // println!("\t\tPlace is {}", place);
                found = true;
                break;
            }
        }
        if !found {
            // the new interval belongs on the end
            // println!("\t\tAdding Interval({},{}) to the end", a,b);
            self.contents.push(Interval::new(a, b));
            return self.check_rep();
            // return Ok(());
        }
        // now self.contents only contains the left_side
        let mut right_side = self.contents.split_off(place);

        // Is the new interval totally before? (no combining)
        if b < right_side[0].start {
            // println!("\t\tThe new interval ({},{}) will go before place.", a,b);
            self.contents.push(Interval::new(a, b));
            self.contents.extend(right_side.into_iter());
            return self.check_rep();
            // return Ok(());
        }

        // Ok; the new interval needs to be combined with the one at right_side[0]
        // which is guaranteed to exist because it's at place.
        // println!("\t\tWill modify {:?}", right_side[0]);
        right_side[0].start = cmp::min(a, right_side[0].start);
        right_side[0].end = cmp::max(b, right_side[0].end);
        // println!("\t\tNow it's {:?}", right_side[0]);

        // Now check if it needs to be combined with right_side[1]
        if right_side.len() > 1 {
            if right_side[0].end >= right_side[1].start {
                // println!("\t\tWill combine with neighbor {:?}", right_side[1]);
                right_side[0].end = right_side[1].end;
                right_side.remove(1);
            }
        }
        self.contents.extend(right_side.into_iter());
        return self.check_rep();
        // return Ok(());


    }

    /// Fill the whole thing
    pub fn fill(&mut self) -> () {
        self.contents = vec![Interval::new(0, self.size)];
    }

    pub fn clear(&mut self) -> () {
        self.contents = Vec::new();
    }

    /// Get the index of the first unfilled byte.
    pub fn first_unfilled(&self) -> Option<u64> {
        if self.contents.is_empty() {
            return Some(0);
        }
        if let Some(interval) = self.contents.first() {
            let end = interval.end;
            if end < self.size {
                return Some(end);
            }
        }
        None
    }

    /// Get the index of the first unfilled byte starting at offset.
    /// Returns some offset in [start, self.size)
    /// Returns None if everything from then on is filled.
    pub fn first_unfilled_starting_at(&self, start: u64) -> Result<Option<u64>> {
        if start >= self.size {
            bail!("first_unfilled_starting_at start:{} >= size:{}",
                  start,
                  self.size);
        }
        if self.contents.is_empty() {
            return Ok(Some(start));
        }
        for interval in self.contents.iter() {
            if interval.end >= start {
                return Ok(Some(interval.end));
            }
        }
        Ok(None)
    }

    fn check_rep(&self) -> Result<()> {
        if self.contents.len() == 0 {
            Ok(())
        } else if self.contents.len() == 1 {
            self.contents[0].check_rep()
        } else {
            let mut res: Result<()> = Ok(());
            // self.contents.into_iter().map(|i| i.check_rep()).fold(
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
                        bail!("Interval starting at {} cannot follow interval ending at {}",
                              i.start,
                              l.end);
                    }
                }
                last = Some(i)
            }
            res
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct Interval {
    pub start: u64, // inclusive
    pub end: u64, // exclusive
}

impl Interval {
    fn new(a: u64, b: u64) -> Self {
        Self { start: a, end: b }
    }
    fn check_rep(&self) -> Result<()> {
        match self.start < self.end {
            false => bail!("Invalid interval ({}, {})", self.start, self.end),
            true => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use fillable::*;

    #[test]
    fn test_fillable_add() {
        // [ [0,3) ] <- [1,2) // completely inclosed in 1 interval already
        let mut f = Fillable::new(3);
        assert!(f.add(0, 3).is_ok());
        assert!(f.has(0));
        assert!(f.has(1));
        assert!(f.has(2));
        assert!(!f.has(3));
        assert!(f.add(1, 2).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);

        // [ [0,3) ] <- [4,5) // completely separate
        f = Fillable::new(5);
        assert!(f.add(0, 3).is_ok());
        assert!(f.add(4, 5).is_ok());
        assert!(!f.has(3));
        assert!(f.has(4));
        assert!(!f.has(5));
        assert!(f.contents.len() == 2);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);
        assert!(f.contents[1].start == 4);
        assert!(f.contents[1].end == 5);

        // [ [0,3) ] <- [1,6) // overlap on left side
        f = Fillable::new(6);
        assert!(f.add(0, 3).is_ok());
        assert!(f.add(1, 6).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 6);

        // [ [0,3) [4,5) ] <- [3,4) // overlap 2 existing intervals
        f = Fillable::new(5);
        assert!(f.add(0, 3).is_ok());
        assert!(f.add(4, 5).is_ok());
        assert!(f.add(3, 4).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 5);

        // [ [1,3) ] <- [0,1) // overlap on right side
        f = Fillable::new(3);
        assert!(f.add(1, 3).is_ok());
        assert!(f.add(0, 1).is_ok());
        assert!(f.contents.len() == 1);
        assert!(f.contents[0].start == 0);
        assert!(f.contents[0].end == 3);
    }
}
