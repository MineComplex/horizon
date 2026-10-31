//! `BlockState.canSurvive(level, pos)` for every block: the rule of the class that implements
//! `canSurvive` (or the `mayPlaceOn` family of `VegetationBlock`), evaluated over the neighbouring
//! states.
//!
//! Which rule a block has comes from the server owner's jar: `LeafBlockStatesExporter.java` maps
//! the block's class to a rule name and writes the names and each state's rule into
//! `block_states.bin` (version 2, see `flags.rs`). The tags the rules read (`supports_cactus`,
//! `supports_vegetation`...) come from the data packs; flammability and fence gates are state
//! flags of the same file. Everything is resolved when [`BlockSurvival::new`] runs.
//!
//! A world answers two questions: the block at a position and `getRawBrightness` (Paper's
//! `WorldGenRegion` answers 0 while a chunk's light is not finished, as it is during features).

use super::flags::{
    FLAG_DYNAMIC_SHAPE, FLAG_FENCE_GATE, FLAG_FLAMMABLE, FLAG_LEGACY_SOLID, FLAG_LIQUID,
};
use super::flags::{FLAG_SOLID_RENDER, SupportType};
use super::{BlockId, BlockState, BlockStateData, BlockStates};
use crate::core::geometry::{Direction, ShapeError, ShapeOp, VoxelShape};

/// A `canSurvive` rule (the exporter's names; `TagBelow` carries its tag in the name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    Fire,
    Cactus,
    Path,
    Mushroom,
    Multiface,
    Vine,
    HangingMoss,
    SnowLayer,
    SeaPickle,
    Bell,
    GrowingPlant,
    MangrovePropagule,
    Speleothem,
    Scaffolding,
    PistonHead,
    ChorusPlant,
    ChorusFlower,
    PitcherCrop,
    SugarCane,
    LilyPad,
    Frogspawn,
    SporeBlossom,
    SmallDripleaf,
    TallSeagrass,
    BigDripleaf,
    BigDripleafStem,
    Farmland,
    Cocoa,
    Crop,
    DoublePlant,
    Door,
    /// The index of the rule's tag in [`BlockSurvival::tags`].
    TagBelow(usize),
    CactusFlower,
    Seagrass,
    BubbleColumn,
    RedstoneWire,
    Always,
    SolidBelow,
    SolidBehind,
    NonAirBelow,
    MossCarpet,
    CenterBelow,
    Lantern,
    FullAbove,
    FullBehind,
    RigidOrCenterBelow,
    RigidBelow,
    FullBelow,
    CenterAbove,
    FaceAttached,
    SupportsBamboo,
}

const RULES: [(&str, Rule); 50] = [
    ("Fire", Rule::Fire),
    ("Cactus", Rule::Cactus),
    ("Path", Rule::Path),
    ("Mushroom", Rule::Mushroom),
    ("Multiface", Rule::Multiface),
    ("Vine", Rule::Vine),
    ("HangingMoss", Rule::HangingMoss),
    ("SnowLayer", Rule::SnowLayer),
    ("SeaPickle", Rule::SeaPickle),
    ("Bell", Rule::Bell),
    ("GrowingPlant", Rule::GrowingPlant),
    ("MangrovePropagule", Rule::MangrovePropagule),
    ("Speleothem", Rule::Speleothem),
    ("Scaffolding", Rule::Scaffolding),
    ("PistonHead", Rule::PistonHead),
    ("ChorusPlant", Rule::ChorusPlant),
    ("ChorusFlower", Rule::ChorusFlower),
    ("PitcherCrop", Rule::PitcherCrop),
    ("SugarCane", Rule::SugarCane),
    ("LilyPad", Rule::LilyPad),
    ("Frogspawn", Rule::Frogspawn),
    ("SporeBlossom", Rule::SporeBlossom),
    ("SmallDripleaf", Rule::SmallDripleaf),
    ("TallSeagrass", Rule::TallSeagrass),
    ("BigDripleaf", Rule::BigDripleaf),
    ("BigDripleafStem", Rule::BigDripleafStem),
    ("Farmland", Rule::Farmland),
    ("Cocoa", Rule::Cocoa),
    ("Crop", Rule::Crop),
    ("DoublePlant", Rule::DoublePlant),
    ("Door", Rule::Door),
    ("CactusFlower", Rule::CactusFlower),
    ("Seagrass", Rule::Seagrass),
    ("BubbleColumn", Rule::BubbleColumn),
    ("RedstoneWire", Rule::RedstoneWire),
    ("Always", Rule::Always),
    ("SolidBelow", Rule::SolidBelow),
    ("SolidBehind", Rule::SolidBehind),
    ("NonAirBelow", Rule::NonAirBelow),
    ("MossCarpet", Rule::MossCarpet),
    ("CenterBelow", Rule::CenterBelow),
    ("Lantern", Rule::Lantern),
    ("FullAbove", Rule::FullAbove),
    ("FullBehind", Rule::FullBehind),
    ("RigidOrCenterBelow", Rule::RigidOrCenterBelow),
    ("RigidBelow", Rule::RigidBelow),
    ("FullBelow", Rule::FullBelow),
    ("CenterAbove", Rule::CenterAbove),
    ("FaceAttached", Rule::FaceAttached),
    ("SupportsBamboo", Rule::SupportsBamboo),
];

