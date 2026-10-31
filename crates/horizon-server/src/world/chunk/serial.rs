//! The 26.3 chunk NBT as Leaf reads and writes it: vanilla's `SerializableChunkData.parse`/`write`
//! with Paper's newer-version guard and Moonrise's light data (Starlight nibbles: `isLightOn` is
//! written false so vanilla relights, `starlight.light_version` marks the light as Moonrise's, and
//! each section may carry `starlight.blocklight_state`/`starlight.skylight_state`).
//!
//! Read and kept as data: coordinates, `Status`, times, sections (block states and biomes through
//! the containers' codec, light arrays and states), heightmaps' raw longs and the light flag.
//! Parts whose systems are not ported yet are kept as the NBT they were read as and written back
//! unchanged ([`RawChunkParts`]); vanilla would re-encode them (unknown structures and far references), which happens once those systems exist.
//! Unknown top-level keys are dropped, as vanilla drops them. Data fixing is not ported: older
//! `DataVersion`s are read as they are (ARCHITECTURE.md, "Scope").

use std::collections::HashMap;

use thiserror::Error;
use tracing::error;

use super::paletted::{PackedData, PaletteValue, PalettedContainer, Strategy};
use super::section::{ContainerFactory, LevelChunkSection, SectionFlags};
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::world::ticks::{SavedTick, filter_ticks_for_chunk, read_saved_ticks, write_saved_ticks};

/// `SharedConstants.getCurrentVersion().dataVersion()` of 26.3.
pub const DATA_VERSION: i32 = 5023;
/// Moonrise's `SaveUtil.STARLIGHT_LIGHT_VERSION`.
pub const STARLIGHT_LIGHT_VERSION: i32 = 10;
const LIGHT_VERSION_TAG: &str = "starlight.light_version";
const BLOCK_LIGHT_STATE_TAG: &str = "starlight.blocklight_state";
const SKY_LIGHT_STATE_TAG: &str = "starlight.skylight_state";

/// `ChunkStatus`, in registry (and generation) order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChunkStatus {
    Empty,
    StructureStarts,
    StructureReferences,
    Biomes,
    Terrain,
    Features,
    InitializeLight,
    Light,
    Spawn,
    Full,
}

impl ChunkStatus {
    pub const ALL: [ChunkStatus; 10] = [
        Self::Empty,
        Self::StructureStarts,
        Self::StructureReferences,
        Self::Biomes,
        Self::Terrain,
        Self::Features,
        Self::InitializeLight,
        Self::Light,
        Self::Spawn,
        Self::Full,
    ];
    /// The registry path (`minecraft:<name>` in saves).
    pub fn name(self) -> &'static str {
        [
            "empty",
            "structure_starts",
            "structure_references",
            "biomes",
            "terrain",
            "features",
            "initialize_light",
            "light",
            "spawn",
            "full",
        ][self as usize]
    }
    /// `ChunkStatus.CODEC`: an identifier of the registry.
    pub fn by_id(id: &str) -> Option<Self> {
        let id = identifier(id).ok()?;
        let path = id.strip_prefix("minecraft:")?;
        Self::ALL.into_iter().find(|s| s.name() == path)
    }
    /// `getChunkType() == LEVELCHUNK`.
    pub fn is_level_chunk(self) -> bool {
        self == Self::Full
    }
}

/// `Heightmap.Types` with their serialization keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heightmap {
    WorldSurfaceWg,
    WorldSurface,
    OceanFloorWg,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

impl Heightmap {
    pub const ALL: [Heightmap; 6] = [
        Self::WorldSurfaceWg,
        Self::WorldSurface,
        Self::OceanFloorWg,
        Self::OceanFloor,
        Self::MotionBlocking,
        Self::MotionBlockingNoLeaves,
    ];
    pub fn key(self) -> &'static str {
        [
            "WORLD_SURFACE_WG",
            "WORLD_SURFACE",
            "OCEAN_FLOOR_WG",
            "OCEAN_FLOOR",
            "MOTION_BLOCKING",
            "MOTION_BLOCKING_NO_LEAVES",
        ][self as usize]
    }
}

