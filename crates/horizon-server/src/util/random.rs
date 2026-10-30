//! The game's random number generators (`RandomSource` and its implementations).
//!
//! Behaviour follows Leaf's sources exactly: `LegacyRandomSource`/`SingleThreadedRandomSource` (Java's 48-bit
//! LCG; the two differ only in Java's thread checks, which ownership makes unnecessary), `XoroshiroRandomSource`
//! (Xoroshiro128++ with Mojang's bounded-int and float/double conversions), `MarsagliaPolarGaussian`,
//! `RandomSupport` and the positional factories. Note Leaf's `nextFloat`/`nextDouble` multiply in `float`
//! (`bits * 1.110223E-16F` with a `long`/`int` operand is a float multiplication in Java), so they do here too.
//!
//! A generator is a plain value owned by whatever uses it (`&mut self` to draw); the variants are an enum, not
//! trait objects. Leaf's optional `FastRNG` (JDK `Xoroshiro128PlusPlus`, off by default) is not provided yet.

/// Java's `String.hashCode` (UTF-16 code units), used by `fromHashOf`.
pub fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// `Mth.getSeed(x, y, z)`: the per-position seed of positional randoms and block offsets.
pub fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
    seed = seed.wrapping_mul(seed).wrapping_mul(42317861).wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

const FLOAT_UNIT: f32 = 5.9604645E-8;
const DOUBLE_UNIT: f32 = 1.110223E-16;

/// `RandomSupport.mixStafford13`.
pub fn mix_stafford13(mut z: i64) -> i64 {
    z = (z ^ ((z as u64) >> 30) as i64).wrapping_mul(-4658895280553007687);
    z = (z ^ ((z as u64) >> 27) as i64).wrapping_mul(-7723592293110705685);
    z ^ ((z as u64) >> 31) as i64
}

pub const GOLDEN_RATIO_64: i64 = -7046029254386353131;
pub const SILVER_RATIO_64: i64 = 7640891576956012809;

/// `RandomSupport.Seed128bit`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed128 {
    pub lo: i64,
    pub hi: i64,
}
impl Seed128 {
    /// `RandomSupport.upgradeSeedTo128bitUnmixed`.
    pub fn upgrade_unmixed(legacy_seed: i64) -> Seed128 {
        let lo = legacy_seed ^ SILVER_RATIO_64;
        Seed128 { lo, hi: lo.wrapping_add(GOLDEN_RATIO_64) }
    }
    /// `RandomSupport.upgradeSeedTo128bit`.
    pub fn upgrade(legacy_seed: i64) -> Seed128 {
        Self::upgrade_unmixed(legacy_seed).mixed()
    }
    pub fn mixed(self) -> Seed128 {
        Seed128 { lo: mix_stafford13(self.lo), hi: mix_stafford13(self.hi) }
    }
    pub fn xor(self, lo: i64, hi: i64) -> Seed128 {
        Seed128 { lo: self.lo ^ lo, hi: self.hi ^ hi }
    }
    /// `RandomSupport.seedFromHashOf`: the MD5 of the UTF-8 string, big-endian halves.
    pub fn from_hash_of(input: &str) -> Seed128 {
        use md5::{Digest, Md5};
        let h = Md5::digest(input.as_bytes());
        Seed128 {
            lo: i64::from_be_bytes(h[0..8].try_into().unwrap()),
            hi: i64::from_be_bytes(h[8..16].try_into().unwrap()),
        }
    }
}

/// `RandomSupport.generateUniqueSeed`: a seed for randoms nobody reproduces (mob AI, particles...).
pub fn generate_unique_seed() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    // Shared counter across threads, as Java's AtomicLong: the one place a seed source is global.
    static UNIQUIFIER: AtomicI64 = AtomicI64::new(8682522807148012);
    let mut cur = UNIQUIFIER.load(Ordering::Relaxed);
    let next = loop {
        let n = cur.wrapping_mul(1181783497276652981);
        match UNIQUIFIER.compare_exchange_weak(cur, n, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break n,
            Err(c) => cur = c,
        }
    };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
    next ^ nanos
}

/// `MarsagliaPolarGaussian`'s cached second value.
#[derive(Clone, Copy, Default, Debug)]
struct Gaussian {
    next: f64,
    have_next: bool,
}