/// A sorted set of registry ids (a block or fluid tag's members).
#[derive(Clone, Debug, Default)]
pub struct IdSet(Box<[u32]>);

impl IdSet {
    pub fn new(mut ids: Vec<u32>) -> Self {
        ids.sort_unstable();
        ids.dedup();
        Self(ids.into())
    }
    pub fn contains(&self, id: u32) -> bool {
        self.0.binary_search(&id).is_ok()
    }
}

/// What the rules read from a level.
pub trait SurvivalWorld {
    type Error;
    fn block_state(&self, x: i32, y: i32, z: i32) -> Result<BlockState, Self::Error>;
    /// `getRawBrightness(pos, 0)`.
    fn raw_brightness(&self, x: i32, y: i32, z: i32) -> Result<i32, Self::Error>;
}

/// Why `canSurvive` has no answer.
#[derive(Debug, PartialEq, Eq)]
pub enum SurvivalError<E> {
    World(E),
    /// The state's block has no rule in the file, or a shape query failed.
    Unsupported(&'static str),
}

#[derive(Debug, thiserror::Error)]
#[error("block survival data: {0}")]
pub struct SurvivalDataError(pub String);

/// The tags the rules read, by field of [`BlockSurvival`].
const BLOCK_TAGS: [&str; 24] = [
    "minecraft:supports_cactus",
    "minecraft:overrides_mushroom_light_requirement",
    "minecraft:unstable_bottom_center",
    "minecraft:supports_bamboo",
    "minecraft:support_override_cactus_flower",
    "minecraft:cannot_support_seagrass",
    "minecraft:supports_vegetation",
    "minecraft:supports_big_dripleaf",
    "minecraft:maintains_farmland",
    "minecraft:supports_cocoa",
    "minecraft:supports_crops",
    "minecraft:cannot_support_snow_layer",
    "minecraft:support_override_snow_layer",
    "minecraft:supports_mangrove_propagule",
    "minecraft:supports_hanging_mangrove_propagule",
    "minecraft:cannot_support_kelp",
    "minecraft:supports_chorus_plant",
    "minecraft:supports_chorus_flower",
    "minecraft:supports_lily_pad",
    "minecraft:supports_frogspawn",
    "minecraft:supports_small_dripleaf",
    "minecraft:supports_sugar_cane",
    "minecraft:supports_sugar_cane_adjacently",
    "minecraft:enables_bubble_column_push_up",
];
const TAG_CACTUS: usize = 0;
const TAG_MUSHROOM: usize = 1;
const TAG_UNSTABLE: usize = 2;
const TAG_BAMBOO: usize = 3;
const TAG_CACTUS_FLOWER: usize = 4;
const TAG_NO_SEAGRASS: usize = 5;
const TAG_VEGETATION: usize = 6;
const TAG_DRIPLEAF: usize = 7;
const TAG_FARMLAND: usize = 8;
const TAG_COCOA: usize = 9;
const TAG_CROPS: usize = 10;
const TAG_NO_SNOW: usize = 11;
const TAG_SNOW: usize = 12;
const TAG_MANGROVE: usize = 13;
const TAG_HANGING_MANGROVE: usize = 14;
const TAG_NO_KELP: usize = 15;
const TAG_CHORUS_PLANT: usize = 16;
const TAG_CHORUS_FLOWER: usize = 17;
const TAG_LILY_PAD: usize = 18;
const TAG_FROGSPAWN: usize = 19;
const TAG_SMALL_DRIPLEAF: usize = 20;
const TAG_SUGAR_CANE: usize = 21;
const TAG_SUGAR_CANE_ADJACENT: usize = 22;
const TAG_BUBBLE_UP: usize = 23;
const BUBBLE_DOWN: &str = "minecraft:enables_bubble_column_drag_down";

/// Fluid tags the rules read: water, lily pad, frogspawn, sugar cane, lava.
const FLUID_TAGS: [&str; 5] = [
    "minecraft:water",
    "minecraft:supports_lily_pad",
    "minecraft:supports_frogspawn",
    "minecraft:supports_sugar_cane_adjacently",
    "minecraft:lava",
];

/// Every block's rule with the tags and blocks the rules name. Immutable once built.
#[derive(Debug)]
pub struct BlockSurvival {
    /// By block id: the rule of its states (`None`: no rule in the file).
    rules: Vec<Option<Rule>>,
    /// The block tags above, then `drag_down`, then each `TagBelow` tag.
    tags: Vec<IdSet>,
    fluid_tags: [IdSet; 5],
    chorus_plant: BlockId,
    big_dripleaf: BlockId,
    big_dripleaf_stem: BlockId,
    hopper: BlockId,
    moving_piston: BlockId,
    piston: BlockId,
    sticky_piston: BlockId,
    scaffolding: BlockId,
    kelp_family: [BlockId; 8],
}

fn tag_set(tags: &[(String, Vec<i32>)], name: &str) -> IdSet {
    (tags.iter())
        .find(|(t, _)| t == name)
        .map(|(_, ids)| IdSet::new(ids.iter().map(|&i| i as u32).collect()))
        .unwrap_or_default()
}

impl BlockSurvival {
    /// Resolves the file's rule names and the tags (`block_tags`, `fluid_tags`: tag name to
    /// registry ids, as the feature registry loads them).
    pub fn new(
        blocks: &BlockStates,
        data: &BlockStateData,
        block_tags: &[(String, Vec<i32>)],
        fluid_tags: &[(String, Vec<i32>)],
    ) -> Result<Self, SurvivalDataError> {
        let named = |name: &str| {
            blocks
                .block_by_name(name)
                .ok_or_else(|| SurvivalDataError(format!("no block {name}")))
        };
        let mut tags: Vec<IdSet> = BLOCK_TAGS.iter().map(|t| tag_set(block_tags, t)).collect();
        tags.push(tag_set(block_tags, BUBBLE_DOWN));
        let mut resolved = Vec::new();
        for name in data.survival_rule_names() {
            resolved.push(match name.strip_prefix("TagBelow:") {
                Some(tag) => {
                    tags.push(tag_set(block_tags, tag));
                    Rule::TagBelow(tags.len() - 1)
                }
                None => RULES
                    .iter()
                    .find(|(n, _)| **n == **name)
                    .map(|&(_, r)| r)
                    .ok_or_else(|| SurvivalDataError(format!("unknown rule {name}")))?,
            });
        }
        let rules = (0..blocks.block_count())
            .map(|b| {
                let state = blocks.default_state(BlockId(b));
                data.survival_rule(state).map(|i| resolved[i])
            })
            .collect();
        let kelp = |n: &str| named(n);
        Ok(Self {
            rules,
            tags,
            fluid_tags: FLUID_TAGS.map(|t| tag_set(fluid_tags, t)),
            chorus_plant: named("minecraft:chorus_plant")?,
            big_dripleaf: named("minecraft:big_dripleaf")?,
            big_dripleaf_stem: named("minecraft:big_dripleaf_stem")?,
            hopper: named("minecraft:hopper")?,
            moving_piston: named("minecraft:moving_piston")?,
            piston: named("minecraft:piston")?,
            sticky_piston: named("minecraft:sticky_piston")?,
            scaffolding: named("minecraft:scaffolding")?,
            kelp_family: [
                kelp("minecraft:kelp")?,
                kelp("minecraft:kelp_plant")?,
                kelp("minecraft:cave_vines")?,
                kelp("minecraft:cave_vines_plant")?,
                kelp("minecraft:twisting_vines")?,
                kelp("minecraft:twisting_vines_plant")?,
                kelp("minecraft:weeping_vines")?,
                kelp("minecraft:weeping_vines_plant")?,
            ],
        })
    }

