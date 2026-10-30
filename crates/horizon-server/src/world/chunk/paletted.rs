//! `PalettedContainer` as Leaf runs it: vanilla 26.3's strategies and resizing, with Moonrise's
//! palettes (every list palette is a `FastHashMapPalette`: ids in insertion order, the first id
//! of a value wins), Moonrise's `pack` (the saved form) and block counting order.
//!
//! The values are registry ids (`PaletteValue`): the global palette stores them directly, so the
//! registry itself stays outside; a `Strategy` only knows its size.

use std::collections::HashMap;
use std::io::Cursor;

use thiserror::Error;

use super::bit_storage::{BitStorage, WrongLength};
use crate::azalea::buf::{AzBuf, AzBufVar, BufReadError};

/// A value a container holds: an id into a registry (block states, biomes).
pub trait PaletteValue: Copy + Eq + std::fmt::Debug {
    fn id(self) -> u32;
    fn from_id(id: u32) -> Self;
}
impl PaletteValue for u32 {
    fn id(self) -> u32 {
        self
    }
    fn from_id(id: u32) -> Self {
        id
    }
}
impl PaletteValue for u16 {
    fn id(self) -> u32 {
        u32::from(self)
    }
    fn from_id(id: u32) -> Self {
        id as u16
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `Strategy.createForBlockStates`: 16^3 entries; 4-bit lists up to 16 values, then 5..=8 bits.
    BlockStates,
    /// `Strategy.createForBiomes`: 4^3 entries; lists of 1..=3 bits.
    Biomes,
}

/// `Strategy`: the entry count and the configuration for each bit count. `registry_size` is the
/// global map's size (`Block.BLOCK_STATE_REGISTRY`, the biome registry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strategy {
    kind: Kind,
    registry_size: u32,
    global_bits: u8,
}

/// `Configuration`: a single value, a list palette of `1 << bits` entries, or the global palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Config {
    Single,
    List(u8),
    /// `Configuration.Global(bitsInMemory, bitsInStorage)`: the bits it is saved with.
    Global {
        disk_bits: u8,
    },
}

/// `Mth.ceillog2`.
pub const fn ceil_log2(n: u32) -> u8 {
    if n <= 1 {
        0
    } else {
        (u32::BITS - (n - 1).leading_zeros()) as u8
    }
}

impl Strategy {
    pub const fn block_states(registry_size: u32) -> Self {
        Self::new(Kind::BlockStates, registry_size)
    }
    pub const fn biomes(registry_size: u32) -> Self {
        Self::new(Kind::Biomes, registry_size)
    }
    const fn new(kind: Kind, registry_size: u32) -> Self {
        Self {
            kind,
            registry_size,
            global_bits: ceil_log2(registry_size),
        }
    }
    pub const fn kind(self) -> Kind {
        self.kind
    }
    pub const fn registry_size(self) -> u32 {
        self.registry_size
    }
    /// Bits of the global palette in memory and on the wire (`globalPaletteBitsInMemory`).
    pub const fn global_bits(self) -> u8 {
        self.global_bits
    }
    pub const fn entry_count(self) -> usize {
        match self.kind {
            Kind::BlockStates => 4096,
            Kind::Biomes => 64,
        }
    }
    /// `getIndex`: `(y << axis | z) << axis | x`.
    pub const fn index(self, x: u32, y: u32, z: u32) -> usize {
        let axis = match self.kind {
            Kind::BlockStates => 4,
            Kind::Biomes => 2,
        };
        (((y << axis | z) << axis) | x) as usize
    }
    /// `getConfigurationForBitCount` (the wire's bits byte is signed: negatives fall to global).
    fn config(self, bits: i32) -> Config {
        match (self.kind, bits) {
            (_, 0) => Config::Single,
            (Kind::BlockStates, 1..=4) => Config::List(4),
            (Kind::BlockStates, 5..=8) | (Kind::Biomes, 1..=3) => Config::List(bits as u8),
            _ => Config::Global {
                disk_bits: bits.clamp(0, 32) as u8,
            },
        }
    }
    /// `getConfigurationForPaletteSize`.
    fn config_for_size(self, n: usize) -> Config {
        self.config(i32::from(ceil_log2(n as u32)))
    }
    fn memory_bits(self, config: Config) -> u8 {
        match config {
            Config::Single => 0,
            Config::List(bits) => bits,
            Config::Global { .. } => self.global_bits,
        }
    }
    fn disk_bits(config: Config) -> u8 {
        match config {
            Config::Single => 0,
            Config::List(bits) => bits,
            Config::Global { disk_bits } => disk_bits,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Palette<T> {
    /// `bits == 0`: one value, no storage.
    Single(T),
    /// Up to `1 << bits` values in id order; duplicates (palettes read from disk) are kept.
    List { bits: u8, values: Vec<T> },
    /// Storage holds registry ids.
    Global,
}

/// Errors of the saved form (vanilla's `DataResult` messages where it has them) and of the wire.
#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("Missing values for non-zero storage")]
    MissingData,
    #[error(
        "Failed to read PalettedContainer: Invalid length given for storage, got: {got} but expected: {expected}"
    )]
    WrongLength { got: usize, expected: usize },
    /// Vanilla loads an empty palette and fails on first use ("Use of an uninitialized palette").
    #[error("Use of an uninitialized palette")]
    EmptyPalette,
    /// An index past the palette: vanilla loads it and throws `MissingPaletteEntryException` on
    /// first read; here the load fails.
    #[error("Missing Palette entry for index {0}.")]
    MissingEntry(u32),
    #[error("No value with id {0}")]
    UnknownId(i32),
    #[error("Palette of {len} entries does not fit {bits} bits")]
    PaletteTooLarge { len: i32, bits: u8 },
    #[error(transparent)]
    Read(#[from] BufReadError),
}

impl From<WrongLength> for ContainerError {
    fn from(e: WrongLength) -> Self {
        Self::WrongLength {
            got: e.got,
            expected: e.expected,
        }
    }
}

/// `PalettedContainerRO.PackedData`: what the chunk NBT holds (`palette` list, `data` longs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedData<T> {
    pub palette: Vec<T>,
    pub data: Option<Vec<i64>>,
}