/// Java's 48-bit LCG (`LegacyRandomSource`, `SingleThreadedRandomSource`).
#[derive(Clone, Debug)]
pub struct LegacyRandom {
    seed: i64,
    gaussian: Gaussian,
}
const MODULUS_MASK: i64 = (1 << 48) - 1;
const MULTIPLIER: i64 = 25214903917;
impl LegacyRandom {
    pub fn new(seed: i64) -> Self {
        LegacyRandom { seed: (seed ^ MULTIPLIER) & MODULUS_MASK, gaussian: Gaussian::default() }
    }
    pub fn set_seed(&mut self, seed: i64) {
        *self = Self::new(seed);
    }
    /// `BitRandomSource.next(bits)`.
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(11) & MODULUS_MASK;
        (self.seed >> (48 - bits)) as i32
    }
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((bound as i64).wrapping_mul(self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let sample = self.next(31);
            let modulo = sample % bound;
            if sample.wrapping_sub(modulo).wrapping_add(bound - 1) >= 0 {
                return modulo;
            }
        }
    }
    pub fn next_long(&mut self) -> i64 {
        let upper = self.next(32) as i64;
        let lower = self.next(32) as i64;
        (upper << 32).wrapping_add(lower)
    }
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * FLOAT_UNIT
    }
    pub fn next_double(&mut self) -> f64 {
        let upper = self.next(26) as i64;
        let lower = self.next(27) as i64;
        let combined = (upper << 27).wrapping_add(lower);
        (combined as f32 * DOUBLE_UNIT) as f64
    }
}

/// Mojang's Xoroshiro128++ (`Xoroshiro128PlusPlus`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Xoroshiro128 {
    lo: i64,
    hi: i64,
}
impl Xoroshiro128 {
    pub fn new(lo: i64, hi: i64) -> Self {
        if lo | hi == 0 { Xoroshiro128 { lo: GOLDEN_RATIO_64, hi: SILVER_RATIO_64 } } else { Xoroshiro128 { lo, hi } }
    }
    pub fn next_long(&mut self) -> i64 {
        let s0 = self.lo;
        let mut s1 = self.hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        self.lo = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        self.hi = s1.rotate_left(28);
        result
    }
    pub fn seeds(&self) -> (i64, i64) {
        (self.lo, self.hi)
    }
}

/// `XoroshiroRandomSource`.
#[derive(Clone, Debug)]
pub struct XoroshiroRandom {
    rng: Xoroshiro128,
    gaussian: Gaussian,
}
impl XoroshiroRandom {
    pub fn new(seed: i64) -> Self {
        Self::from_seed128(Seed128::upgrade(seed))
    }
    pub fn from_seed128(s: Seed128) -> Self {
        Self::from_parts(s.lo, s.hi)
    }
    pub fn from_parts(lo: i64, hi: i64) -> Self {
        XoroshiroRandom { rng: Xoroshiro128::new(lo, hi), gaussian: Gaussian::default() }
    }
    pub fn set_seed(&mut self, seed: i64) {
        *self = Self::new(seed);
    }
    pub fn next_long(&mut self) -> i64 {
        self.rng.next_long()
    }
    pub fn next_int(&mut self) -> i32 {
        self.rng.next_long() as i32
    }
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        let bound64 = bound as u64;
        let mut bits = self.next_int() as u32 as u64;
        let mut multiplied = bits * bound64;
        let mut fractional = multiplied & 0xFFFF_FFFF;
        if fractional < bound64 {
            let unbiased_start = ((!bound).wrapping_add(1) as u32 % bound as u32) as u64;
            while fractional < unbiased_start {
                bits = self.next_int() as u32 as u64;
                multiplied = bits * bound64;
                fractional = multiplied & 0xFFFF_FFFF;
            }
        }
        (multiplied >> 32) as i32
    }
    fn next_bits(&mut self, bits: u32) -> i64 {
        ((self.rng.next_long() as u64) >> (64 - bits)) as i64
    }
    pub fn next_boolean(&mut self) -> bool {
        self.rng.next_long() & 1 != 0
    }
    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * FLOAT_UNIT
    }
    pub fn next_double(&mut self) -> f64 {
        (self.next_bits(53) as f32 * DOUBLE_UNIT) as f64
    }
}

/// A game random: the `RandomSource` implementations Leaf creates.
#[derive(Clone, Debug)]
pub enum RandomSource {
    Legacy(LegacyRandom),
    Xoroshiro(XoroshiroRandom),
}

impl RandomSource {
    /// `RandomSource.create(seed)` (Leaf's default: the legacy LCG).
    pub fn create(seed: i64) -> Self {
        RandomSource::Legacy(LegacyRandom::new(seed))
    }
    /// `RandomSource.create()`: a fresh unique seed.
    pub fn create_unseeded() -> Self {
        Self::create(generate_unique_seed())
    }
    pub fn xoroshiro(seed: i64) -> Self {
        RandomSource::Xoroshiro(XoroshiroRandom::new(seed))
    }