    /// `state.canSurvive(level, pos)`.
    pub fn can_survive<W: SurvivalWorld>(
        &self,
        blocks: &BlockStates,
        data: &BlockStateData,
        world: &W,
        state: BlockState,
        pos: [i32; 3],
    ) -> Result<bool, SurvivalError<W::Error>> {
        let rule = self.rules[blocks.block(state).0 as usize].ok_or(SurvivalError::Unsupported(
            "a block without a survival rule",
        ))?;
        let eval = Eval {
            s: self,
            blocks,
            data,
            world,
            state,
            pos,
            dynamic: Default::default(),
        };
        let answer = eval.run(rule)?;
        if eval.dynamic.get() {
            return Err(SurvivalError::Unsupported(
                "a neighbour with a dynamic shape",
            ));
        }
        Ok(answer)
    }
}

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn direction_name(d: Direction) -> &'static str {
    match d {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

fn shape_error<E>(_: ShapeError) -> SurvivalError<E> {
    SurvivalError::Unsupported("shape query")
}

/// One evaluation: the candidate state at a position.
struct Eval<'a, W: SurvivalWorld> {
    s: &'a BlockSurvival,
    blocks: &'a BlockStates,
    data: &'a BlockStateData,
    world: &'a W,
    state: BlockState,
    pos: [i32; 3],
    /// Set when a rule read a neighbour with a dynamic shape (shulker boxes, moving pistons: no
    /// cached sturdiness; their shape asks the block entity): the answer is then unknown.
    dynamic: std::cell::Cell<bool>,
}

type Out<W> = Result<bool, SurvivalError<<W as SurvivalWorld>::Error>>;

impl<W: SurvivalWorld> Eval<'_, W> {
    fn at(&self, pos: [i32; 3], d: Direction) -> [i32; 3] {
        let n = d.normal();
        [
            pos[0].wrapping_add(n[0]),
            pos[1].wrapping_add(n[1]),
            pos[2].wrapping_add(n[2]),
        ]
    }
    fn read(&self, pos: [i32; 3]) -> Result<BlockState, SurvivalError<W::Error>> {
        (self.world.block_state(pos[0], pos[1], pos[2])).map_err(SurvivalError::World)
    }
    fn light(&self) -> Result<i32, SurvivalError<W::Error>> {
        let p = self.pos;
        (self.world.raw_brightness(p[0], p[1], p[2])).map_err(SurvivalError::World)
    }
    fn tag(&self, tag: usize, state: BlockState) -> bool {
        self.s.tags[tag].contains(self.blocks.block(state).0)
    }
    fn fluid_tag(&self, tag: usize, state: BlockState) -> bool {
        self.s.fluid_tags[tag].contains(self.data.fluid(state).fluid as u32)
    }
    fn block(&self, state: BlockState) -> BlockId {
        self.blocks.block(state)
    }
    fn same(&self, a: BlockState) -> bool {
        self.block(a) == self.block(self.state)
    }
    fn prop(&self, state: BlockState, name: &str) -> Option<&str> {
        self.blocks.value(state, name)
    }
    fn is(&self, state: BlockState, name: &str, value: &str) -> bool {
        self.prop(state, name) == Some(value)
    }
    fn facing(&self) -> Direction {
        match self.prop(self.state, "facing") {
            Some("down") => Direction::Down,
            Some("up") => Direction::Up,
            Some("north") => Direction::North,
            Some("south") => Direction::South,
            Some("west") => Direction::West,
            _ => Direction::East,
        }
    }
    fn upper(&self) -> bool {
        self.is(self.state, "half", "upper")
    }
    fn flag(&self, state: BlockState, flag: u32) -> bool {
        self.data.has(state, flag)
    }
    fn air(&self, state: BlockState) -> bool {
        self.data.is_air(state)
    }
    fn sturdy(&self, state: BlockState, face: Direction, kind: SupportType) -> bool {
        if self.flag(state, FLAG_DYNAMIC_SHAPE) {
            self.dynamic.set(true);
        }
        self.data.is_face_sturdy(state, face, kind)
    }
    /// `BlockState.isFaceSturdy` of the block below (the rules' common support test).
    fn full_below(&self) -> Out<W> {
        let below = self.read(self.at(self.pos, Direction::Down))?;
        Ok(self.sturdy(below, Direction::Up, SupportType::Full))
    }
    fn is_lower_half(&self, below: BlockState) -> bool {
        self.same(below) && self.is(below, "half", "lower")
    }