/// The biome registry's names in id order (`biomes.asHolderIdMap()` and `holderByNameCodec`).
pub struct BiomeIds {
    names: Vec<Box<str>>,
    ids: HashMap<Box<str>, u32>,
}

impl BiomeIds {
    pub fn new<S: Into<Box<str>>>(names: impl IntoIterator<Item = S>) -> Self {
        let names: Vec<Box<str>> = names.into_iter().map(Into::into).collect();
        let ids = names.iter().zip(0..).map(|(n, i)| (n.clone(), i)).collect();
        Self { names, ids }
    }
    pub fn len(&self) -> u32 {
        self.names.len() as u32
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    pub fn id(&self, name: &str) -> Option<u32> {
        self.ids.get(name).copied()
    }
    pub fn name(&self, id: u32) -> &str {
        &self.names[id as usize]
    }
}

/// A light nibble array (`DataLayer`'s 2048 bytes).
pub type LightData = Box<[u8; 2048]>;

/// `SerializableChunkData.SectionData` with Moonrise's light states (`None` is Moonrise's -1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionData {
    pub y: i32,
    /// `None` outside the level's sections (light-only sections, one below and above).
    pub section: Option<LevelChunkSection>,
    pub block_light: Option<LightData>,
    pub sky_light: Option<LightData>,
    pub block_light_state: Option<i32>,
    pub sky_light_state: Option<i32>,
}

/// The parts of a chunk kept as NBT until their systems are ported (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawChunkParts {
    pub upgrade_data: Option<Tag>,
    pub blending_data: Option<Tag>,
    pub below_zero_retrogen: Option<Tag>,
    /// `block_ticks` and `fluid_ticks`, as `SavedTick`s (delays relative to the time they were
    /// saved at), already filtered to the chunk.
    pub block_ticks: Vec<SavedTick<BlockId>>,
    pub fluid_ticks: Vec<SavedTick<u8>>,
    pub post_processing: Option<Tag>,
    /// Only proto-chunks keep entities in the chunk; full chunks' are in `entities/`.
    pub entities: Vec<Compound>,
    pub block_entities: Vec<Compound>,
    /// Written as `{starts:{},References:{}}` when absent.
    pub structures: Option<Compound>,
    /// CraftBukkit's `ChunkBukkitValues` (persistent data container).
    pub bukkit_values: Option<Compound>,
    /// Paper's `Paper.FailedSpawnAttempts`.
    pub failed_spawn_attempts: Option<Compound>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkData {
    pub pos: ChunkPos,
    pub min_section_y: i32,
    pub last_update: i64,
    pub inhabited_time: i64,
    pub status: ChunkStatus,
    /// Moonrise's reading: lit, by Moonrise, at `light` or later.
    pub light_correct: bool,
    /// Raw words, indexed by `Heightmap as usize`.
    pub heightmaps: [Option<Vec<i64>>; 6],
    pub sections: Vec<SectionData>,
    pub raw: RawChunkParts,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ChunkReadError {
    /// Paper exits the server here (unless `-DPaper.ignoreWorldDataVersion=true`); the caller decides.
    #[error(
        "Server attempted to load chunk saved with newer version of minecraft! {0} > {DATA_VERSION}"
    )]
    NewerVersion(i32),
    /// `ChunkReadException`: a section's container could not be read.
    #[error("{0}")]
    Container(String),
    #[error("DataLayer should be 2048 bytes not: {0}")]
    LightLength(usize),
}

/// What reading and writing chunks needs: the registries and the level's section range.
pub struct ChunkCodec<'a, F> {
    pub blocks: &'a BlockStates,
    pub flags: &'a F,
    pub biomes: &'a BiomeIds,
    pub factory: ContainerFactory,
    pub min_section_y: i32,
    pub max_section_y: i32,
}

