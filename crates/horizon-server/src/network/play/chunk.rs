//! `ClientboundLevelChunkWithLightPacket` and `ClientboundLightUpdatePacket` from a chunk's parts.
//!
//! Chunk data (`ClientboundLevelChunkPacketData`):
//! - heightmaps: only the types with `Usage.CLIENT` (`WORLD_SURFACE`, `MOTION_BLOCKING`,
//!   `MOTION_BLOCKING_NO_LEAVES`), as a map of type id to raw longs. Vanilla collects them into a
//!   `HashMap` (enum identity hash order); Leaf's "Optimize chunk packet construction" uses an
//!   `EnumMap`, so they go in type id order, as here.
//! - the sections' network form, concatenated (`LevelChunkSection.write`), as one byte array
//!   (at most 2 MiB).
//! - block entities: packed section-relative x/z, y, type id, update tag (Paper sanitizes it
//!   first). Paper ("Handle Oversized block entities in chunks") puts at most
//!   [`BLOCK_ENTITY_LIMIT`] in the packet; each one past it that has an update packet is sent as
//!   its own `block_entity_data` packet after the chunk (the caller's job: pass the first 750 here).
//!
//! Light data (`ClientboundLightUpdatePacketData`, Leaf's rewrite writes the same bytes): for each
//! light section (the level's sections plus one below and one above), each layer is either absent
//! (no bit), empty (a bit in the empty mask) or data (a bit in the mask and its 2048 bytes, in
//! section order). With Moonrise the light engine is Starlight: `getDataLayerData` returns
//! `SWMRNibbleArray.toVanillaNibble()` — see [`LightSection::from_starlight`]. The sky layer is
//! absent altogether (`None`) when the chunk is not light-correct (`isLightCorrect`, i.e. not lit
//! yet), before `ChunkStatus.LIGHT`, or when the level has no sky light (no emptiness map).

use bytes::BufMut;

use super::ids::cb;
use super::start;
use crate::core::position::ChunkPos;
use crate::network::codec::put_var_int;
use crate::world::chunk::LevelChunkSection;

/// `Paper.excessiveTELimit`'s default.
pub const BLOCK_ENTITY_LIMIT: usize = 750;

/// Heightmap type ids (`Heightmap.Types`, the order of `ChunkData::heightmaps`) the client gets.
const CLIENT_HEIGHTMAPS: [usize; 3] = [1, 4, 5];

/// One layer of one light section, as `ClientboundLightUpdatePacketData` sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightSection<'a> {
    /// `getDataLayerData` returned null: no bit.
    Absent,
    /// An empty `DataLayer` (no array): a bit in the empty mask.
    Empty,
    /// A `DataLayer` with an array (even all zero): a bit in the mask and the bytes.
    Data(&'a [u8; 2048]),
}

impl<'a> LightSection<'a> {
    /// `SWMRNibbleArray.toVanillaNibble` for a nibble in `state` (Starlight's visible state:
    /// 0 null, 1 uninitialised, 2 initialised, 3 hidden). Saved chunks give the state from the
    /// section's `starlight.*_state` tag (0 when missing, even with an array) and the array from
    /// `BlockLight`/`SkyLight` (Moonrise's `SaveUtil.loadLightHook`); sections the save does not
    /// list, and every section of an unlit chunk, are uninitialised.
    pub fn from_starlight(state: i32, data: Option<&'a [u8; 2048]>) -> Self {
        match (state, data) {
            (1, _) => Self::Empty,
            (2, Some(data)) => Self::Data(data),
            _ => Self::Absent,
        }
    }
}

/// A `java.util.BitSet` being built. 26.3's `ByteBufCodecs.BIT_SET` writes `toByteArray()`: a
/// byte array, little-endian, trailing zero bytes dropped.
#[derive(Default)]
struct Bits(Vec<u8>);

impl Bits {
    fn set(&mut self, i: usize) {
        if self.0.len() <= i / 8 {
            self.0.resize(i / 8 + 1, 0);
        }
        self.0[i / 8] |= 1 << (i % 8);
    }
    fn write(&self, out: &mut Vec<u8>) {
        put_var_int(out, self.0.len() as i32);
        out.put_slice(&self.0);
    }
}