    /// `Block.canSupportCenter`-style test: `face` of the neighbour opposite the face.
    fn support(&self, face: Direction, kind: SupportType, center_restriction: bool) -> Out<W> {
        let neighbor = self.read(self.at(self.pos, face.opposite()))?;
        if center_restriction && face == Direction::Down && self.tag(TAG_UNSTABLE, neighbor) {
            return Ok(false);
        }
        Ok(self.sturdy(neighbor, face, kind))
    }

    /// `MultifaceBlock.canAttachTo` / `VineBlock.canSupportAtFace`: sturdy face, else the
    /// collision shape's face fills the whole face.
    fn can_attach(&self, neighbor: BlockState, direction: Direction) -> Out<W> {
        let face = direction.opposite();
        if self.sturdy(neighbor, face, SupportType::Full) {
            return Ok(true);
        }
        let shape = self.collision(neighbor)?;
        full_supports(shape, face)
    }
    fn collision(&self, state: BlockState) -> Result<&VoxelShape, SurvivalError<W::Error>> {
        (self.data.collision_shape(state)).ok_or(SurvivalError::Unsupported("a dynamic shape"))
    }
    fn face_attr(&self, state: BlockState, d: Direction) -> bool {
        self.is(state, direction_name(d), "true")
    }
    fn faces(&self) -> Vec<Direction> {
        (Direction::ALL.into_iter())
            .filter(|&d| self.face_attr(self.state, d))
            .collect()
    }

