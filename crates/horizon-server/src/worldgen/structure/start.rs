//! Structure starts and references as a chunk holds and saves them: `StructureStart`
//! (`createTag`, `loadStaticStart`), `SerializableChunkData`'s `structures` tag
//! (`packStructureData`, `unpackStructureStart`, `unpackStructureReferences`) and the
//! STRUCTURE_REFERENCES step (`ChunkGenerator.createReferences`).
//!
//! Jigsaw pieces (`minecraft:jigsaw`, `PoolElementStructurePiece`) are [`JigsawPiece`]s, saved and
//! loaded as vanilla does (a loaded piece's box is its element's, as vanilla recomputes it). Pieces of
//! the other types stay as their saved NBT until those types are ported; a start's bounding box is
//! computed from their `BB` boxes, which is all references need. A child without a valid `BB` (or a
//! jigsaw piece whose element is not in the pools) is dropped, as `PiecesContainer.load` drops a
//! piece that fails to load.
//!
//! Leaf (`0211-optimize-structure-map`) keeps a chunk's starts and references in insertion-ordered
//! array maps, and a generated chunk's references for one structure in a `LongArraySet` (insertion
//! order); references read from a save are a `LongOpenHashSet`, so their order is that set's
//! ([`hash_set_order`]). The order matters: the FEATURES step places starts in it. Loaded starts
//! come from a `HashMap` keyed by identity hashes in Leaf (no fixed order); here they keep the
//! order of their ids.

use super::jigsaw::{JigsawPiece, Junction};
use super::legacy_piece::LegacyPiece;
use super::place::LiquidSettings;
use super::pool::{Projection, liquid_name};
use super::{StructureId, StructureRegistry};
use crate::block::transform::Rotation;
use crate::core::position::BlockPos;
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::registry::parse_identifier;
use crate::worldgen::density::{StructureBounds, TerrainAdjustment};

/// `StructureStart.INVALID_START_ID`.
const INVALID_START_ID: &str = "INVALID";
/// CraftBukkit's persistent data of a start, kept as saved.
const BUKKIT_VALUES: &str = "StructureBukkitValues";

/// A structure start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureStart {
    /// `None` for `INVALID_START`.
    pub structure: Option<StructureId>,
    pub chunk: ChunkPos,
    /// How often a locating search has taken this start (`addReference`).
    pub references: i32,
    /// The pieces, in order (the first is the centre piece `placeInChunk` takes its reference from).
    pub children: Vec<StructurePiece>,
    /// `StructureStart.getBoundingBox`: the pieces' box, inflated by 12 for a terrain-adapting
    /// structure; `None` without pieces.
    pub bounding_box: Option<StructureBounds>,
    /// CraftBukkit's `StructureBukkitValues`, written back as read.
    pub bukkit_values: Option<Compound>,
}

/// A structure piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructurePiece {
    /// `PoolElementStructurePiece` (`minecraft:jigsaw`).
    Jigsaw(JigsawPiece),
    /// A piece that places its blocks in code (`legacy_piece`).
    Legacy(LegacyPiece),
    /// A piece of a type not ported yet, as saved (`StructurePiece.createTag`).
    Saved(Compound),
}

impl StructurePiece {
    /// `getBoundingBox`.
    pub fn bounding_box(&self) -> Option<StructureBounds> {
        match self {
            Self::Jigsaw(piece) => Some(piece.bounding_box),
            Self::Legacy(piece) => Some(piece.bounding_box().to_bounds()),
            Self::Saved(tag) => piece_box(tag),
        }
    }

    /// `StructurePiece.createTag`.
    pub fn create_tag(&self, registry: &StructureRegistry) -> Compound {
        match self {
            Self::Saved(tag) => tag.clone(),
            Self::Jigsaw(piece) => jigsaw_tag(registry, piece),
            Self::Legacy(piece) => piece.create_tag(),
        }
    }
}

/// `Rotation.LEGACY_CODEC`: the enum constant's name.
fn rotation_name(rotation: Rotation) -> &'static str {
    [
        "NONE",
        "CLOCKWISE_90",
        "CLOCKWISE_180",
        "COUNTERCLOCKWISE_90",
    ][rotation as usize]
}

fn box_tag(b: StructureBounds) -> Tag {
    let ([x0, y0, z0], [x1, y1, z1]) = (b.minimum(), b.maximum());
    Tag::IntArray(vec![x0, y0, z0, x1, y1, z1])
}