/// A container read from NBT with the recoverable errors' message (logged by the caller).
pub type Decoded<T> = Result<(PalettedContainer<T>, Option<String>), String>;

impl<F: SectionFlags> ChunkCodec<'_, F> {
    /// `SerializableChunkData.parse`: `None` without a `Status` string, as vanilla.
    pub fn read(&self, tag: &Compound) -> Result<Option<ChunkData>, ChunkReadError> {
        let Some(Tag::String(status)) = get(tag, "Status") else {
            return Ok(None);
        };
        if let Some(version) = get(tag, "DataVersion").and_then(Tag::int_value)
            && version > DATA_VERSION
        {
            return Err(ChunkReadError::NewerVersion(version));
        }
        let pos = ChunkPos::new(int_or(tag, "xPos", 0), int_or(tag, "zPos", 0));
        let status = ChunkStatus::by_id(&status.to_string_lossy()).unwrap_or(ChunkStatus::Empty);
        let light_correct = status >= ChunkStatus::Light
            && get(tag, "isLightOn").is_some()
            && int_or(tag, LIGHT_VERSION_TAG, -1) == STARLIGHT_LIGHT_VERSION;
        let mut heightmaps: [Option<Vec<i64>>; 6] = Default::default();
        if let Some(Tag::Compound(maps)) = get(tag, "Heightmaps") {
            for kind in Heightmap::ALL {
                if let Some(Tag::LongArray(words)) = get(maps, kind.key()) {
                    heightmaps[kind as usize] = Some(words.clone());
                }
            }
        }
        let compounds = |key| match get(tag, key) {
            Some(Tag::List(list)) => list
                .iter()
                .filter_map(|t| match t {
                    Tag::Compound(c) => Some(c.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let compound = |key| match get(tag, key) {
            Some(Tag::Compound(c)) => Some(c.clone()),
            _ => None,
        };
        let raw = RawChunkParts {
            upgrade_data: compound("UpgradeData").map(Tag::Compound),
            blending_data: get(tag, "blending_data").cloned(),
            below_zero_retrogen: get(tag, "below_zero_retrogen").cloned(),
            block_ticks: chunk_ticks(get(tag, "block_ticks"), pos, |n| self.block(n)),
            fluid_ticks: chunk_ticks(get(tag, "fluid_ticks"), pos, |n| self.fluid(n)),
            post_processing: get(tag, "PostProcessing").cloned(),
            entities: compounds("entities"),
            block_entities: compounds("block_entities"),
            structures: compound("structures"),
            bukkit_values: compound("ChunkBukkitValues"),
            failed_spawn_attempts: compound("Paper.FailedSpawnAttempts"),
        };
        let mut sections = Vec::new();
        if let Some(Tag::List(list)) = get(tag, "sections") {
            for section in list {
                if let Tag::Compound(section) = section {
                    sections.push(self.read_section(pos, section)?);
                }
            }
        }
        Ok(Some(ChunkData {
            pos,
            min_section_y: self.min_section_y,
            last_update: get(tag, "LastUpdate").and_then(long_value).unwrap_or(0),
            inhabited_time: get(tag, "InhabitedTime").and_then(long_value).unwrap_or(0),
            status,
            light_correct,
            heightmaps,
            sections,
            raw,
        }))
    }

    fn read_section(&self, pos: ChunkPos, tag: &Compound) -> Result<SectionData, ChunkReadError> {
        let y = i32::from(get(tag, "Y").and_then(byte_value).unwrap_or(0));
        let section = if (self.min_section_y..=self.max_section_y).contains(&y) {
            // `promotePartial(logErrors)`, then `getOrThrow(ChunkReadException::new)`.
            fn take<T>(
                decoded: Option<Decoded<T>>,
                pos: ChunkPos,
                y: i32,
            ) -> Result<Option<PalettedContainer<T>>, ChunkReadError> {
                let Some(decoded) = decoded else {
                    return Ok(None);
                };
                let (container, partial) = decoded.map_err(ChunkReadError::Container)?;
                if let Some(message) = partial {
                    error!(
                        "Recoverable errors when loading section [{}, {}, {}]: {}",
                        pos.x, y, pos.z, message
                    );
                }
                Ok(Some(container))
            }
            let compound = |key| get(tag, key).filter(|t| matches!(t, Tag::Compound(_)));
            let states = take(
                compound("block_states").map(|t| self.decode_block_states(t)),
                pos,
                y,
            )?
            .unwrap_or_else(|| self.factory.create_for_block_states());
            let biomes = take(compound("biomes").map(|t| self.decode_biomes(t)), pos, y)?
                .unwrap_or_else(|| self.factory.create_for_biomes());
            Some(LevelChunkSection::new(states, biomes, self.flags))
        } else {
            None
        };
        let light = |key| match get(tag, key) {
            Some(Tag::ByteArray(bytes)) => LightData::try_from(bytes.clone().into_boxed_slice())
                .map(Some)
                .map_err(|b| ChunkReadError::LightLength(b.len())),
            _ => Ok(None),
        };
        let state = |key| get(tag, key).map(|t| t.int_value().unwrap_or(0));
        Ok(SectionData {
            y,
            section,
            block_light: light("BlockLight")?,
            sky_light: light("SkyLight")?,
            block_light_state: state(BLOCK_LIGHT_STATE_TAG),
            sky_light_state: state(SKY_LIGHT_STATE_TAG),
        })
    }

    /// The block states codec (`PalettedContainer.codecRW(BlockState.CODEC, ...)`): each palette
    /// entry a block name (its default state) or `{id, properties}`; an unreadable entry becomes
    /// air with a recoverable error, as `ExtraCodecs.orElsePartial` does.
    pub fn decode_block_states(&self, tag: &Tag) -> Decoded<BlockState> {
        decode_container(tag, self.factory.block_states, BlockState::AIR, |entry| {
            self.block_entry(entry)
        })
    }

    /// The biomes codec (`codecRO(biomes.holderByNameCodec(), ...)`); unknown names become the
    /// default biome with a recoverable error.
    pub fn decode_biomes(&self, tag: &Tag) -> Decoded<u32> {
        decode_container(
            tag,
            self.factory.biomes,
            self.factory.default_biome,
            |entry| {
                let Tag::String(name) = entry else {
                    return Err("Not a string".into());
                };
                let id = identifier(&name.to_string_lossy())?;
                self.biomes.id(&id).ok_or_else(|| {
                format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/biome]: {id}"
                )
            })
            },
        )
    }

    /// `BlockState.CODEC`: `Either<Block, BlockState>`, the block by name or the `FULL_CODEC` map.
    fn block_entry(&self, entry: &Tag) -> Result<BlockState, String> {
        let (first, second) = match entry {
            Tag::String(name) => match self.block(&name.to_string_lossy()) {
                Ok(block) => return Ok(self.blocks.default_state(block)),
                Err(first) => (first, format!("Not a map: {entry}")),
            },
            Tag::Compound(map) => match self.full_state(entry, map) {
                Ok(state) => return Ok(state),
                Err(second) => ("Not a string".to_owned(), second),
            },
            _ => ("Not a string".to_owned(), format!("Not a map: {entry}")),
        };
        Err(format!(
            "Failed to parse either. First: {first}; Second: {second}"
        ))
    }

    /// `FULL_CODEC`: `id` dispatches; each property read from `properties` when it is a known
    /// value of the block's, the default's value otherwise; other keys are ignored.
    fn full_state(&self, entry: &Tag, map: &Compound) -> Result<BlockState, String> {
        let name = match get(map, "id") {
            Some(Tag::String(name)) => name.to_string_lossy(),
            Some(_) => return Err("Not a string".into()),
            None => return Err(format!("No key id in MapLike[{entry}]")),
        };
        let block = self.block(&name)?;
        let mut state = self.blocks.default_state(block);
        if let Some(Tag::Compound(properties)) = get(map, "properties") {
            for property in self.blocks.properties(block) {
                let value = match get(properties, self.blocks.property_name(property)) {
                    Some(Tag::String(value)) => value.to_string_lossy(),
                    _ => continue,
                };
                let values = self.blocks.property_values(property);
                if let Some(index) = values.iter().position(|v| **v == *value) {
                    state = self.blocks.try_with_value_index(state, property, index);
                }
            }
        }
        Ok(state)
    }

    /// `BuiltInRegistries.BLOCK.byNameCodec()`.
    fn block(&self, name: &str) -> Result<BlockId, String> {
        let id = identifier(name)?;
        self.blocks.block_by_name(&id).ok_or_else(|| {
            format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {id}")
        })
    }

    /// `BuiltInRegistries.FLUID.byNameCodec()`.
    fn fluid(&self, name: &str) -> Result<u8, String> {
        let id = identifier(name)?;
        self.flags.fluid_by_name(&id).ok_or_else(|| {
            format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:fluid]: {id}")
        })
    }

    /// `SerializableChunkData.write` with Paper's and Moonrise's additions.
    pub fn write(&self, chunk: &ChunkData) -> Compound {
        let mut tag = Compound::new();
        put(&mut tag, "DataVersion", Tag::Int(DATA_VERSION));
        put(&mut tag, "xPos", Tag::Int(chunk.pos.x));
        put(&mut tag, "yPos", Tag::Int(chunk.min_section_y));
        put(&mut tag, "zPos", Tag::Int(chunk.pos.z));
        put(&mut tag, "LastUpdate", Tag::Long(chunk.last_update));
        put(&mut tag, "InhabitedTime", Tag::Long(chunk.inhabited_time));
        let status = format!("minecraft:{}", chunk.status.name());
        put(&mut tag, "Status", Tag::String(status.as_str().into()));
        let raw = &chunk.raw;
        for (key, value) in [
            ("blending_data", &raw.blending_data),
            ("below_zero_retrogen", &raw.below_zero_retrogen),
            ("UpgradeData", &raw.upgrade_data),
        ] {
            if let Some(value) = value {
                put(&mut tag, key, value.clone());
            }
        }
        let sections = chunk.sections.iter().filter_map(|s| self.write_section(s));
        put(&mut tag, "sections", Tag::List(sections.collect()));
        if chunk.light_correct {
            put(&mut tag, "isLightOn", Tag::Byte(1));
        }
        let list =
            |items: &[Compound]| Tag::List(items.iter().cloned().map(Tag::Compound).collect());
        put(&mut tag, "block_entities", list(&raw.block_entities));
        if !chunk.status.is_level_chunk() {
            put(&mut tag, "entities", list(&raw.entities));
        }
        let or_empty_list = |t: &Option<Tag>| t.clone().unwrap_or(Tag::List(Vec::new()));
        put(
            &mut tag,
            "block_ticks",
            write_saved_ticks(&raw.block_ticks, |b| self.blocks.block_name(b).to_owned()),
        );
        put(
            &mut tag,
            "fluid_ticks",
            write_saved_ticks(&raw.fluid_ticks, |f| self.flags.fluid_name(f).to_owned()),
        );
        put(
            &mut tag,
            "PostProcessing",
            or_empty_list(&raw.post_processing),
        );
        let mut heightmaps = Compound::new();
        for kind in Heightmap::ALL {
            if let Some(words) = &chunk.heightmaps[kind as usize] {
                put(&mut heightmaps, kind.key(), Tag::LongArray(words.clone()));
            }
        }
        put(&mut tag, "Heightmaps", Tag::Compound(heightmaps));
        let structures = raw.structures.clone().unwrap_or_else(|| {
            let mut empty = Compound::new();
            put(&mut empty, "starts", Tag::Compound(Compound::new()));
            put(&mut empty, "References", Tag::Compound(Compound::new()));
            empty
        });
        put(&mut tag, "structures", Tag::Compound(structures));
        if let Some(values) = &raw.bukkit_values {
            put(&mut tag, "ChunkBukkitValues", Tag::Compound(values.clone()));
        }
        if let Some(attempts) = &raw.failed_spawn_attempts {
            put(
                &mut tag,
                "Paper.FailedSpawnAttempts",
                Tag::Compound(attempts.clone()),
            );
        }
        // Moonrise: vanilla relights (`isLightOn` false); Moonrise trusts its version tag.
        if chunk.light_correct && chunk.status >= ChunkStatus::Light {
            put(&mut tag, "isLightOn", Tag::Byte(0));
            put(
                &mut tag,
                LIGHT_VERSION_TAG,
                Tag::Int(STARLIGHT_LIGHT_VERSION),
            );
        }
        tag
    }

    fn write_section(&self, section: &SectionData) -> Option<Tag> {
        let mut tag = Compound::new();
        if let Some(s) = &section.section {
            put(
                &mut tag,
                "block_states",
                self.encode_block_states(s.states()),
            );
            let biomes = encode(s.biomes(), |&id| Tag::String(self.biomes.name(id).into()));
            put(&mut tag, "biomes", biomes);
        }
        for (key, light) in [
            ("BlockLight", &section.block_light),
            ("SkyLight", &section.sky_light),
        ] {
            if let Some(light) = light {
                put(&mut tag, key, Tag::ByteArray(light.to_vec()));
            }
        }
        for (key, state) in [
            (BLOCK_LIGHT_STATE_TAG, section.block_light_state),
            (SKY_LIGHT_STATE_TAG, section.sky_light_state),
        ] {
            if let Some(state) = state.filter(|&s| s > 0) {
                put(&mut tag, key, Tag::Int(state));
            }
        }
        if tag.is_empty() {
            return None;
        }
        put(&mut tag, "Y", Tag::Byte(section.y as i8));
        Some(Tag::Compound(tag))
    }

    /// The saved block states: a default state as its block's name, others as `{id, properties}`
    /// with every property (so palettes are mixed lists).
    pub fn encode_block_states(&self, states: &PalettedContainer<BlockState>) -> Tag {
        encode(states, |&state| {
            let block = self.blocks.block(state);
            let name = Tag::String(self.blocks.block_name(block).into());
            if state == self.blocks.default_state(block) {
                return name;
            }
            let mut properties = Compound::new();
            for (key, value) in self.blocks.values(state) {
                put(&mut properties, key, Tag::String(value.into()));
            }
            let mut map = Compound::new();
            put(&mut map, "id", name);
            put(&mut map, "properties", Tag::Compound(properties));
            Tag::Compound(map)
        })
    }
}