/// Storage ids in first-occurrence order and every entry re-encoded to them (Moonrise's
/// `PaletteReadoutCache.readout`): the order of `pack`'s palette and of block counting.
struct Readout {
    ids: Vec<u32>,
    counts: Vec<u32>,
    reencoded: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PalettedContainer<T> {
    strategy: Strategy,
    palette: Palette<T>,
    storage: BitStorage,
}

impl<T: PaletteValue> PalettedContainer<T> {
    /// `new PalettedContainer(initialValue, strategy)`.
    pub fn new(strategy: Strategy, value: T) -> Self {
        Self {
            strategy,
            palette: Palette::Single(value),
            storage: BitStorage::new(0, strategy.entry_count()),
        }
    }
    /// An empty container in `config`.
    fn with_config(strategy: Strategy, config: Config, single: T) -> Self {
        let bits = strategy.memory_bits(config);
        let palette = match config {
            Config::Single => Palette::Single(single),
            Config::List(bits) => Palette::List {
                bits,
                values: Vec::with_capacity(1 << bits),
            },
            Config::Global { .. } => Palette::Global,
        };
        Self {
            strategy,
            palette,
            storage: BitStorage::new(bits, strategy.entry_count()),
        }
    }
    pub fn strategy(&self) -> Strategy {
        self.strategy
    }
    /// `bitsPerEntry`: bits per stored index, also the wire's leading byte.
    pub fn bits(&self) -> u8 {
        self.storage.bits()
    }
    /// `Palette.getSize`: list entries (unused ones included), the registry size when global.
    pub fn palette_size(&self) -> usize {
        match &self.palette {
            Palette::Single(_) => 1,
            Palette::List { values, .. } => values.len(),
            Palette::Global => self.strategy.registry_size as usize,
        }
    }
    pub fn is_global(&self) -> bool {
        matches!(self.palette, Palette::Global)
    }
    /// `Palette.valueFor`. Stored ids are always in range: loads and reads check them.
    #[inline]
    fn value_for(&self, id: u32) -> T {
        match &self.palette {
            Palette::Single(value) => *value,
            Palette::List { values, .. } => values[id as usize],
            Palette::Global => T::from_id(id),
        }
    }
    #[inline]
    pub fn get(&self, index: usize) -> T {
        match &self.palette {
            Palette::Single(value) => *value,
            Palette::List { values, .. } => values[self.storage.get(index) as usize],
            Palette::Global => T::from_id(self.storage.get(index)),
        }
    }
    pub fn get_xyz(&self, x: u32, y: u32, z: u32) -> T {
        self.get(self.strategy.index(x, y, z))
    }
    /// `getAndSet`: the palette grows first, then the old value is read from the new storage.
    #[inline]
    pub fn get_and_set(&mut self, index: usize, value: T) -> T {
        let id = self.id_for(value);
        let old = self.storage.get_and_set(index, id);
        self.value_for(old)
    }
    #[inline]
    pub fn set(&mut self, index: usize, value: T) {
        let id = self.id_for(value);
        self.storage.set(index, id);
    }
    pub fn get_and_set_xyz(&mut self, x: u32, y: u32, z: u32, value: T) -> T {
        self.get_and_set(self.strategy.index(x, y, z), value)
    }