/// `ClientboundLightUpdatePacketData.STREAM_CODEC` for the given layers (`sky: None` when the
/// sky layer is absent everywhere). A section filter (`*ChangedLightSectionFilter`) is the same as
/// passing [`LightSection::Absent`] for the filtered-out sections.
pub fn write_light_data(out: &mut Vec<u8>, sky: Option<&[LightSection]>, block: &[LightSection]) {
    let mut masks: [Bits; 4] = Default::default();
    let mut add = |layers: &[LightSection], mask: usize| {
        for (i, layer) in layers.iter().enumerate() {
            match layer {
                LightSection::Absent => {}
                LightSection::Empty => masks[mask + 2].set(i),
                LightSection::Data(_) => masks[mask].set(i),
            }
        }
    };
    add(sky.unwrap_or_default(), 0);
    add(block, 1);
    for m in &masks {
        m.write(out);
    }
    for layers in [sky.unwrap_or_default(), block] {
        let data = || {
            layers.iter().filter_map(|l| match l {
                LightSection::Data(d) => Some(d),
                _ => None,
            })
        };
        put_var_int(out, data().count() as i32);
        for d in data() {
            put_var_int(out, 2048);
            out.put_slice(&d[..]);
        }
    }
}

/// `ClientboundLightUpdatePacket` (chunk x/z as VarInts).
pub fn light_update(
    pos: ChunkPos,
    sky: Option<&[LightSection]>,
    block: &[LightSection],
) -> Vec<u8> {
    let mut out = start(cb::LIGHT_UPDATE);
    put_var_int(&mut out, pos.x);
    put_var_int(&mut out, pos.z);
    write_light_data(&mut out, sky, block);
    out
}

/// One `ClientboundLevelChunkPacketData.BlockEntityInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockEntityInfo<'a> {
    /// `(x & 15) << 4 | (z & 15)`: see [`BlockEntityInfo::pack_xz`].
    pub packed_xz: u8,
    pub y: i16,
    /// Id in `minecraft:block_entity_type`.
    pub type_id: i32,
    /// The update tag as network NBT (a nameless compound); `None` when the tag is empty.
    pub tag: Option<&'a [u8]>,
}

impl BlockEntityInfo<'_> {
    pub fn pack_xz(x: i32, z: i32) -> u8 {
        ((x & 15) << 4 | (z & 15)) as u8
    }
}

/// `ClientboundLevelChunkWithLightPacket`: `heightmaps` indexed by `Heightmap.Types` id (the
/// form `ChunkData` keeps), `sections` the level's sections bottom to top, light as
/// [`write_light_data`] takes it.
pub fn level_chunk_with_light(
    pos: ChunkPos,
    heightmaps: &[Option<Vec<i64>>; 6],
    sections: &[LevelChunkSection],
    block_entities: &[BlockEntityInfo],
    sky: Option<&[LightSection]>,
    block: &[LightSection],
) -> Vec<u8> {
    let size: usize = sections
        .iter()
        .map(LevelChunkSection::serialized_size)
        .sum();
    let mut out = Vec::with_capacity(64 + size);
    out.extend_from_slice(&start(cb::LEVEL_CHUNK_WITH_LIGHT));
    out.put_i32(pos.x);
    out.put_i32(pos.z);
    let sent: Vec<(usize, &Vec<i64>)> = CLIENT_HEIGHTMAPS
        .iter()
        .filter_map(|&t| heightmaps[t].as_ref().map(|h| (t, h)))
        .collect();
    put_var_int(&mut out, sent.len() as i32);
    for (t, longs) in sent {
        put_var_int(&mut out, t as i32);
        put_var_int(&mut out, longs.len() as i32);
        for l in longs {
            out.put_i64(*l);
        }
    }
    put_var_int(&mut out, size as i32);
    for s in sections {
        s.write(&mut out);
    }
    put_var_int(&mut out, block_entities.len() as i32);
    for be in block_entities {
        out.put_u8(be.packed_xz);
        out.put_i16(be.y);
        put_var_int(&mut out, be.type_id);
        match be.tag {
            Some(tag) => out.put_slice(tag),
            None => out.put_u8(0),
        }
    }
    write_light_data(&mut out, sky, block);
    out
}