/// `PoolElementStructurePiece.addAdditionalSaveData` after `StructurePiece.createTag`'s own fields
/// (no orientation: `O` is -1; generation depth 0).
fn jigsaw_tag(registry: &StructureRegistry, piece: &JigsawPiece) -> Compound {
    let junctions = (piece.junctions.iter())
        .map(|j| {
            Tag::compound([
                ("source_x", Tag::Int(j.source_x)),
                ("source_ground_y", Tag::Int(j.source_ground_y)),
                ("source_z", Tag::Int(j.source_z)),
                ("delta_y", Tag::Int(j.delta_y)),
                ("dest_proj", Tag::String(j.dest_projection.name().into())),
            ])
        })
        .collect();
    let element = registry.pools.element_tag(piece.element);
    let mut tag = Compound::from([
        ("id".into(), Tag::String("minecraft:jigsaw".into())),
        ("BB".into(), box_tag(piece.bounding_box)),
        ("O".into(), Tag::Int(-1)),
        ("GD".into(), Tag::Int(0)),
        ("PosX".into(), Tag::Int(piece.position.x)),
        ("PosY".into(), Tag::Int(piece.position.y)),
        ("PosZ".into(), Tag::Int(piece.position.z)),
        (
            "ground_level_delta".into(),
            Tag::Int(piece.ground_level_delta),
        ),
        ("pool_element".into(), Tag::Compound(element)),
        (
            "rotation".into(),
            Tag::String(rotation_name(piece.rotation).into()),
        ),
        ("junctions".into(), Tag::List(junctions)),
    ]);
    if piece.liquid != LiquidSettings::ApplyWaterlogging {
        tag.insert(
            "liquid_settings".into(),
            Tag::String(liquid_name(piece.liquid).into()),
        );
    }
    tag
}

