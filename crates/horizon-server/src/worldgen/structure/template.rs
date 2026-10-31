//! Structure templates (`StructureTemplate`, `StructureTemplateManager`): the `.nbt` files that
//! template structures and jigsaw pieces place, decoded once into owned, immutable data.
//!
//! [`StructureTemplates::load`] reads every template up front, in `StructureTemplateManager`'s
//! source order: the world's `generated/<namespace>/structure/` directory first, then the vanilla
//! data pack's `data/<namespace>/structure/` in the server jar. Vanilla loads a template the first
//! time an id is asked for and caches it (a `ConcurrentHashMap` the workers share); here the map is
//! built before generation starts and only read. A file that fails to decode is skipped (vanilla
//! logs it and tries the next source).
//!
//! Not ported: Paper's data converter (jar templates are already at 26.3's data version; older
//! `generated/` files are read as they are, the user's "no datafixers" decision), `.snbt` sources
//! (only `DEBUG_SAVE_STRUCTURES_AS_SNBT` writes them), saving, and CraftBukkit's
//! `BukkitValues` container (no plugins).

use std::collections::HashMap;
use std::path::Path;

use crate::block::flags::{FLAG_COLLISION_FULL_BLOCK, FLAG_DYNAMIC_SHAPE};
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockState, BlockStateData, BlockStates};
use crate::core::position::BlockPos;
use crate::nbt::{Compound, JavaString, Limits, Tag};
use crate::server::vanilla_jar::{VanillaJar, VanillaJarError};
use crate::util::random::{LegacyRandom, RandomSource, position_seed};
use crate::world::storage::compression::Compression;
use crate::worldgen::density::StructureBounds;

/// `StructureTemplate.StructureBlockInfo`: a position (relative to the template until placed), a
/// state and the block entity's NBT.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfo {
    pub pos: BlockPos,
    pub state: BlockState,
    pub nbt: Option<Box<Compound>>,
}

/// `JigsawBlockEntity.JointType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointType {
    Rollable,
    Aligned,
}

/// `StructureTemplate.JigsawBlockInfo`: a jigsaw block and what its block entity says.
#[derive(Clone, Debug, PartialEq)]
pub struct JigsawInfo {
    pub pos: BlockPos,
    pub state: BlockState,
    pub joint: JointType,
    pub name: String,
    pub pool: String,
    pub target: String,
    pub placement_priority: i32,
    pub selection_priority: i32,
}

/// `StructureTemplate.Palette`: the blocks in placement order and the jigsaws among them (vanilla
/// fills its jigsaw list on first use).
#[derive(Clone, Debug, Default)]
pub struct Palette {
    blocks: Vec<BlockInfo>,
    jigsaws: Vec<JigsawInfo>,
}

impl Palette {
    pub fn blocks(&self) -> &[BlockInfo] {
        &self.blocks
    }
    pub fn jigsaws(&self) -> &[JigsawInfo] {
        &self.jigsaws
    }
}

/// `StructureTemplate.StructureEntityInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityInfo {
    pub pos: [f64; 3],
    pub block_pos: BlockPos,
    pub nbt: Compound,
}

/// A decoded template. `new StructureTemplate()` (what `getOrCreate` makes for an unknown id) is
/// [`StructureTemplate::default`]: no palettes, size zero.
#[derive(Clone, Debug, Default)]
pub struct StructureTemplate {
    size: [i32; 3],
    palettes: Vec<Palette>,
    entities: Vec<EntityInfo>,
}

/// The block facts template decoding needs: block names, the jigsaw block, the list a state goes to.
pub(crate) struct DecodeBlocks<'a> {
    pub blocks: &'a BlockStates,
    pub data: &'a BlockStateData,
}

fn field<'a>(compound: &'a Compound, key: &str) -> Option<&'a Tag> {
    compound.get(&JavaString::from(key))
}

fn list(tag: Option<&Tag>) -> &[Tag] {
    match tag {
        Some(Tag::List(values)) => values,
        _ => &[],
    }
}

/// `ListTag.getIntOr(i, 0)` for the first three elements.
fn ints3(tag: Option<&Tag>) -> [i32; 3] {
    let values = list(tag);
    std::array::from_fn(|i| values.get(i).and_then(Tag::int_value).unwrap_or(0))
}