/// The containers' codec around an element reader: `palette` (required, a list) and the lenient
/// `data` long stream, then `unpack`. Recoverable errors are joined last first, as DFU's list
/// codec reports them; a fatal error after recoverable ones carries both.
fn decode_container<T: PaletteValue>(
    tag: &Tag,
    strategy: Strategy,
    default: T,
    mut entry: impl FnMut(&Tag) -> Result<T, String>,
) -> Decoded<T> {
    let Tag::Compound(map) = tag else {
        return Err(format!("Not a map: {tag}"));
    };
    let palette = get(map, "palette").ok_or_else(|| format!("No key palette in MapLike[{tag}]"))?;
    let mut errors = Vec::new();
    let palette = elements(palette)
        .ok_or_else(|| format!("Not a list: {palette}"))?
        .iter()
        .map(|t| {
            entry(t).unwrap_or_else(|e| {
                errors.push(format!("({e} -> using default)"));
                default
            })
        })
        .collect();
    let partial = (!errors.is_empty()).then(|| {
        errors.reverse();
        errors.join("; ")
    });
    let data = get(map, "data").and_then(|d| {
        elements(d)?
            .iter()
            .map(long_value)
            .collect::<Option<Vec<i64>>>()
    });
    match PalettedContainer::unpack(strategy, PackedData { palette, data }) {
        Ok(container) => Ok((container, partial)),
        Err(e) => Err(match partial {
            Some(partial) => format!("{partial}; {e}"),
            None => e.to_string(),
        }),
    }
}