    /// `Palette.idFor` with the container as resize handler.
    #[inline]
    fn id_for(&mut self, value: T) -> u32 {
        let grow = match &mut self.palette {
            Palette::Single(v) if *v == value => return 0,
            Palette::Single(_) => 1,
            Palette::List { bits, values } => {
                if let Some(id) = values.iter().position(|&v| v == value) {
                    return id as u32;
                }
                if values.len() < 1 << *bits {
                    values.push(value);
                    return values.len() as u32 - 1;
                }
                i32::from(*bits) + 1
            }
            Palette::Global => return self.global_id(value),
        };
        self.resize(grow, value)
    }
    /// `GlobalPalette.idFor`: ids outside the registry become 0.
    fn global_id(&self, value: T) -> u32 {
        let id = value.id();
        if id < self.strategy.registry_size {
            id
        } else {
            0
        }
    }
    /// `idFor` on a palette that is known to have room (`PaletteResize.noResizeExpected`).
    fn add_unchecked(&mut self, value: T) -> u32 {
        match &mut self.palette {
            Palette::Single(_) => 0,
            Palette::List { values, .. } => match values.iter().position(|&v| v == value) {
                Some(id) => id as u32,
                None => {
                    values.push(value);
                    values.len() as u32 - 1
                }
            },
            Palette::Global => self.global_id(value),
        }
    }
    /// `onResize`: a container in the configuration for `bits`, filled from this one in index
    /// order (`Data.copyFrom`), then `last` added.
    fn resize(&mut self, bits: i32, last: T) -> u32 {
        let mut grown = Self::with_config(self.strategy, self.strategy.config(bits), last);
        if self.palette_size() == 1 {
            let id = grown.add_unchecked(self.value_for(0));
            grown.storage.fill(id);
        } else {
            let (mut last_old, mut new_id) = (u32::MAX, 0);
            for i in 0..self.storage.len() {
                let old = self.storage.get(i);
                if old != last_old {
                    last_old = old;
                    new_id = grown.add_unchecked(self.value_for(old));
                }
                grown.storage.set(i, new_id);
            }
        }
        let id = grown.add_unchecked(last);
        *self = grown;
        id
    }

    /// `maybeHas`: any palette entry, used or not; always true for the global palette.
    pub fn maybe_has(&self, mut predicate: impl FnMut(T) -> bool) -> bool {
        match &self.palette {
            Palette::Single(value) => predicate(*value),
            Palette::List { values, .. } => values.iter().any(|&v| predicate(v)),
            Palette::Global => true,
        }
    }
    /// `forEachInPalette`: list entries in id order, the whole registry when global.
    pub fn for_each_in_palette(&self, mut f: impl FnMut(T)) {
        (0..self.palette_size() as u32).for_each(|id| f(self.value_for(id)));
    }
    /// `recreate`: a single-value container of palette entry 0.
    pub fn recreate(&self) -> Self {
        Self::new(self.strategy, self.value_for(0))
    }