    pub fn set_seed(&mut self, seed: i64) {
        match self {
            RandomSource::Legacy(r) => r.set_seed(seed),
            RandomSource::Xoroshiro(r) => r.set_seed(seed),
        }
    }
    pub fn next_int(&mut self) -> i32 {
        match self {
            RandomSource::Legacy(r) => r.next_int(),
            RandomSource::Xoroshiro(r) => r.next_int(),
        }
    }
    /// `nextInt(bound)`; panics on a non-positive bound as Java throws.
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        match self {
            RandomSource::Legacy(r) => r.next_int_bounded(bound),
            RandomSource::Xoroshiro(r) => r.next_int_bounded(bound),
        }
    }
    /// `nextIntBetweenInclusive(min, max)`.
    pub fn next_int_between_inclusive(&mut self, min: i32, max_inclusive: i32) -> i32 {
        self.next_int_bounded(max_inclusive.wrapping_sub(min).wrapping_add(1)).wrapping_add(min)
    }
    /// `nextInt(origin, bound)`.
    pub fn next_int_range(&mut self, origin: i32, bound: i32) -> i32 {
        assert!(origin < bound, "bound - origin is non positive");
        origin.wrapping_add(self.next_int_bounded(bound.wrapping_sub(origin)))
    }
    pub fn next_long(&mut self) -> i64 {
        match self {
            RandomSource::Legacy(r) => r.next_long(),
            RandomSource::Xoroshiro(r) => r.next_long(),
        }
    }
    pub fn next_boolean(&mut self) -> bool {
        match self {
            RandomSource::Legacy(r) => r.next_boolean(),
            RandomSource::Xoroshiro(r) => r.next_boolean(),
        }
    }
    pub fn next_float(&mut self) -> f32 {
        match self {
            RandomSource::Legacy(r) => r.next_float(),
            RandomSource::Xoroshiro(r) => r.next_float(),
        }
    }
    pub fn next_double(&mut self) -> f64 {
        match self {
            RandomSource::Legacy(r) => r.next_double(),
            RandomSource::Xoroshiro(r) => r.next_double(),
        }
    }
    /// `MarsagliaPolarGaussian.nextGaussian`.
    pub fn next_gaussian(&mut self) -> f64 {
        let cached = match self {
            RandomSource::Legacy(r) => &mut r.gaussian,
            RandomSource::Xoroshiro(r) => &mut r.gaussian,
        };
        if cached.have_next {
            cached.have_next = false;
            return cached.next;
        }
        loop {
            let x = 2.0 * self.next_double() - 1.0;
            let y = 2.0 * self.next_double() - 1.0;
            let r2 = x * x + y * y;
            if r2 < 1.0 && r2 != 0.0 {
                let m = (-2.0 * r2.ln() / r2).sqrt();
                let g = match self {
                    RandomSource::Legacy(r) => &mut r.gaussian,
                    RandomSource::Xoroshiro(r) => &mut r.gaussian,
                };
                g.next = y * m;
                g.have_next = true;
                return x * m;
            }
        }
    }
    /// `triangle(mean, spread)` (double).
    pub fn triangle(&mut self, mean: f64, spread: f64) -> f64 {
        mean + spread * (self.next_double() - self.next_double())
    }
    /// `triangle(mean, spread)` (float).
    pub fn triangle_f32(&mut self, mean: f32, spread: f32) -> f32 {
        mean + spread * (self.next_float() - self.next_float())
    }
    pub fn consume_count(&mut self, rounds: i32) {
        for _ in 0..rounds.max(0) {
            match self {
                RandomSource::Legacy(r) => {
                    r.next_int();
                }
                RandomSource::Xoroshiro(r) => {
                    r.next_long();
                }
            }
        }
    }
    /// `fork()`: an independent generator of the same kind, seeded from this one.
    pub fn fork(&mut self) -> RandomSource {
        match self {
            RandomSource::Legacy(r) => RandomSource::Legacy(LegacyRandom::new(r.next_long())),
            RandomSource::Xoroshiro(r) => {
                let lo = r.next_long();
                let hi = r.next_long();
                RandomSource::Xoroshiro(XoroshiroRandom::from_parts(lo, hi))
            }
        }
    }
    /// `forkPositional()`.
    pub fn fork_positional(&mut self) -> PositionalRandomFactory {
        match self {
            RandomSource::Legacy(r) => PositionalRandomFactory::Legacy { seed: r.next_long() },
            RandomSource::Xoroshiro(r) => {
                let lo = r.next_long();
                let hi = r.next_long();
                PositionalRandomFactory::Xoroshiro { lo, hi }
            }
        }
    }
}