fn encode<T: PaletteValue>(container: &PalettedContainer<T>, entry: impl Fn(&T) -> Tag) -> Tag {
    let packed = container.pack();
    let mut tag = Compound::new();
    put(
        &mut tag,
        "palette",
        Tag::List(packed.palette.iter().map(entry).collect()),
    );
    if let Some(data) = packed.data {
        put(&mut tag, "data", Tag::LongArray(data));
    }
    Tag::Compound(tag)
}

/// `NbtOps.getStream`: a list's elements, or an array's as numeric tags.
fn elements(tag: &Tag) -> Option<std::borrow::Cow<'_, [Tag]>> {
    use std::borrow::Cow;
    Some(match tag {
        Tag::List(list) => Cow::Borrowed(list.as_slice()),
        Tag::ByteArray(a) => Cow::Owned(a.iter().map(|&b| Tag::Byte(b as i8)).collect()),
        Tag::IntArray(a) => Cow::Owned(a.iter().map(|&v| Tag::Int(v)).collect()),
        Tag::LongArray(a) => Cow::Owned(a.iter().map(|&v| Tag::Long(v)).collect()),
        _ => return None,
    })
}

/// `Identifier.read`: the normalized `namespace:path`, or `Identifier.read`'s message.
pub fn identifier(input: &str) -> Result<String, String> {
    let (namespace, path) = match input.split_once(':') {
        Some(("", path)) => ("minecraft", path),
        Some((namespace, path)) => {
            let valid = namespace != ".."
                && namespace
                    .chars()
                    .all(|c| matches!(c, '_' | '-' | 'a'..='z' | '0'..='9' | '.'));
            if !valid {
                return Err(format!(
                    "Not a valid resource location: {input} Non [a-z0-9_.-] character in namespace of identifier: {namespace}:{path}"
                ));
            }
            (namespace, path)
        }
        None => ("minecraft", input),
    };
    if !path
        .chars()
        .all(|c| matches!(c, '_' | '-' | 'a'..='z' | '0'..='9' | '/' | '.'))
    {
        return Err(format!(
            "Not a valid resource location: {input} Non [a-z0-9/._-] character in path of location: {namespace}:{path}"
        ));
    }
    Ok(format!("{namespace}:{path}"))
}

