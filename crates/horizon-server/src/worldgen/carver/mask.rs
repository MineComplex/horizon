//! `CarvingMask`: the blocks the carvers dug in one chunk, one bit per block, Y innermost, then Z,
//! then X. Carvers write into it; [`CarvingMask::visit`] then hands the result to the chunk as
//! vertical runs.

/// The bits of a chunk's carved blocks between `min_y` and `max_y` (inclusive).
#[derive(Clone, Debug)]
pub struct CarvingMask {
    min_y: i32,
    max_y: i32,
    height: usize,
    words: Vec<u64>,
    empty: bool,
}

impl CarvingMask {
    /// `new CarvingMask(minY, maxY)`; a range of no height holds nothing.
    pub fn new(min_y: i32, max_y: i32) -> Self {
        let height = (i64::from(max_y) - i64::from(min_y) + 1).max(0) as usize;
        Self {
            min_y,
            max_y,
            height,
            words: vec![0; (256 * height).div_ceil(64)],
            empty: true,
        }
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn max_y(&self) -> i32 {
        self.max_y
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// `carve(x, y, z)` for a chunk-local `x`, `z` in `0..16` and `y` in `min_y..=max_y`.
    pub fn carve(&mut self, x: i32, y: i32, z: i32) {
        let index = self.index(x, y, z);
        self.words[index / 64] |= 1 << (index % 64);
        self.empty = false;
    }

    pub fn is_carved(&self, x: i32, y: i32, z: i32) -> bool {
        let index = self.index(x, y, z);
        self.words[index / 64] >> (index % 64) & 1 != 0
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        debug_assert!((0..16).contains(&x) && (0..16).contains(&z));
        debug_assert!((self.min_y..=self.max_y).contains(&y));
        (y - self.min_y) as usize + (z as usize + ((x as usize) << 4)) * self.height
    }

    /// `BitSet.nextSetBit`.
    fn next_set(&self, from: usize) -> Option<usize> {
        let mut word = from / 64;
        if word >= self.words.len() {
            return None;
        }
        let mut bits = self.words[word] & (u64::MAX << (from % 64));
        loop {
            if bits != 0 {
                return Some(word * 64 + bits.trailing_zeros() as usize);
            }
            word += 1;
            bits = *self.words.get(word)?;
        }
    }

    /// `BitSet.nextClearBit` (the bit after the last word is clear).
    fn next_clear(&self, from: usize) -> usize {
        let mut word = from / 64;
        if word >= self.words.len() {
            return from;
        }
        let mut bits = !self.words[word] & (u64::MAX << (from % 64));
        loop {
            if bits != 0 {
                return word * 64 + bits.trailing_zeros() as usize;
            }
            word += 1;
            match self.words.get(word) {
                Some(next) => bits = !next,
                None => return word * 64,
            }
        }
    }

    /// `visit`: every run of carved blocks, in index order, split at the column boundaries, as
    /// `(x, z, bottom_y, top_y)` (both inclusive). The first error of `visitor` stops it.
    pub fn visit<E>(
        &self,
        mut visitor: impl FnMut(i32, i32, i32, i32) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut start = self.next_set(0);
        while let Some(first) = start {
            let last = self.next_clear(first) - 1;
            for column in first / self.height..=last / self.height {
                let base = column * self.height;
                let bottom = first.saturating_sub(base) as i32 + self.min_y;
                let top = (last - base).min(self.height - 1) as i32 + self.min_y;
                visitor((column >> 4 & 15) as i32, (column & 15) as i32, bottom, top)?;
            }
            start = self.next_set(last + 1);
        }
        Ok(())
    }

    /// The runs [`Self::visit`] gives, collected.
    pub fn runs(&self) -> Vec<[i32; 4]> {
        let mut runs = Vec::new();
        let _ = self.visit(|x, z, bottom, top| {
            runs.push([x, z, bottom, top]);
            Ok::<_, std::convert::Infallible>(())
        });
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_split_at_columns_and_gaps() {
        let mut mask = CarvingMask::new(-63, 120);
        assert!(mask.is_empty());
        // Column (0, 0): two runs; column (0, 1): from its bottom; the top of (0, 0) joins it in
        // the bit set but Java hands the pieces over per column.
        for y in [-10, -9, -8, 4] {
            mask.carve(0, y, 0);
        }
        mask.carve(0, 120, 0);
        mask.carve(0, -63, 1);
        mask.carve(0, -62, 1);
        mask.carve(15, 7, 15);
        assert!(!mask.is_empty());
        assert!(mask.is_carved(0, 4, 0) && !mask.is_carved(0, 5, 0));
        assert_eq!(
            mask.runs(),
            vec![
                [0, 0, -10, -8],
                [0, 0, 4, 4],
                [0, 0, 120, 120],
                [0, 1, -63, -62],
                [15, 15, 7, 7],
            ]
        );
    }

    #[test]
    fn a_run_over_many_columns_is_cut_into_columns() {
        let mut mask = CarvingMask::new(0, 9);
        for y in 5..=9 {
            mask.carve(0, y, 0);
        }
        for y in 0..=9 {
            mask.carve(0, y, 1);
        }
        for y in 0..=2 {
            mask.carve(0, y, 2);
        }
        assert_eq!(mask.runs(), vec![[0, 0, 5, 9], [0, 1, 0, 9], [0, 2, 0, 2]]);
    }
}
