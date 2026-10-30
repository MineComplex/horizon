//! `SimpleBitStorage` and `ZeroBitStorage` as one type: `len` values of `bits` bits each, packed
//! into 64-bit words from the low bits up, never across a word boundary (Java's `long[]` layout,
//! which the chunk packet and the region format carry as is). `bits == 0` holds no words and reads
//! 0 everywhere.

/// Paper's `BETTER_MAGIC`: `index / (64 / bits)` as `(index * MAGIC[bits].0) >> 20`, exact for
/// the indices of a section (`< 4096`, Paper refuses bigger storages too); `.1` is the bits a full
/// word holds (`64 / bits * bits`) so the shift inside the word needs no division either.
const MAGIC: [(u32, u32); 33] = {
    let mut table = [(0, 0); 33];
    let mut bits = 1;
    while bits <= 32 {
        let per = 64 / bits as u32;
        table[bits] = ((1u32 << 20).div_ceil(per), per * bits as u32);
        bits += 1;
    }
    table
};

/// The most entries a storage holds: one section's blocks.
pub const MAX_ENTRIES: usize = 4096;

/// Words needed for `len` values of `bits` bits (`(size + valuesPerLong - 1) / valuesPerLong`).
pub const fn word_count(bits: u8, len: usize) -> usize {
    if bits == 0 {
        0
    } else {
        len.div_ceil(64 / bits as usize)
    }
}

/// `SimpleBitStorage.InitializationException`: saved words of the wrong count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrongLength {
    pub got: usize,
    pub expected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitStorage {
    bits: u8,
    len: usize,
    words: Vec<u64>,
}

impl BitStorage {
    /// Zeroed storage (`new SimpleBitStorage(bits, size)`, or `ZeroBitStorage` for 0 bits).
    pub fn new(bits: u8, len: usize) -> Self {
        assert!(
            bits <= 32 && len <= MAX_ENTRIES,
            "storage of {len} x {bits} bits"
        );
        Self {
            bits,
            len,
            words: vec![0; word_count(bits, len)],
        }
    }

    /// `new SimpleBitStorage(bits, size, long[])`: the words are kept as given, padding included.
    pub fn from_words(bits: u8, len: usize, words: Vec<u64>) -> Result<Self, WrongLength> {
        assert!(
            bits <= 32 && len <= MAX_ENTRIES,
            "storage of {len} x {bits} bits"
        );
        let expected = word_count(bits, len);
        if words.len() != expected {
            return Err(WrongLength {
                got: words.len(),
                expected,
            });
        }
        Ok(Self { bits, len, words })
    }

    /// `new SimpleBitStorage(bits, size, int[])`: values masked to `bits`, unused high bits of
    /// the last word zero.
    pub fn from_values(bits: u8, values: &[u32]) -> Self {
        let mut storage = Self::new(bits, values.len());
        if bits != 0 {
            let per = 64 / usize::from(bits);
            let mask = storage.mask();
            for (word, chunk) in storage.words.iter_mut().zip(values.chunks(per)) {
                *word = chunk
                    .iter()
                    .rev()
                    .fold(0u64, |acc, &v| (acc << bits) | (u64::from(v) & mask));
            }
        }
        storage
    }

    pub fn bits(&self) -> u8 {
        self.bits
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// `getRaw`.
    pub fn words(&self) -> &[u64] {
        &self.words
    }
    pub(crate) fn words_mut(&mut self) -> &mut [u64] {
        &mut self.words
    }

    #[inline]
    fn mask(&self) -> u64 {
        (1u64 << self.bits) - 1
    }

    /// Word and bit offset of entry `i`.
    #[inline]
    fn locate(&self, i: usize) -> (usize, u32) {
        debug_assert!(i < self.len);
        let bits = self.bits;
        if bits.is_power_of_two() {
            let log = bits.trailing_zeros();
            (i >> (6 - log), ((i & ((64 >> log) - 1)) as u32) << log)
        } else {
            let (magic, filled) = MAGIC[usize::from(bits)];
            let word = (i as u32 * magic) >> 20;
            (word as usize, i as u32 * u32::from(bits) - word * filled)
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> u32 {
        if self.bits == 0 {
            return 0;
        }
        let (word, shift) = self.locate(i);
        ((self.words[word] >> shift) & self.mask()) as u32
    }

    /// Stores `value` (masked to `bits`, as Paper writes it unchecked) and returns the old one.
    #[inline]
    pub fn get_and_set(&mut self, i: usize, value: u32) -> u32 {
        if self.bits == 0 {
            return 0;
        }
        let (word, shift) = self.locate(i);
        let mask = self.mask();
        let old = self.words[word];
        self.words[word] = (old & !(mask << shift)) | ((u64::from(value) & mask) << shift);
        ((old >> shift) & mask) as u32
    }

    #[inline]
    pub fn set(&mut self, i: usize, value: u32) {
        self.get_and_set(i, value);
    }

    /// `fill`: every slot of every word, padding included, holds `value`.
    pub fn fill(&mut self, value: u32) {
        if self.bits == 0 {
            return;
        }
        let mut word = u64::from(value) & self.mask();
        let mut shift = u32::from(self.bits);
        while shift < 64 {
            word |= word << shift;
            shift <<= 1;
        }
        self.words.fill(word);
    }

    /// Every value in index order (`getAll` / `unpack`).
    pub fn for_each(&self, mut f: impl FnMut(u32)) {
        if self.bits == 0 {
            (0..self.len).for_each(|_| f(0));
            return;
        }
        let (bits, mask) = (u32::from(self.bits), self.mask());
        let per = 64 / self.bits as usize;
        let mut left = self.len;
        for &word in &self.words {
            let mut w = word;
            for _ in 0..per.min(left) {
                f((w & mask) as u32);
                w >>= bits;
            }
            left = left.saturating_sub(per);
        }
    }
}