/// `new PoolElementStructurePiece(context, tag)`: `None` (logged) when the element is unknown.
fn load_jigsaw(registry: &StructureRegistry, tag: &Compound) -> Option<JigsawPiece> {
    let get = |key: &str| tag.get(&JavaString::from(key));
    let int = |key: &str| get(key).and_then(Tag::int_value).unwrap_or(0);
    let text = |tag: Option<&Tag>| match tag {
        Some(Tag::String(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    let failed = |why: &str| {
        tracing::error!("Exception loading structure piece with id minecraft:jigsaw: {why}");
    };
    let Some(element) = (match get("pool_element") {
        Some(Tag::Compound(element)) => registry.pools.find_element(element),
        _ => None,
    }) else {
        failed("Invalid pool element found");
        return None;
    };
    let rotation = match text(get("rotation")).as_str() {
        "NONE" => Rotation::None,
        "CLOCKWISE_90" => Rotation::Clockwise90,
        "CLOCKWISE_180" => Rotation::Clockwise180,
        "COUNTERCLOCKWISE_90" => Rotation::Counterclockwise90,
        _ => {
            failed("No rotation");
            return None;
        }
    };
    let position = BlockPos::new(int("PosX"), int("PosY"), int("PosZ"));
    let junctions = match get("junctions") {
        Some(Tag::List(list)) => list
            .iter()
            .map(|j| {
                let field = |key: &str| j.get(key).and_then(Tag::int_value).unwrap_or(0);
                Junction {
                    source_x: field("source_x"),
                    source_ground_y: field("source_ground_y"),
                    source_z: field("source_z"),
                    delta_y: field("delta_y"),
                    dest_projection: Projection::by_name(&text(j.get("dest_proj")))
                        .unwrap_or(Projection::TerrainMatching),
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let liquid = match text(get("liquid_settings")).as_str() {
        "ignore_waterlogging" => LiquidSettings::IgnoreWaterlogging,
        _ => LiquidSettings::ApplyWaterlogging,
    };
    Some(JigsawPiece {
        element,
        position,
        ground_level_delta: int("ground_level_delta"),
        rotation,
        bounding_box: (registry.pools).bounding_box(
            &registry.templates,
            element,
            position,
            rotation,
        ),
        junctions,
        liquid,
    })
}

/// A piece tag's `BB` (`BoundingBox.CODEC`: six ints).
fn piece_box(piece: &Compound) -> Option<StructureBounds> {
    match piece.get(&JavaString::from("BB"))? {
        Tag::IntArray(values) => match values[..] {
            [x0, y0, z0, x1, y1, z1] => {
                Some(StructureBounds::from_corners([x0, y0, z0], [x1, y1, z1]))
            }
            _ => None,
        },
        _ => None,
    }
}

impl StructureStart {
    /// `INVALID_START`.
    pub fn invalid() -> Self {
        Self {
            structure: None,
            chunk: ChunkPos::new(0, 0),
            references: 0,
            children: Vec::new(),
            bounding_box: None,
            bukkit_values: None,
        }
    }

    /// A start of `structure` made of `children` (each with its `BB`).
    pub fn new(
        registry: &StructureRegistry,
        structure: StructureId,
        chunk: ChunkPos,
        references: i32,
        children: Vec<StructurePiece>,
    ) -> Self {
        let boxes = children.iter().filter_map(StructurePiece::bounding_box);
        let bounding_box =
            boxes.reduce(StructureBounds::encapsulating).map(|bounds| {
                match registry.structure(structure).terrain_adaptation {
                    TerrainAdjustment::None => bounds,
                    _ => bounds.inflated_by(12),
                }
            });
        Self {
            structure: Some(structure),
            chunk,
            references,
            children,
            bounding_box,
            bukkit_values: None,
        }
    }

    /// `isValid`: it has pieces.
    pub fn is_valid(&self) -> bool {
        !self.children.is_empty()
    }

    /// `canBeReferenced` (`getMaxReferences` is 1).
    pub fn can_be_referenced(&self) -> bool {
        self.references < 1
    }

    /// `createTag` in the chunk `chunk` holding it.
    pub fn create_tag(&self, registry: &StructureRegistry, chunk: ChunkPos) -> Compound {
        let mut tag = Compound::new();
        if let Some(values) = self.bukkit_values.as_ref().filter(|v| !v.is_empty()) {
            tag.insert(BUKKIT_VALUES.into(), Tag::Compound(values.clone()));
        }
        match self.structure.filter(|_| self.is_valid()) {
            Some(structure) => {
                let id = registry.structure(structure).id.as_str();
                tag.insert("id".into(), Tag::String(id.into()));
                tag.insert("ChunkX".into(), Tag::Int(chunk.x));
                tag.insert("ChunkZ".into(), Tag::Int(chunk.z));
                tag.insert("references".into(), Tag::Int(self.references));
                let children = (self.children.iter())
                    .map(|piece| Tag::Compound(piece.create_tag(registry)))
                    .collect();
                tag.insert("Children".into(), Tag::List(children));
            }
            None => {
                tag.insert("id".into(), Tag::String(INVALID_START_ID.into()));
            }
        }
        tag
    }

    /// `loadStaticStart`: `Ok(None)` (logged) for an unknown structure id; `Err` for a malformed
    /// one, where `Identifier.parse` throws and the chunk's load fails.
    pub fn load(registry: &StructureRegistry, tag: &Compound) -> Result<Option<Self>, String> {
        let get = |key: &str| tag.get(&JavaString::from(key));
        let id = match get("id") {
            Some(Tag::String(id)) => id.to_string_lossy(),
            _ => String::new(),
        };
        if id == INVALID_START_ID {
            return Ok(Some(Self::invalid()));
        }
        let name = parse_identifier(&id)?;
        let Some(structure) = registry.structure_id(&name) else {
            tracing::error!("Unknown stucture id: {id}");
            return Ok(None);
        };
        let int = |key: &str| get(key).and_then(Tag::int_value).unwrap_or(0);
        let chunk = ChunkPos::new(int("ChunkX"), int("ChunkZ"));
        let children = match get("Children") {
            Some(Tag::List(children)) => children
                .iter()
                .filter_map(|child| {
                    let piece = match child {
                        Tag::Compound(piece) => piece,
                        _ => &Compound::new(),
                    };
                    // `PiecesContainer.load`: the id lower-cased, then its `RENAMES`.
                    let kind = match piece.get(&JavaString::from("id")) {
                        Some(Tag::String(id)) => id.to_string_lossy().to_lowercase(),
                        _ => String::new(),
                    };
                    if matches!(
                        parse_identifier(&kind).unwrap_or_default().as_str(),
                        "minecraft:jigsaw"
                            | "minecraft:nvi"
                            | "minecraft:pcp"
                            | "minecraft:bastionremnant"
                            | "minecraft:runtime"
                    ) {
                        return load_jigsaw(registry, piece).map(StructurePiece::Jigsaw);
                    }
                    let id = parse_identifier(&kind).unwrap_or_default();
                    if LegacyPiece::handles(&id) {
                        let loaded = LegacyPiece::load(&id, piece);
                        if loaded.is_none() {
                            tracing::error!("Exception loading structure piece with id {id}");
                        }
                        return loaded.map(StructurePiece::Legacy);
                    }
                    let Some(bounds) = piece_box(piece) else {
                        tracing::error!(
                            "Exception loading structure piece of {id}: no bounding box"
                        );
                        return None;
                    };
                    // A loaded piece saves its box as the constructor fixed it.
                    let [x0, y0, z0] = bounds.minimum();
                    let [x1, y1, z1] = bounds.maximum();
                    let mut piece = piece.clone();
                    piece.insert("BB".into(), Tag::IntArray(vec![x0, y0, z0, x1, y1, z1]));
                    Some(StructurePiece::Saved(piece))
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(Some(Self::new(
            registry,
            structure,
            chunk,
            int("references"),
            children,
        )))
    }
}

/// A chunk's structure starts and references (`ChunkAccess.structureStarts`,
/// `structureReferences`), in Leaf's orders (module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkStructures {
    starts: Vec<(StructureId, StructureStart)>,
    /// Chunk keys (`ChunkPos.pack`) of the starts referenced, per structure.
    references: Vec<(StructureId, Vec<i64>)>,
}

impl ChunkStructures {
    /// `getStartForStructure`.
    pub fn start(&self, structure: StructureId) -> Option<&StructureStart> {
        let (_, start) = self.starts.iter().find(|(s, _)| *s == structure)?;
        Some(start)
    }

    /// `getAllStarts`.
    pub fn starts(&self) -> impl Iterator<Item = (StructureId, &StructureStart)> {
        self.starts.iter().map(|(s, start)| (*s, start))
    }

    /// `setStartForStructure`.
    pub fn set_start(&mut self, structure: StructureId, start: StructureStart) {
        match self.starts.iter_mut().find(|(s, _)| *s == structure) {
            Some((_, slot)) => *slot = start,
            None => self.starts.push((structure, start)),
        }
    }

    /// `getReferencesForStructure`.
    pub fn references(&self, structure: StructureId) -> &[i64] {
        (self.references.iter())
            .find(|(s, _)| *s == structure)
            .map_or(&[], |(_, keys)| keys.as_slice())
    }

    /// `getAllReferences`.
    pub fn all_references(&self) -> impl Iterator<Item = (StructureId, &[i64])> {
        self.references
            .iter()
            .map(|(s, keys)| (*s, keys.as_slice()))
    }

    /// `addReferenceForStructure`: a set, so a key already there stays where it is.
    pub fn add_reference(&mut self, structure: StructureId, key: i64) {
        let keys = match self.references.iter().position(|(s, _)| *s == structure) {
            Some(i) => &mut self.references[i].1,
            None => {
                self.references.push((structure, Vec::new()));
                &mut self.references.last_mut().expect("just pushed").1
            }
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    /// The `structures` tag (`packStructureData`): every start under its structure's id, and
    /// the non-empty reference sets as long arrays.
    pub fn save(&self, registry: &StructureRegistry, chunk: ChunkPos) -> Compound {
        let starts = (self.starts.iter())
            .map(|(s, start)| {
                let id = registry.structure(*s).id.as_str();
                (id.into(), Tag::Compound(start.create_tag(registry, chunk)))
            })
            .collect();
        let references = (self.references.iter())
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(s, keys)| {
                (
                    registry.structure(*s).id.as_str().into(),
                    Tag::LongArray(keys.clone()),
                )
            })
            .collect();
        Compound::from([
            ("starts".into(), Tag::Compound(starts)),
            ("References".into(), Tag::Compound(references)),
        ])
    }

    /// The `structures` tag of the chunk at `chunk` (`unpackStructureStart`,
    /// `unpackStructureReferences`): unknown structures are logged and dropped, as are references
    /// more than 8 chunks away. A start whose inner `id` is malformed fails the load.
    pub fn load(
        registry: &StructureRegistry,
        tag: &Compound,
        chunk: ChunkPos,
    ) -> Result<Self, String> {
        let compound = |tag: &Compound, key: &str| match tag.get(&JavaString::from(key)) {
            Some(Tag::Compound(inner)) => inner.clone(),
            _ => Compound::new(),
        };
        let lookup = |key: &JavaString| {
            let name = parse_identifier(&key.to_string_lossy()).ok()?;
            registry.structure_id(&name)
        };
        let mut structures = Self::default();
        for (key, value) in &compound(tag, "starts") {
            let Some(structure) = lookup(key) else {
                tracing::error!("Unknown structure start: {}", key.to_string_lossy());
                continue;
            };
            let start_tag = match value {
                Tag::Compound(start) => start.clone(),
                _ => Compound::new(),
            };
            if let Some(mut start) = StructureStart::load(registry, &start_tag)? {
                if let Some(Tag::Compound(values)) = start_tag.get(&JavaString::from(BUKKIT_VALUES))
                {
                    start.bukkit_values = Some(values.clone());
                }
                structures.set_start(structure, start);
            }
        }
        for (key, value) in &compound(tag, "References") {
            let Some(structure) = lookup(key) else {
                tracing::warn!(
                    "Found reference to unknown structure '{}' in chunk {chunk:?}, discarding",
                    key.to_string_lossy()
                );
                continue;
            };
            let Tag::LongArray(keys) = value else {
                continue;
            };
            let keys: Vec<i64> = (keys.iter().copied())
                .filter(|&key| {
                    let reference = ChunkPos::unpack(key);
                    let distance = (reference.x.wrapping_sub(chunk.x).wrapping_abs())
                        .max(reference.z.wrapping_sub(chunk.z).wrapping_abs());
                    if distance > 8 {
                        tracing::warn!(
                            "Found invalid structure reference [ {} @ {reference:?} ] for chunk {chunk:?}.",
                            key_name(registry, structure)
                        );
                    }
                    distance <= 8
                })
                .collect();
            let ordered = hash_set_order(&keys);
            let slot = match structures
                .references
                .iter()
                .position(|(s, _)| *s == structure)
            {
                Some(i) => &mut structures.references[i].1,
                None => {
                    structures.references.push((structure, Vec::new()));
                    &mut structures.references.last_mut().expect("just pushed").1
                }
            };
            *slot = ordered;
        }
        Ok(structures)
    }
}

fn key_name(registry: &StructureRegistry, structure: StructureId) -> &str {
    &registry.structure(structure).id
}

/// `ChunkGenerator.createReferences` for the chunk `chunk`: every valid start within 8 chunks
/// whose box meets the chunk's columns, as (structure, start chunk key) in the order they are
/// added (source x, then z, then each chunk's starts in order). `starts_at` gives a chunk's
/// starts (STRUCTURE_STARTS is done in all of them).
pub fn create_references<'a>(
    chunk: ChunkPos,
    starts_at: impl Fn(ChunkPos) -> Option<&'a ChunkStructures>,
) -> Vec<(StructureId, i64)> {
    let (min_x, min_z) = (chunk.x << 4, chunk.z << 4);
    let mut references = Vec::new();
    for source_x in chunk.x.wrapping_sub(8)..=chunk.x.wrapping_add(8) {
        for source_z in chunk.z.wrapping_sub(8)..=chunk.z.wrapping_add(8) {
            let source = ChunkPos::new(source_x, source_z);
            let Some(structures) = starts_at(source) else {
                continue;
            };
            for (structure, start) in structures.starts() {
                let meets = start.bounding_box.is_some_and(|bounds| {
                    bounds.intersects_columns(min_x, min_z, min_x + 15, min_z + 15)
                });
                if start.is_valid() && meets {
                    references.push((structure, source.pack()));
                }
            }
        }
    }
    references
}

/// The iteration order of fastutil's `LongOpenHashSet(long[])` (default load factor) built from
/// `keys`: the keys, deduplicated, as `unpackStructureReferences`' sets give them back. The zero
/// key comes first, then the table from its last slot down.
pub fn hash_set_order(keys: &[i64]) -> Vec<i64> {
    const PHI: u64 = 0x9E37_79B9_7F4A_7C15;
    let mix = |key: i64| {
        let mut h = (key as u64).wrapping_mul(PHI);
        h ^= h >> 32;
        (h ^ (h >> 16)) as u32 as usize
    };
    // `HashCommon.arraySize` and `maxFill` for load factor 0.75.
    let array_size = |expected: usize| (expected * 4).div_ceil(3).next_power_of_two().max(2);
    let max_fill = |n: usize| (n * 3).div_ceil(4).min(n - 1);
    let insert = |table: &mut [Option<i64>], key: i64| {
        let mask = table.len() - 1;
        let mut pos = mix(key) & mask;
        while let Some(present) = table[pos] {
            if present == key {
                return false;
            }
            pos = (pos + 1) & mask;
        }
        table[pos] = Some(key);
        true
    };
    let mut table = vec![None; array_size(keys.len())];
    let mut zero = false;
    let mut size = 0;
    for &key in keys {
        let added = if key == 0 {
            !std::mem::replace(&mut zero, true)
        } else {
            insert(&mut table, key)
        };
        if !added {
            continue;
        }
        size += 1;
        if size > max_fill(table.len()) {
            let mut grown = vec![None; array_size(size + 1)];
            for key in table.iter().rev().flatten() {
                insert(&mut grown, *key);
            }
            table = grown;
        }
    }
    let zero = zero.then_some(0);
    zero.into_iter()
        .chain(table.iter().rev().flatten().copied())
        .collect()
}