    fn run(&self, rule: Rule) -> Out<W> {
        let pos = self.pos;
        let down = self.at(pos, Direction::Down);
        let up = self.at(pos, Direction::Up);
        let s = self.state;
        Ok(match rule {
            Rule::Always => true,
            Rule::Fire => {
                if self.full_below()? {
                    return Ok(true);
                }
                for d in Direction::ALL {
                    if self.flag(self.read(self.at(pos, d))?, FLAG_FLAMMABLE) {
                        return Ok(true);
                    }
                }
                false
            }
            Rule::Cactus => {
                for d in HORIZONTAL {
                    let n = self.read(self.at(pos, d))?;
                    if self.flag(n, FLAG_LEGACY_SOLID) || self.fluid_tag(4, n) {
                        return Ok(false);
                    }
                }
                let below = self.read(down)?;
                if !self.same(below) && !self.tag(TAG_CACTUS, below) {
                    return Ok(false);
                }
                !self.flag(self.read(up)?, FLAG_LIQUID)
            }
            Rule::Path => {
                let above = self.read(up)?;
                !self.flag(above, FLAG_LEGACY_SOLID) || self.flag(above, FLAG_FENCE_GATE)
            }
            Rule::Mushroom => {
                let below = self.read(down)?;
                if self.tag(TAG_MUSHROOM, below) {
                    return Ok(true);
                }
                self.light()? < 13 && self.flag(below, FLAG_SOLID_RENDER)
            }
            Rule::Multiface => {
                let faces = self.faces();
                for &d in &faces {
                    if !self.can_attach(self.read(self.at(pos, d))?, d)? {
                        return Ok(false);
                    }
                }
                !faces.is_empty()
            }
            Rule::HangingMoss => {
                let n = self.read(up)?;
                self.can_attach(n, Direction::Up)? || self.same(n)
            }
            Rule::Vine => {
                let above = self.read(up)?;
                let mut any = false;
                if self.face_attr(s, Direction::Up) {
                    // getUpdatedState passes DOWN here, unlike canSupportAtFace.
                    any = self.can_attach(above, Direction::Down)?;
                }
                for d in HORIZONTAL {
                    if !self.face_attr(s, d) {
                        continue;
                    }
                    let n = self.read(self.at(pos, d))?;
                    let valid =
                        self.can_attach(n, d)? || (self.same(above) && self.face_attr(above, d));
                    any |= valid;
                }
                any
            }
            Rule::SnowLayer => {
                let n = self.read(down)?;
                if self.tag(TAG_NO_SNOW, n) {
                    return Ok(false);
                }
                if self.tag(TAG_SNOW, n) {
                    return Ok(true);
                }
                full_supports(self.collision(n)?, Direction::Up)?
                    || (self.same(n) && self.is(n, "layers", "8"))
            }
            Rule::SeaPickle => {
                let n = self.read(down)?;
                let face = self.collision(n)?.face_shape(Direction::Up);
                !face.map_err(shape_error)?.is_empty()
                    || self.sturdy(n, Direction::Up, SupportType::Full)
            }
            Rule::Bell => {
                let (face, kind, restrict) = match self.prop(s, "attachment") {
                    Some("floor") => (Direction::Up, SupportType::Full, false),
                    Some("ceiling") => (Direction::Down, SupportType::Center, true),
                    _ => (self.facing().opposite(), SupportType::Full, false),
                };
                self.support(face, kind, restrict)?
            }
            Rule::GrowingPlant => {
                let b = self.block(s);
                let i = self.s.kelp_family.iter().position(|&k| k == b);
                let i = i.ok_or(SurvivalError::Unsupported("growing plant family"))?;
                // Families in order: kelp (grows up), cave vines (down), twisting (up), weeping (down).
                let direction = if i / 2 % 2 == 0 {
                    Direction::Up
                } else {
                    Direction::Down
                };
                let kelp = i / 2 == 0;
                let n = self.read(self.at(pos, direction.opposite()))?;
                if kelp && self.tag(TAG_NO_KELP, n) {
                    return Ok(false);
                }
                let nb = self.block(n);
                nb == self.s.kelp_family[i / 2 * 2]
                    || nb == self.s.kelp_family[i / 2 * 2 + 1]
                    || self.sturdy(n, direction, SupportType::Full)
            }
            Rule::MangrovePropagule => {
                let hanging = self.is(s, "hanging", "true");
                let n = self.read(if hanging { up } else { down })?;
                self.tag(
                    if hanging {
                        TAG_HANGING_MANGROVE
                    } else {
                        TAG_MANGROVE
                    },
                    n,
                )
            }
            Rule::Speleothem => {
                let dir = if self.is(s, "vertical_direction", "up") {
                    Direction::Up
                } else {
                    Direction::Down
                };
                let n = self.read(self.at(pos, dir.opposite()))?;
                self.sturdy(n, dir, SupportType::Full)
                    || (self.same(n)
                        && self.prop(n, "vertical_direction") == self.prop(s, "vertical_direction"))
            }
            Rule::Scaffolding => {
                let n = self.read(down)?;
                let mut distance = if self.block(n) == self.s.scaffolding {
                    self.distance(n)
                } else {
                    if self.sturdy(n, Direction::Up, SupportType::Full) {
                        return Ok(true);
                    }
                    7
                };
                for d in HORIZONTAL {
                    let n = self.read(self.at(pos, d))?;
                    if self.block(n) == self.s.scaffolding {
                        distance = distance.min(self.distance(n) + 1);
                        if distance == 1 {
                            break;
                        }
                    }
                }
                distance < 7
            }
            Rule::PistonHead => {
                let behind = self.read(self.at(pos, self.facing().opposite()))?;
                let base = if self.is(s, "type", "sticky") {
                    self.s.sticky_piston
                } else {
                    self.s.piston
                };
                let b = self.block(behind);
                ((b == base && self.is(behind, "extended", "true")) || b == self.s.moving_piston)
                    && self.prop(behind, "facing") == self.prop(s, "facing")
            }
            Rule::ChorusPlant => {
                let below = self.read(down)?;
                let above = self.read(up)?;
                let crowded = !self.air(above) && !self.air(below);
                for d in HORIZONTAL {
                    let at = self.at(pos, d);
                    if self.same(self.read(at)?) {
                        if crowded {
                            return Ok(false);
                        }
                        let support = self.read(self.at(at, Direction::Down))?;
                        if self.same(support) || self.tag(TAG_CHORUS_PLANT, support) {
                            return Ok(true);
                        }
                    }
                }
                self.same(below) || self.tag(TAG_CHORUS_PLANT, below)
            }
            Rule::ChorusFlower => {
                let below = self.read(down)?;
                if self.block(below) == self.s.chorus_plant || self.tag(TAG_CHORUS_FLOWER, below) {
                    return Ok(true);
                }
                if !self.air(below) {
                    return Ok(false);
                }
                let mut found = false;
                for d in HORIZONTAL {
                    let n = self.read(self.at(pos, d))?;
                    if self.block(n) == self.s.chorus_plant {
                        if found {
                            return Ok(false);
                        }
                        found = true;
                    } else if !self.air(n) {
                        return Ok(false);
                    }
                }
                found
            }
            Rule::PitcherCrop => {
                let upper = self.upper();
                if !upper && self.light()? < 8 {
                    return Ok(false);
                }
                let below = self.read(down)?;
                if upper {
                    self.is_lower_half(below)
                } else {
                    self.tag(TAG_CROPS, below)
                }
            }
            Rule::SugarCane => {
                let n = self.read(down)?;
                if self.same(n) {
                    return Ok(true);
                }
                if !self.tag(TAG_SUGAR_CANE, n) {
                    return Ok(false);
                }
                for d in HORIZONTAL {
                    let adjacent = self.read(self.at(down, d))?;
                    if self.fluid_tag(3, adjacent) || self.tag(TAG_SUGAR_CANE_ADJACENT, adjacent) {
                        return Ok(true);
                    }
                }
                false
            }
            Rule::LilyPad => {
                let n = self.read(down)?;
                let here = self.read(pos)?;
                (self.fluid_tag(1, n) || self.tag(TAG_LILY_PAD, n)) && self.fluid_empty(here)
            }
            Rule::Frogspawn => {
                let n = self.read(down)?;
                let here = self.read(pos)?;
                (self.fluid_tag(2, n) || self.tag(TAG_FROGSPAWN, n)) && self.fluid_empty(here)
            }
            Rule::SporeBlossom => {
                self.support(Direction::Down, SupportType::Center, true)?
                    && !self.fluid_tag(0, self.read(pos)?)
            }
            Rule::SmallDripleaf => {
                let n = self.read(down)?;
                if self.upper() {
                    return Ok(self.is_lower_half(n));
                }
                self.tag(TAG_SMALL_DRIPLEAF, n)
                    || (self.is_water(self.read(pos)?) && self.tag(TAG_VEGETATION, n))
            }
            Rule::TallSeagrass => {
                let n = self.read(down)?;
                if self.upper() {
                    return Ok(self.is_lower_half(n));
                }
                let here = self.data.fluid(self.read(pos)?);
                self.sturdy(n, Direction::Up, SupportType::Full)
                    && !self.tag(TAG_NO_SEAGRASS, n)
                    && self.s.fluid_tags[0].contains(here.fluid as u32)
                    && here.amount == 8
            }
            Rule::BigDripleaf => {
                let n = self.read(down)?;
                let b = self.block(n);
                b == self.s.big_dripleaf
                    || b == self.s.big_dripleaf_stem
                    || self.tag(TAG_DRIPLEAF, n)
            }
            Rule::BigDripleafStem => {
                let (below, above) = (self.read(down)?, self.read(up)?);
                let ab = self.block(above);
                (self.block(below) == self.s.big_dripleaf_stem || self.tag(TAG_DRIPLEAF, below))
                    && (ab == self.s.big_dripleaf || ab == self.s.big_dripleaf_stem)
            }
            Rule::Farmland => {
                let above = self.read(up)?;
                !self.flag(above, FLAG_LEGACY_SOLID) || self.tag(TAG_FARMLAND, above)
            }
            Rule::Cocoa => self.tag(TAG_COCOA, self.read(self.at(pos, self.facing()))?),
            Rule::Crop => self.light()? >= 8 && self.tag(TAG_CROPS, self.read(down)?),
            Rule::DoublePlant => {
                let below = self.read(down)?;
                if self.upper() {
                    self.is_lower_half(below)
                } else {
                    self.tag(TAG_VEGETATION, below)
                }
            }
            Rule::Door => {
                let n = self.read(down)?;
                if self.upper() {
                    self.same(n)
                } else {
                    self.sturdy(n, Direction::Up, SupportType::Full)
                }
            }
            Rule::TagBelow(tag) => {
                let n = self.read(down)?;
                self.s.tags[tag].contains(self.block(n).0)
            }
            Rule::CactusFlower => {
                let n = self.read(down)?;
                self.tag(TAG_CACTUS_FLOWER, n) || self.sturdy(n, Direction::Up, SupportType::Center)
            }
            Rule::Seagrass => {
                let n = self.read(down)?;
                self.sturdy(n, Direction::Up, SupportType::Full) && !self.tag(TAG_NO_SEAGRASS, n)
            }
            Rule::BubbleColumn => {
                let n = self.read(down)?;
                self.same(n) || self.tag(TAG_BUBBLE_UP, n) || self.tag(TAG_BUBBLE_UP + 1, n)
            }
            Rule::RedstoneWire => {
                let n = self.read(down)?;
                self.sturdy(n, Direction::Up, SupportType::Full) || self.block(n) == self.s.hopper
            }
            Rule::SolidBelow => self.flag(self.read(down)?, FLAG_LEGACY_SOLID),
            Rule::SolidBehind => {
                let n = self.read(self.at(pos, self.facing().opposite()))?;
                self.flag(n, FLAG_LEGACY_SOLID)
            }
            Rule::NonAirBelow => !self.air(self.read(down)?),
            Rule::MossCarpet => {
                let n = self.read(down)?;
                if self.is(s, "bottom", "true") {
                    !self.air(n)
                } else {
                    self.same(n) && self.is(n, "bottom", "true")
                }
            }
            Rule::CenterBelow => self.support(Direction::Up, SupportType::Center, true)?,
            Rule::Lantern => {
                let face = if self.is(s, "hanging", "true") {
                    Direction::Down
                } else {
                    Direction::Up
                };
                self.support(face, SupportType::Center, true)?
            }
            Rule::FullAbove => self.support(Direction::Down, SupportType::Full, false)?,
            Rule::FullBehind => self.support(self.facing(), SupportType::Full, false)?,
            Rule::RigidOrCenterBelow => {
                self.support(Direction::Up, SupportType::Rigid, false)?
                    || self.support(Direction::Up, SupportType::Center, true)?
            }
            Rule::RigidBelow => self.support(Direction::Up, SupportType::Rigid, false)?,
            Rule::FullBelow => self.support(Direction::Up, SupportType::Full, false)?,
            Rule::CenterAbove => self.support(Direction::Down, SupportType::Center, false)?,
            Rule::FaceAttached => {
                let face = match self.prop(s, "face") {
                    Some("floor") => Direction::Up,
                    Some("ceiling") => Direction::Down,
                    _ => self.facing(),
                };
                self.support(face, SupportType::Full, false)?
            }
            Rule::SupportsBamboo => self.tag(TAG_BAMBOO, self.read(down)?),
        })
    }