/// `PositionalRandomFactory`: deterministic randoms per position or name, from a world seed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PositionalRandomFactory {
    Legacy { seed: i64 },
    Xoroshiro { lo: i64, hi: i64 },
}
impl PositionalRandomFactory {
    pub fn at(&self, x: i32, y: i32, z: i32) -> RandomSource {
        let p = position_seed(x, y, z);
        match *self {
            PositionalRandomFactory::Legacy { seed } => RandomSource::Legacy(LegacyRandom::new(p ^ seed)),
            PositionalRandomFactory::Xoroshiro { lo, hi } => RandomSource::Xoroshiro(XoroshiroRandom::from_parts(p ^ lo, hi)),
        }
    }
    pub fn from_hash_of(&self, name: &str) -> RandomSource {
        match *self {
            PositionalRandomFactory::Legacy { seed } => RandomSource::Legacy(LegacyRandom::new(java_string_hash(name) as i64 ^ seed)),
            PositionalRandomFactory::Xoroshiro { lo, hi } => {
                RandomSource::Xoroshiro(XoroshiroRandom::from_seed128(Seed128::from_hash_of(name).xor(lo, hi)))
            }
        }
    }
    pub fn from_seed(&self, seed: i64) -> RandomSource {
        match *self {
            PositionalRandomFactory::Legacy { .. } => RandomSource::Legacy(LegacyRandom::new(seed)),
            PositionalRandomFactory::Xoroshiro { lo, hi } => RandomSource::Xoroshiro(XoroshiroRandom::from_parts(seed ^ lo, seed ^ hi)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws in the order of the Java fixture program (Leaf's algorithms run on the JVM):
    /// nextInt, nextInt(10), nextInt(100), nextInt(16), nextLong, nextFloat bits, nextDouble bits[, 2 gaussians].
    fn draws(r: &mut RandomSource, gaussians: bool) -> Vec<i64> {
        let mut v = vec![
            r.next_int() as i64,
            r.next_int_bounded(10) as i64,
            r.next_int_bounded(100) as i64,
            r.next_int_bounded(16) as i64,
            r.next_long(),
            r.next_float().to_bits() as i64,
            r.next_double().to_bits() as i64,
        ];
        if gaussians {
            v.push(r.next_gaussian().to_bits() as i64);
            v.push(r.next_gaussian().to_bits() as i64);
        }
        v
    }

    #[test]
    fn matches_leaf_on_the_jvm() {
        let legacy: [(i64, [i64; 9]); 3] = [
            (0, [-1155484576, 8, 29, 9, -6688467811848818630, 1057810800, 4593095635275612160, 4605446429949404853, -4618811753571811926]),
            (42, [-1170105035, 3, 48, 0, 5694868678511409995, 1049484602, 4604549709611139072, -4617305470428694839, -4631177999741404598]),
            (-1234567890123, [7952977, 3, 97, 2, 1535756699810059369, 1065171527, 4592000097566326784, 4593499449827161127, 4607052895455958281]),
        ];
        for (seed, want) in legacy {
            assert_eq!(draws(&mut RandomSource::create(seed), true), want.to_vec(), "legacy seed {seed}");
        }
        let xoro: [(i64, [i64; 7]); 3] = [
            (0, [-160476802, 1, 15, 4, 1839370574944072389, 1061270979, 4605131736735547392]),
            (42, [1774573855, 3, 85, 7, 8419651034331256779, 1059448835, 4601757022554161152]),
            (-1234567890123, [-707066730, 3, 71, 7, -6798054903569452960, 1061103350, 4592619162341212160]),
        ];
        for (seed, want) in xoro {
            assert_eq!(draws(&mut RandomSource::xoroshiro(seed), false), want.to_vec(), "xoroshiro seed {seed}");
        }
    }

    #[test]
    fn seeds_match_leaf() {
        assert_eq!(position_seed(12, -5, 300), -3976170102258);
        assert_eq!(position_seed(-100000, 64, 77777), -63304526991014);
        assert_eq!(Seed128::from_hash_of("minecraft:test"), Seed128 { lo: 1885711875966874721, hi: 3357099975631595687 });
        assert_eq!(java_string_hash("hello"), 99162322);
    }
}