    fn readout(&self) -> Readout {
        let n = self.storage.len();
        let mut r = Readout {
            ids: Vec::new(),
            counts: Vec::new(),
            reencoded: Vec::with_capacity(n),
        };
        fn first_seen(r: &mut Readout, id: u32) -> u32 {
            r.ids.push(id);
            r.counts.push(0);
            r.ids.len() as u32 - 1
        }
        if self.bits() <= 12 {
            let mut table = vec![u32::MAX; 1 << self.bits()];
            self.storage.for_each(|id| {
                let slot = &mut table[id as usize];
                if *slot == u32::MAX {
                    *slot = first_seen(&mut r, id);
                }
                let new = *slot;
                r.counts[new as usize] += 1;
                r.reencoded.push(new);
            });
        } else {
            let mut table: HashMap<u32, u32> = HashMap::new();
            self.storage.for_each(|id| {
                let new = match table.get(&id) {
                    Some(&new) => new,
                    None => {
                        let new = first_seen(&mut r, id);
                        table.insert(id, new);
                        new
                    }
                };
                r.counts[new as usize] += 1;
                r.reencoded.push(new);
            });
        }
        r
    }

    /// Moonrise's `countBlocks`: each used palette id once with its count, in order of first
    /// occurrence (one entry for the whole container when the palette has one value).
    pub fn count(&self, mut f: impl FnMut(T, u32)) {
        if self.palette_size() == 1 {
            return f(self.value_for(0), self.storage.len() as u32);
        }
        let r = self.readout();
        for (&id, &count) in r.ids.iter().zip(&r.counts) {
            f(self.value_for(id), count);
        }
    }
    /// The indices whose value satisfies `predicate`, appended to `out` grouped as Moonrise
    /// builds a section's ticking block list: by palette id in first-occurrence order, each
    /// group in index order.
    pub fn positions_where(&self, mut predicate: impl FnMut(T) -> bool, out: &mut Vec<u16>) {
        if self.palette_size() == 1 {
            if predicate(self.value_for(0)) {
                out.extend(0..self.storage.len() as u16);
            }
            return;
        }
        let r = self.readout();
        let mut next: Vec<usize> = Vec::with_capacity(r.ids.len());
        let mut end = out.len();
        for (&id, &count) in r.ids.iter().zip(&r.counts) {
            let wanted = predicate(self.value_for(id));
            next.push(if wanted { end } else { usize::MAX });
            end += if wanted { count as usize } else { 0 };
        }
        out.resize(end, 0);
        for (i, &group) in r.reencoded.iter().enumerate() {
            let slot = &mut next[group as usize];
            if *slot != usize::MAX {
                out[*slot] = i as u16;
                *slot += 1;
            }
        }
    }

    // ---- network form ----

    /// `getSerializedSize`.
    pub fn serialized_size(&self) -> usize {
        let palette = match &self.palette {
            Palette::Single(value) => var_int_size(value.id()),
            Palette::List { values, .. } => {
                var_int_size(values.len() as u32)
                    + values.iter().map(|v| var_int_size(v.id())).sum::<usize>()
            }
            Palette::Global => 0,
        };
        1 + palette + self.storage.words().len() * 8
    }
    /// `write`: the bits byte, the palette (one id, a counted id list, or nothing when global)
    /// and the words without a length.
    pub fn write(&self, out: &mut Vec<u8>) {
        out.reserve(self.serialized_size());
        out.push(self.bits());
        match &self.palette {
            Palette::Single(value) => write_var_int(out, value.id()),
            Palette::List { values, .. } => {
                write_var_int(out, values.len() as u32);
                values.iter().for_each(|v| write_var_int(out, v.id()));
            }
            Palette::Global => {}
        }
        for &word in self.storage.words() {
            out.extend_from_slice(&word.to_be_bytes());
        }
    }
    /// `read`: replaces the contents with a container as `write` sent it. Ids must be in the
    /// registry; unlike vanilla, stored indices past the palette fail here instead of on use.
    pub fn read(&mut self, buf: &mut Cursor<&[u8]>) -> Result<(), ContainerError> {
        let bits = i32::from(u8::azalea_read(buf)? as i8);
        let config = self.strategy.config(bits);
        let registry = self.strategy.registry_size;
        let read_id = |buf: &mut Cursor<&[u8]>| -> Result<T, ContainerError> {
            let id = i32::azalea_read_var(buf)?;
            match u32::try_from(id) {
                Ok(id) if id < registry => Ok(T::from_id(id)),
                _ => Err(ContainerError::UnknownId(id)),
            }
        };
        let palette = match config {
            Config::Single => Palette::Single(read_id(buf)?),
            Config::List(bits) => {
                let len = i32::azalea_read_var(buf)?;
                if !(0..=1 << bits).contains(&len) {
                    return Err(ContainerError::PaletteTooLarge { len, bits });
                }
                let mut values = Vec::with_capacity(1 << bits);
                for _ in 0..len {
                    values.push(read_id(buf)?);
                }
                Palette::List { bits, values }
            }
            Config::Global { .. } => Palette::Global,
        };
        let bits = self.strategy.memory_bits(config);
        let mut storage = BitStorage::new(bits, self.strategy.entry_count());
        for word in storage.words_mut() {
            *word = i64::azalea_read(buf)? as u64;
        }
        let container = Self {
            strategy: self.strategy,
            palette,
            storage,
        };
        container.check_ids()?;
        *self = container;
        Ok(())
    }
    /// Every stored id names a palette entry (a registry id when global).
    fn check_ids(&self) -> Result<(), ContainerError> {
        let limit = self.palette_size() as u32;
        if self.bits() == 0 || (self.bits() < 32 && limit >= 1 << self.bits()) {
            return Ok(());
        }
        let mut bad = None;
        self.storage.for_each(|id| {
            if id >= limit && bad.is_none() {
                bad = Some(id);
            }
        });
        bad.map_or(Ok(()), |id| Err(ContainerError::MissingEntry(id)))
    }