    fn distance(&self, scaffolding: BlockState) -> i32 {
        (self.prop(scaffolding, "distance"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(7)
    }
    fn fluid_empty(&self, state: BlockState) -> bool {
        self.data.fluid(state).fluid == 0
    }
    fn is_water(&self, state: BlockState) -> bool {
        matches!(
            self.data.fluid_name(self.data.fluid(state).fluid),
            "minecraft:water"
        )
    }
}

/// `Block.isFaceFull(shape, face)` (`SupportType.FULL.isSupporting`): whether the shape's face
/// covers the whole face of the block. A shape the cache cannot answer for counts as not full.
pub fn shape_fills_face(shape: &VoxelShape, face: Direction) -> bool {
    full_supports::<()>(shape, face).unwrap_or(false)
}

/// `MultifaceBlock.canAttachTo` / `VineBlock.isAcceptableNeighbour` for the neighbour state that
/// `direction` leads to: a sturdy face, else a collision shape whose face fills the block's.
/// `None` for a state with a dynamic shape (its shape asks a block entity).
pub fn can_attach_to(
    data: &BlockStateData,
    neighbor: BlockState,
    direction: Direction,
) -> Option<bool> {
    let face = direction.opposite();
    if data.is_face_sturdy(neighbor, face, SupportType::Full) {
        return Some(true);
    }
    data.collision_shape(neighbor)
        .map(|shape| shape_fills_face(shape, face))
}

/// `SupportType.FULL.isSupporting(shape, face)`.
fn full_supports<E>(shape: &VoxelShape, face: Direction) -> Result<bool, SurvivalError<E>> {
    let face = shape.face_shape(face).map_err(shape_error)?;
    let differs = face
        .join_is_not_empty(&VoxelShape::block(), ShapeOp::XOR)
        .map_err(shape_error)?;
    Ok(!differs)
}