/// A tick list as `SerializableChunkData.parse` reads it: decoded, then only the chunk's own.
fn chunk_ticks<T>(
    tag: Option<&Tag>,
    chunk: ChunkPos,
    ty: impl Fn(&str) -> Result<T, String>,
) -> Vec<SavedTick<T>> {
    let mut ticks = read_saved_ticks(tag, ty);
    filter_ticks_for_chunk(&mut ticks, chunk);
    ticks
}

fn get<'a>(map: &'a Compound, key: &str) -> Option<&'a Tag> {
    map.get(&JavaString::from(key))
}
fn put(map: &mut Compound, key: &str, value: Tag) {
    map.insert(key.into(), value);
}

/// `NumericTag.byteValue`.
fn byte_value(tag: &Tag) -> Option<i8> {
    tag.int_value().map(|v| v as i8)
}
/// `NumericTag.longValue`: a float truncates, a double floors.
fn long_value(tag: &Tag) -> Option<i64> {
    Some(match *tag {
        Tag::Long(v) => v,
        Tag::Float(v) => f32::from_bits(v) as i64,
        Tag::Double(v) => f64::from_bits(v).floor() as i64,
        _ => i64::from(tag.int_value()?),
    })
}
fn int_or(map: &Compound, key: &str, default: i32) -> i32 {
    get(map, key).and_then(Tag::int_value).unwrap_or(default)
}