    // ---- saved form ----

    /// `unpack`: a container from the chunk NBT's `palette` (already mapped to ids, unknown names
    /// to the default value as `orElsePartial` does) and `data`.
    pub fn unpack(strategy: Strategy, packed: PackedData<T>) -> Result<Self, ContainerError> {
        let PackedData { palette, data } = packed;
        let Some(&first) = palette.first() else {
            return Err(ContainerError::EmptyPalette);
        };
        let config = strategy.config_for_size(palette.len());
        let disk_bits = Strategy::disk_bits(config);
        if config == Config::Single {
            return Ok(Self::new(strategy, first));
        }
        let words: Vec<u64> = data
            .ok_or(ContainerError::MissingData)?
            .into_iter()
            .map(|w| w as u64)
            .collect();
        let entries = strategy.entry_count();
        let disk = BitStorage::from_words(disk_bits, entries, words)?;
        let container = match config {
            Config::List(bits) => Self {
                strategy,
                palette: Palette::List {
                    bits,
                    values: palette,
                },
                storage: disk,
            },
            _ => {
                // Global: re-encoded from palette indices to registry ids.
                let mut container = Self::with_config(strategy, config, first);
                for i in 0..entries {
                    let index = disk.get(i);
                    let value = *palette
                        .get(index as usize)
                        .ok_or(ContainerError::MissingEntry(index))?;
                    let id = container.global_id(value);
                    container.storage.set(i, id);
                }
                container
            }
        };
        container.check_ids()?;
        Ok(container)
    }

    /// Moonrise's `pack`: the used palette ids in first-occurrence order with the data re-encoded
    /// for the saved bit count; when every palette entry is used and the bits stay, the palette
    /// order and the words are kept as they are.
    pub fn pack(&self) -> PackedData<T> {
        if self.palette_size() == 1 || self.bits() == 0 {
            return PackedData {
                palette: vec![self.value_for(0)],
                data: None,
            };
        }
        let r = self.readout();
        let n = r.ids.len();
        let bits = Strategy::disk_bits(self.strategy.config_for_size(n));
        let to_longs = |words: &[u64]| words.iter().map(|&w| w as i64).collect();
        if bits != 0 && n == self.palette_size() && bits == self.bits() {
            return PackedData {
                palette: (0..n as u32).map(|id| self.value_for(id)).collect(),
                data: Some(to_longs(self.storage.words())),
            };
        }
        PackedData {
            palette: r.ids.iter().map(|&id| self.value_for(id)).collect(),
            data: (bits != 0)
                .then(|| to_longs(BitStorage::from_values(bits, &r.reencoded).words())),
        }
    }
}

fn write_var_int(out: &mut Vec<u8>, value: u32) {
    (value as i32)
        .azalea_write_var(out)
        .expect("writing to a Vec cannot fail");
}

/// `VarInt.getByteSize`.
fn var_int_size(value: u32) -> usize {
    (((32 - value.leading_zeros()) as usize).max(1)).div_ceil(7)
}