/// `ListTag.getDoubleOr(i, 0.0)` for the first three elements.
fn doubles3(tag: Option<&Tag>) -> [f64; 3] {
    let values = list(tag);
    std::array::from_fn(|i| match values.get(i) {
        Some(Tag::Double(bits)) => f64::from_bits(*bits),
        Some(Tag::Float(bits)) => f32::from_bits(*bits).into(),
        Some(other) => other.int_value().map_or(0.0, f64::from),
        None => 0.0,
    })
}

fn string(tag: Option<&Tag>) -> Option<String> {
    match tag {
        Some(Tag::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

/// An `Identifier.CODEC` field (`ns:path`, the namespace defaulting to `minecraft`), or `default`.
fn identifier_or(nbt: Option<&Compound>, key: &str, default: &str) -> String {
    nbt.and_then(|nbt| string(field(nbt, key)))
        .and_then(|s| crate::worldgen::codec::identifier(&s).ok())
        .unwrap_or_else(|| default.to_owned())
}

impl DecodeBlocks<'_> {
    /// `NbtUtils.readBlockState`: an unknown or missing block reads as air; an unknown property is
    /// skipped and a bad value keeps the default (vanilla logs it).
    fn read_state(&self, tag: &Tag) -> BlockState {
        let air = self.air();
        let Some(block) = string(tag.get("id"))
            .and_then(|name| crate::worldgen::codec::identifier(&name).ok())
            .and_then(|name| self.blocks.block_by_name(&name))
        else {
            return air;
        };
        let mut state = self.blocks.default_state(block);
        if let Some(Tag::Compound(properties)) = tag.get("properties") {
            for (key, value) in properties {
                let Tag::String(value) = value else { continue };
                let key = key.to_string_lossy();
                if let Some(next) =
                    (self.blocks).with_property(state, &key, &value.to_string_lossy())
                {
                    state = next;
                }
            }
        }
        state
    }

    fn air(&self) -> BlockState {
        let air = self.blocks.block_by_name("minecraft:air").expect("air");
        self.blocks.default_state(air)
    }

    /// `JigsawBlockInfo.parse`, for the jigsaw blocks of a palette (vanilla's `blocks(JIGSAW)`).
    fn jigsaw(&self, info: &BlockInfo) -> JigsawInfo {
        let nbt = info.nbt.as_deref();
        // `getDefaultJointType`: the front of the `orientation` (`front_top`) is horizontal.
        let front = self
            .blocks
            .value(info.state, "orientation")
            .unwrap_or("north_up");
        let horizontal = ["north", "south", "west", "east"]
            .iter()
            .any(|d| front.starts_with(&format!("{d}_")));
        let joint = match nbt.and_then(|n| string(field(n, "joint"))).as_deref() {
            Some("rollable") => JointType::Rollable,
            Some("aligned") => JointType::Aligned,
            _ if horizontal => JointType::Aligned,
            _ => JointType::Rollable,
        };
        let int = |key: &str| {
            nbt.and_then(|n| field(n, key))
                .and_then(Tag::int_value)
                .unwrap_or(0)
        };
        JigsawInfo {
            pos: info.pos,
            state: info.state,
            joint,
            name: identifier_or(nbt, "name", "minecraft:empty"),
            pool: identifier_or(nbt, "pool", "minecraft:empty"),
            target: identifier_or(nbt, "target", "minecraft:empty"),
            placement_priority: int("placement_priority"),
            selection_priority: int("selection_priority"),
        }
    }

    /// `loadPalette`: `addToLists` then `buildInfoList` (full-block collision shapes, then other
    /// blocks, then blocks with NBT, each sorted by y, x, z; the sort is stable).
    fn palette(&self, palette: &[Tag], blocks: &[Tag]) -> Palette {
        let states: Vec<BlockState> = palette.iter().map(|t| self.read_state(t)).collect();
        let air = self.air();
        let mut lists: [Vec<BlockInfo>; 3] = Default::default();
        for block in blocks {
            let Tag::Compound(block) = block else {
                continue;
            };
            let index = field(block, "state").and_then(Tag::int_value).unwrap_or(0);
            let state = usize::try_from(index)
                .ok()
                .and_then(|i| states.get(i).copied())
                .unwrap_or(air);
            let nbt = match field(block, "nbt") {
                Some(Tag::Compound(nbt)) => Some(Box::new(nbt.clone())),
                _ => None,
            };
            let flags = self.data.flags(state);
            let class = if nbt.is_some() {
                2
            } else if flags & FLAG_DYNAMIC_SHAPE == 0 && flags & FLAG_COLLISION_FULL_BLOCK != 0 {
                0
            } else {
                1
            };
            let [x, y, z] = ints3(field(block, "pos"));
            lists[class].push(BlockInfo {
                pos: BlockPos::new(x, y, z),
                state,
                nbt,
            });
        }
        let [mut full, mut other, mut entities] = lists;
        let order = |b: &BlockInfo| (b.pos.y, b.pos.x, b.pos.z);
        full.sort_by_key(order);
        other.sort_by_key(order);
        entities.sort_by_key(order);
        full.append(&mut other);
        full.append(&mut entities);
        let jigsaw = self.blocks.block_by_name("minecraft:jigsaw");
        let jigsaws = full
            .iter()
            .filter(|info| Some(self.blocks.block(info.state)) == jigsaw)
            .map(|info| self.jigsaw(info))
            .collect();
        Palette {
            blocks: full,
            jigsaws,
        }
    }
}

impl StructureTemplate {
    /// `StructureTemplate.load`.
    pub fn from_nbt(root: &Compound, blocks: &BlockStates, data: &BlockStateData) -> Self {
        let decode = DecodeBlocks { blocks, data };
        let size = ints3(field(root, "size"));
        let block_list = list(field(root, "blocks"));
        let palettes = match field(root, "palettes") {
            Some(Tag::List(palettes)) => palettes
                .iter()
                .map(|p| decode.palette(list(Some(p)), block_list))
                .collect(),
            _ => vec![decode.palette(list(field(root, "palette")), block_list)],
        };
        let entities = list(field(root, "entities"))
            .iter()
            .filter_map(|entity| {
                let Tag::Compound(entity) = entity else {
                    return None;
                };
                let Some(Tag::Compound(nbt)) = field(entity, "nbt") else {
                    return None;
                };
                let [x, y, z] = ints3(field(entity, "blockPos"));
                Some(EntityInfo {
                    pos: doubles3(field(entity, "pos")),
                    block_pos: BlockPos::new(x, y, z),
                    nbt: nbt.clone(),
                })
            })
            .collect();
        Self {
            size,
            palettes,
            entities,
        }
    }

    /// `getSize()`.
    pub fn size(&self) -> [i32; 3] {
        self.size
    }
    pub fn palettes(&self) -> &[Palette] {
        &self.palettes
    }
    pub fn entities(&self) -> &[EntityInfo] {
        &self.entities
    }

    /// `getSize(rotation)`: x and z swap for a quarter turn.
    pub fn rotated_size(&self, rotation: Rotation) -> [i32; 3] {
        match rotation {
            Rotation::Clockwise90 | Rotation::Counterclockwise90 => {
                [self.size[2], self.size[1], self.size[0]]
            }
            _ => self.size,
        }
    }

    /// `getZeroPositionWithTransform(zeroPos, mirror, rotation)` with this template's size.
    pub fn zero_position_with_transform(
        &self,
        zero: BlockPos,
        mirror: Mirror,
        rotation: Rotation,
    ) -> BlockPos {
        zero_position_with_transform(zero, mirror, rotation, self.size[0], self.size[2])
    }

    /// `getBoundingBox(position, rotation, pivot, mirror)`.
    pub fn bounding_box(
        &self,
        position: BlockPos,
        rotation: Rotation,
        pivot: BlockPos,
        mirror: Mirror,
    ) -> StructureBounds {
        bounding_box(position, rotation, pivot, mirror, self.size)
    }
}

/// `StructureTemplate.transform(BlockPos, mirror, rotation, pivot)` (Java's wrapping `int`s).
pub fn transform(pos: BlockPos, mirror: Mirror, rotation: Rotation, pivot: BlockPos) -> BlockPos {
    let (mut x, y, mut z) = (pos.x, pos.y, pos.z);
    match mirror {
        Mirror::LeftRight => z = z.wrapping_neg(),
        Mirror::FrontBack => x = x.wrapping_neg(),
        Mirror::None => {}
    }
    let (px, pz) = (pivot.x, pivot.z);
    match rotation {
        Rotation::Counterclockwise90 => BlockPos::new(
            px.wrapping_sub(pz).wrapping_add(z),
            y,
            px.wrapping_add(pz).wrapping_sub(x),
        ),
        Rotation::Clockwise90 => BlockPos::new(
            px.wrapping_add(pz).wrapping_sub(z),
            y,
            pz.wrapping_sub(px).wrapping_add(x),
        ),
        Rotation::Clockwise180 => BlockPos::new(
            px.wrapping_add(px).wrapping_sub(x),
            y,
            pz.wrapping_add(pz).wrapping_sub(z),
        ),
        Rotation::None => BlockPos::new(x, y, z),
    }
}

/// `StructureTemplate.transform(Vec3, mirror, rotation, pivot)`: entity positions.
pub fn transform_vec(
    pos: [f64; 3],
    mirror: Mirror,
    rotation: Rotation,
    pivot: BlockPos,
) -> [f64; 3] {
    let [mut x, y, mut z] = pos;
    match mirror {
        Mirror::LeftRight => z = 1.0 - z,
        Mirror::FrontBack => x = 1.0 - x,
        Mirror::None => {}
    }
    // `pivotX - pivotZ + z` etc.: the int sums first, then the double arithmetic.
    let (px, pz) = (pivot.x, pivot.z);
    match rotation {
        Rotation::Counterclockwise90 => [
            f64::from(px.wrapping_sub(pz)) + z,
            y,
            f64::from(px.wrapping_add(pz).wrapping_add(1)) - x,
        ],
        Rotation::Clockwise90 => [
            f64::from(px.wrapping_add(pz).wrapping_add(1)) - z,
            y,
            f64::from(pz.wrapping_sub(px)) + x,
        ],
        Rotation::Clockwise180 => [
            f64::from(px.wrapping_add(px).wrapping_add(1)) - x,
            y,
            f64::from(pz.wrapping_add(pz).wrapping_add(1)) - z,
        ],
        Rotation::None => [x, y, z],
    }
}

/// `StructureTemplate.getZeroPositionWithTransform(zeroPos, mirror, rotation, sizeX, sizeZ)`.
pub fn zero_position_with_transform(
    zero: BlockPos,
    mirror: Mirror,
    rotation: Rotation,
    size_x: i32,
    size_z: i32,
) -> BlockPos {
    let (sx, sz) = (size_x.wrapping_sub(1), size_z.wrapping_sub(1));
    let mx = if mirror == Mirror::FrontBack { sx } else { 0 };
    let mz = if mirror == Mirror::LeftRight { sz } else { 0 };
    let [dx, dz] = match rotation {
        Rotation::Counterclockwise90 => [mz, sx.wrapping_sub(mx)],
        Rotation::Clockwise90 => [sz.wrapping_sub(mz), mx],
        Rotation::Clockwise180 => [sx.wrapping_sub(mx), sz.wrapping_sub(mz)],
        Rotation::None => [mx, mz],
    };
    zero.offset(dx, 0, dz)
}

/// `StructureTemplate.getBoundingBox(position, rotation, pivot, mirror, size)`.
pub fn bounding_box(
    position: BlockPos,
    rotation: Rotation,
    pivot: BlockPos,
    mirror: Mirror,
    size: [i32; 3],
) -> StructureBounds {
    let delta = BlockPos::new(
        size[0].wrapping_sub(1),
        size[1].wrapping_sub(1),
        size[2].wrapping_sub(1),
    );
    let a = transform(BlockPos::default(), mirror, rotation, pivot);
    let b = transform(delta, mirror, rotation, pivot);
    let moved = |p: BlockPos| {
        [
            p.x.wrapping_add(position.x),
            p.y.wrapping_add(position.y),
            p.z.wrapping_add(position.z),
        ]
    };
    StructureBounds::from_corners(moved(a), moved(b))
}

/// `StructurePlaceSettings.getRandomPalette(palettes, pos)` without a settings random: one
/// `nextInt(size)` of `RandomSource.create(Mth.getSeed(pos))`. `None` without palettes (vanilla
/// throws).
pub fn palette_at(palettes: usize, pos: BlockPos) -> Option<usize> {
    let size = i32::try_from(palettes).ok().filter(|&n| n > 0)?;
    let mut random = LegacyRandom::new(position_seed(pos));
    random.next_int_bound(size).ok().map(|i| i as usize)
}

/// A template that could not be read; the manager skips it as vanilla does.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Nbt(#[from] crate::nbt::Error),
    #[error("the root tag is not a compound")]
    NotCompound,
}

/// `TemplateSource.readStructure`: gzip NBT with no size accounting (`unlimitedHeap`; the byte cap
/// of [`Limits::templates`] is the only guard).
pub fn decode_file(
    bytes: &[u8],
    blocks: &BlockStates,
    data: &BlockStateData,
) -> Result<StructureTemplate, TemplateError> {
    let limits = Limits::templates();
    let raw = Compression::Gzip.decode(bytes, limits.max_bytes)?;
    let (root, _) = crate::nbt::read_named(&raw, limits)?;
    match root.tag {
        Tag::Compound(root) => Ok(StructureTemplate::from_nbt(&root, blocks, data)),
        _ => Err(TemplateError::NotCompound),
    }
}

/// `StructureTemplateManager`'s templates, all decoded at startup.
#[derive(Debug, Default)]
pub struct StructureTemplates {
    templates: HashMap<String, StructureTemplate>,
    /// `listTemplates()`: the `generated/` ids, then the jar's, without repeats.
    order: Vec<String>,
    /// Files that failed to decode (id, source, error), as vanilla's log lines.
    pub failures: Vec<(String, String, String)>,
}

/// `FileToIdConverter("structure", ".nbt")`: `<namespace>/structure/<path>.nbt` to `ns:path`.
fn template_id(namespace: &str, rest: &str) -> Option<String> {
    let path = rest.strip_prefix("structure/")?.strip_suffix(".nbt")?;
    let id = format!("{namespace}:{path}");
    crate::registry::parse_identifier(&id)
        .ok()
        .filter(|p| *p == id)
}

impl StructureTemplates {
    /// Reads the world's `generated` directory (if any) and the jar's vanilla pack. The files are
    /// decoded in parallel (scoped threads), then kept in source order.
    pub fn load(
        jar: &mut VanillaJar,
        generated: Option<&Path>,
        blocks: &BlockStates,
        data: &BlockStateData,
    ) -> Result<Self, VanillaJarError> {
        let mut files: Vec<(String, String, Vec<u8>)> = Vec::new();
        if let Some(dir) = generated {
            let mut found = Vec::new();
            collect_generated(dir, &mut found);
            found.sort();
            for (id, path) in found {
                if let Ok(bytes) = std::fs::read(&path) {
                    files.push((id, path.display().to_string(), bytes));
                }
            }
        }
        let mut names = jar.entry_names();
        names.sort();
        for name in names {
            let Some((namespace, rest)) =
                name.strip_prefix("data/").and_then(|p| p.split_once('/'))
            else {
                continue;
            };
            let Some(id) = template_id(namespace, rest) else {
                continue;
            };
            if let Some(bytes) = jar.read(&name)? {
                files.push((id, "vanilla".to_owned(), bytes));
            }
        }
        Ok(Self::decode_all(files, blocks, data))
    }

    /// Decodes `(id, source, bytes)` in source order: the first file of an id that decodes wins.
    pub fn decode_all(
        files: Vec<(String, String, Vec<u8>)>,
        blocks: &BlockStates,
        data: &BlockStateData,
    ) -> Self {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        let chunk = files.len().div_ceil(threads).max(1);
        let decoded: Vec<Result<StructureTemplate, TemplateError>> = std::thread::scope(|scope| {
            let workers: Vec<_> = files
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|(_, _, bytes)| decode_file(bytes, blocks, data))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().expect("a template decoder panicked"))
                .collect()
        });
        let mut out = Self::default();
        for ((id, source, _), result) in files.into_iter().zip(decoded) {
            if !out.order.contains(&id) {
                out.order.push(id.clone());
            }
            match result {
                Ok(template) => {
                    out.templates.entry(id).or_insert(template);
                }
                Err(e) => out.failures.push((id, source, e.to_string())),
            }
        }
        out
    }

    /// `get(id)`: the template, `None` when no source has it (or none decodes).
    pub fn get(&self, id: &str) -> Option<&StructureTemplate> {
        self.templates.get(id)
    }

    /// `listTemplates()`.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
}

/// `generated/<namespace>/structure/**.nbt` (`DirectoryTemplateSource` over `PathPackResources`).
fn collect_generated(dir: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(namespaces) = std::fs::read_dir(dir) else {
        return;
    };
    for namespace in namespaces.flatten() {
        let Some(ns) = namespace.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let root = namespace.path().join("structure");
        let mut stack = vec![root.clone()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Some(rel) = path
                    .strip_prefix(namespace.path())
                    .ok()
                    .and_then(|r| r.to_str())
                else {
                    continue;
                };
                let rel = rel.replace(std::path::MAIN_SEPARATOR, "/");
                if let Some(id) = template_id(&ns, &rel) {
                    out.push((id, path));
                }
            }
        }
    }
}
