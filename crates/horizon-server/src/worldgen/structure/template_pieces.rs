//! Template structure pieces (`TemplateStructurePiece` and its subclasses `ShipwreckPiece`,
//! `OceanRuinPiece`, `IglooPiece`, `RuinedPortalPiece`, `NetherFossilPiece`, `EndCityPiece`,
//! `WoodlandMansionPiece`): one [`TemplatePiece`] with a [`PieceKind`] for what differs, its saved
//! form (`createTag` and the loading constructors) and its placement (`postProcess`, data markers).
//!
//! What a piece does besides writing blocks comes back in [`PieceOutput`], in order: the templates'
//! block entities and entities ([`PlacedTemplate`]), the loot tables the pieces set on containers
//! (`setCraftLootTable`, `createChest`; the world keeps no block entities yet, so a container is a
//! block state with the randomizable-container flag) and the mobs the data markers spawn (entities
//! are not ported). `postProcess` mutates the piece as vanilla does (a shipwreck's height, an igloo's
//! box), and shares the chunk's writable box, which a nether fossil and a ruined portal grow.
//!
//! Leaf's CraftBukkit hooks (`placeCraftBlockEntity`, `setCraftLootTable`) end in the same writes as
//! vanilla when no plugin transforms structures, so vanilla's order of writes and draws is ported.

use std::borrow::Cow;

use super::piece_place::{PieceTables, provider_error};
use super::place::{
    LiquidSettings, PlaceContext, PlaceSettings, PlacedTemplate, filter_blocks, place_in_world,
};
use super::pool::template_of;
use super::processor::{
    BlockEntityModifier, PosRuleTest, Processor, ProcessorLists, ProcessorRule,
};
use super::template::{StructureTemplate, bounding_box as template_box, transform};
use super::{BiomeSet, StructureRegistry};
use crate::block::flags::FLAG_RANDOMIZABLE_CONTAINER;
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockId, BlockState, BlockStates};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::registry::parse_identifier;
use crate::util::random::RandomSource;
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::feature::rule_test::RuleTest;
use crate::worldgen::placement::DecodeContext;
use crate::worldgen::state_provider::ProviderError;

/// `Block.UPDATE_CLIENTS`: how `TemplateStructurePiece.postProcess` places.
const UPDATE_CLIENTS: i32 = 2;
/// `Block.UPDATE_ALL`: `setBlockAndUpdate`.
const UPDATE_ALL: i32 = 3;

/// The piece type ids (`StructurePieceType`, lower-cased).
pub const SHIPWRECK_ID: &str = "minecraft:shipwreck";
pub const OCEAN_RUIN_ID: &str = "minecraft:orp";
pub const IGLOO_ID: &str = "minecraft:iglu";
pub const RUINED_PORTAL_ID: &str = "minecraft:rupo";
pub const NETHER_FOSSIL_ID: &str = "minecraft:nefos";
pub const END_CITY_ID: &str = "minecraft:ecp";
pub const MANSION_ID: &str = "minecraft:wmp";

/// Whether a piece type id is one of the template pieces.
pub fn is_template_piece(id: &str) -> bool {
    matches!(
        id,
        SHIPWRECK_ID
            | OCEAN_RUIN_ID
            | IGLOO_ID
            | RUINED_PORTAL_ID
            | NETHER_FOSSIL_ID
            | END_CITY_ID
            | MANSION_ID
    )
}

/// `RuinedPortalPiece.VerticalPlacement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalPlacement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
    InNether,
}

impl VerticalPlacement {
    pub const ALL: [Self; 6] = [
        Self::OnLandSurface,
        Self::PartlyBuried,
        Self::OnOceanFloor,
        Self::InMountain,
        Self::Underground,
        Self::InNether,
    ];

    pub fn name(self) -> &'static str {
        [
            "on_land_surface",
            "partly_buried",
            "on_ocean_floor",
            "in_mountain",
            "underground",
            "in_nether",
        ][self as usize]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// `RuinedPortalPiece.Properties`; the float is kept as its bits so pieces compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortalProperties {
    pub cold: bool,
    pub mossiness_bits: u32,
    pub air_pocket: bool,
    pub overgrown: bool,
    pub vines: bool,
    pub replace_with_blackstone: bool,
}

impl PortalProperties {
    pub fn mossiness(&self) -> f32 {
        f32::from_bits(self.mossiness_bits)
    }
}

/// What differs between the template pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Shipwreck {
        beached: bool,
        height_adjusted: bool,
    },
    OceanRuin {
        cold: bool,
        /// `integrity`'s bits.
        integrity_bits: u32,
        large: bool,
    },
    Igloo,
    RuinedPortal {
        placement: VerticalPlacement,
        properties: PortalProperties,
    },
    NetherFossil,
    EndCity {
        /// `OW`: the ignore processor leaves air in (`BlockIgnoreProcessor.STRUCTURE_BLOCK`).
        overwrite: bool,
    },
    Mansion,
}

/// The blocks pieces name, by id (`BlockId(u32::MAX)` for one the table lacks).
#[derive(Clone, Debug)]
pub struct PieceBlocks {
    pub air: BlockId,
    pub structure_block: BlockId,
    pub chest: BlockId,
    pub water: BlockId,
    pub lava: BlockId,
    pub obsidian: BlockId,
    pub netherrack: BlockId,
    pub magma_block: BlockId,
    pub gold_block: BlockId,
    pub vine: BlockId,
    pub jungle_leaves: BlockId,
    pub snow_block: BlockId,
    pub ladder: BlockId,
    pub cobblestone: BlockId,
    pub soul_sand: BlockId,
    pub dried_ghast: BlockId,
}

/// What pieces build their place settings from, resolved once with the processor lists.
#[derive(Clone, Debug)]
pub struct PieceBuiltins {
    /// `OceanRuinPieces.WARM_SUSPICIOUS_BLOCK_PROCESSOR`.
    pub warm_archy: Processor,
    /// `OceanRuinPieces.COLD_SUSPICIOUS_BLOCK_PROCESSOR`.
    pub cold_archy: Processor,
    /// `BlockTags.FEATURES_CANNOT_REPLACE`, as block ids.
    pub cannot_replace: BiomeSet,
    /// `BlockTags.ICE`.
    pub ice: BiomeSet,
    pub blocks: PieceBlocks,
    /// Default states the ruined portal's rules write.
    pub air_state: BlockState,
    pub magma_state: BlockState,
    pub netherrack_state: BlockState,
}

impl PieceBuiltins {
    pub fn new(names: &DecodeContext<'_>) -> Self {
        let blocks = names.blocks;
        let id = |name: &str| {
            blocks
                .block_by_name(&format!("minecraft:{name}"))
                .unwrap_or(BlockId(u32::MAX))
        };
        let state = |name: &str| {
            blocks
                .block_by_name(&format!("minecraft:{name}"))
                .map_or(BlockState(0), |b| blocks.default_state(b))
        };
        let archy = |candidate: &str, replacement: &str, table: &str| {
            let json = format!(
                r#"{{"processor_type":"minecraft:capped","limit":5,"delegate":{{"processor_type":"minecraft:rule","rules":[{{"input_predicate":{{"predicate_type":"minecraft:block_match","block":"minecraft:{candidate}"}},"location_predicate":{{"predicate_type":"minecraft:always_true"}},"position_predicate":{{"predicate_type":"minecraft:always_true"}},"output_state":{{"id":"minecraft:{replacement}"}},"block_entity_modifier":{{"type":"minecraft:append_loot","loot_table":"minecraft:{table}"}}}}]}}}}"#
            );
            serde_json::from_str::<Box<serde_json::value::RawValue>>(&json)
                .ok()
                .and_then(|raw| Processor::decode(&raw, names).ok())
                .unwrap_or(Processor::Nop)
        };
        Self {
            warm_archy: archy("sand", "suspicious_sand", "archaeology/ocean_ruin_warm"),
            cold_archy: archy("gravel", "suspicious_gravel", "archaeology/ocean_ruin_cold"),
            cannot_replace: names
                .block_tag("minecraft:features_cannot_replace")
                .unwrap_or_default(),
            ice: names.block_tag("minecraft:ice").unwrap_or_default(),
            air_state: state("air"),
            magma_state: state("magma_block"),
            netherrack_state: state("netherrack"),
            blocks: PieceBlocks {
                air: id("air"),
                structure_block: id("structure_block"),
                chest: id("chest"),
                water: id("water"),
                lava: id("lava"),
                obsidian: id("obsidian"),
                netherrack: id("netherrack"),
                magma_block: id("magma_block"),
                gold_block: id("gold_block"),
                vine: id("vine"),
                jungle_leaves: id("jungle_leaves"),
                snow_block: id("snow_block"),
                ladder: id("ladder"),
                cobblestone: id("cobblestone"),
                soul_sand: id("soul_sand"),
                dried_ghast: id("dried_ghast"),
            },
        }
    }
}

impl Default for PieceBuiltins {
    fn default() -> Self {
        let none = BlockId(u32::MAX);
        Self {
            warm_archy: Processor::Nop,
            cold_archy: Processor::Nop,
            cannot_replace: BiomeSet::default(),
            ice: BiomeSet::default(),
            air_state: BlockState(0),
            magma_state: BlockState(0),
            netherrack_state: BlockState(0),
            blocks: PieceBlocks {
                air: none,
                structure_block: none,
                chest: none,
                water: none,
                lava: none,
                obsidian: none,
                netherrack: none,
                magma_block: none,
                gold_block: none,
                vine: none,
                jungle_leaves: none,
                snow_block: none,
                ladder: none,
                cobblestone: none,
                soul_sand: none,
                dried_ghast: none,
            },
        }
    }
}

/// What the pieces of a placement left for the ports that do not exist yet, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PieceOutput {
    pub events: Vec<PieceEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PieceEvent {
    /// A template's block entities and entities.
    Template(PlacedTemplate),
    /// A randomizable container given a loot table and seed (`setLootTable(table, random.nextLong())`).
    Loot {
        pos: BlockPos,
        table: &'static str,
        seed: i64,
    },
    /// A mob the data markers spawn (`EntityType.create` + `addFreshEntity`); `group` for
    /// "Group of Allays", whose count comes from the level's random.
    Spawn {
        entity: &'static str,
        pos: BlockPos,
        group: bool,
    },
}

/// Everything `postProcess` reads and writes besides the piece.
pub struct PieceCx<'a, 'w, W, R> {
    pub tables: PieceTables<'a>,
    pub world: &'w mut W,
    pub random: &'w mut R,
    /// The chunk's writable box (`chunkBB`), shared by every piece of the pass.
    pub chunk_box: &'w mut StructureBounds,
    pub reference: BlockPos,
    pub out: &'w mut PieceOutput,
}

/// A template structure piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplatePiece {
    pub kind: PieceKind,
    /// `templateName` as saved (`Template`): the full id for most pieces, the short name for an end
    /// city or mansion piece.
    pub template: String,
    /// `templatePosition`.
    pub position: BlockPos,
    pub rotation: Rotation,
    pub mirror: Mirror,
    /// The place settings' rotation pivot.
    pub pivot: BlockPos,
    pub bounding_box: StructureBounds,
    /// `genDepth` (`GD`).
    pub gen_depth: i32,
}

fn rotation_name(rotation: Rotation) -> &'static str {
    [
        "NONE",
        "CLOCKWISE_90",
        "CLOCKWISE_180",
        "COUNTERCLOCKWISE_90",
    ][rotation as usize]
}

fn mirror_name(mirror: Mirror) -> &'static str {
    ["NONE", "LEFT_RIGHT", "FRONT_BACK"][mirror as usize]
}

fn rotation_by_name(name: &str) -> Option<Rotation> {
    Rotation::ALL
        .into_iter()
        .find(|&r| rotation_name(r) == name)
}

fn mirror_by_name(name: &str) -> Option<Mirror> {
    Mirror::ALL.into_iter().find(|&m| mirror_name(m) == name)
}

pub(super) fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

fn bool_tag(value: bool) -> Tag {
    Tag::Byte(i8::from(value))
}

/// `IglooPieces.PIVOTS`.
fn igloo_pivot(location: &str) -> Option<BlockPos> {
    Some(match location {
        "minecraft:igloo/top" => BlockPos::new(3, 5, 5),
        "minecraft:igloo/middle" => BlockPos::new(1, 3, 1),
        "minecraft:igloo/bottom" => BlockPos::new(3, 6, 7),
        _ => return None,
    })
}

/// `IglooPieces.OFFSETS`.
pub(super) fn igloo_offset(location: &str) -> Option<BlockPos> {
    Some(match location {
        "minecraft:igloo/top" => BlockPos::new(0, 0, 0),
        "minecraft:igloo/middle" => BlockPos::new(2, -3, 4),
        "minecraft:igloo/bottom" => BlockPos::new(0, -3, -2),
        _ => return None,
    })
}

/// `IglooPieces.PIVOTS` for the piece's template.
pub(super) fn igloo_pivot_of(location: &str) -> Option<BlockPos> {
    igloo_pivot(location)
}

/// `ShipwreckPieces.PIVOT`.
pub const SHIPWRECK_PIVOT: BlockPos = BlockPos::new(4, 0, 15);

impl TemplatePiece {
    /// A new piece: its box is the template's at `position` (`getBoundingBox(placeSettings, position)`).
    pub fn new(
        registry: &StructureRegistry,
        kind: PieceKind,
        template: String,
        position: BlockPos,
        rotation: Rotation,
        mirror: Mirror,
        pivot: BlockPos,
    ) -> Self {
        let mut piece = Self {
            kind,
            template,
            position,
            rotation,
            mirror,
            pivot,
            bounding_box: StructureBounds::from_corners([0; 3], [0; 3]),
            gen_depth: 0,
        };
        piece.bounding_box = piece.template_box(registry);
        piece
    }

    /// The template's location (`makeTemplateLocation`).
    pub fn location(&self) -> String {
        match self.kind {
            PieceKind::EndCity { .. } => format!("minecraft:end_city/{}", self.template),
            PieceKind::Mansion => format!("minecraft:woodland_mansion/{}", self.template),
            _ => self.template.clone(),
        }
    }

    /// The piece type's id.
    pub fn type_id(&self) -> &'static str {
        match self.kind {
            PieceKind::Shipwreck { .. } => SHIPWRECK_ID,
            PieceKind::OceanRuin { .. } => OCEAN_RUIN_ID,
            PieceKind::Igloo => IGLOO_ID,
            PieceKind::RuinedPortal { .. } => RUINED_PORTAL_ID,
            PieceKind::NetherFossil => NETHER_FOSSIL_ID,
            PieceKind::EndCity { .. } => END_CITY_ID,
            PieceKind::Mansion => MANSION_ID,
        }
    }

    pub fn template<'t>(&self, registry: &'t StructureRegistry) -> &'t StructureTemplate {
        template_of(&registry.templates, &self.location())
    }

    /// `template.getBoundingBox(placeSettings, templatePosition)`.
    pub fn template_box(&self, registry: &StructureRegistry) -> StructureBounds {
        template_box(
            self.position,
            self.rotation,
            self.pivot,
            self.mirror,
            self.template(registry).size(),
        )
    }

    /// `StructurePiece.move`.
    pub fn move_by(&mut self, dx: i32, dy: i32, dz: i32) {
        let (min, max) = (self.bounding_box.minimum(), self.bounding_box.maximum());
        self.bounding_box = StructureBounds::from_corners(
            [min[0] + dx, min[1] + dy, min[2] + dz],
            [max[0] + dx, max[1] + dy, max[2] + dz],
        );
        self.position = self.position.offset(dx, dy, dz);
    }

    /// `calculateRelativePosition(placeSettings, pos)`.
    pub fn relative_position(&self, pos: BlockPos) -> BlockPos {
        transform(pos, self.mirror, self.rotation, self.pivot)
    }

    /// The piece's place settings (`placeSettings`) before `postProcess` sets the box.
    pub fn settings<'a>(&self, lists: &'a ProcessorLists) -> PlaceSettings<'a> {
        let known = lists.known();
        let builtins = lists.pieces();
        let mut settings = PlaceSettings {
            rotation: self.rotation,
            mirror: self.mirror,
            rotation_pivot: self.pivot,
            ..PlaceSettings::default()
        };
        match self.kind {
            PieceKind::Shipwreck { .. } | PieceKind::NetherFossil => {
                settings
                    .processors
                    .push(Cow::Owned(known.ignore_structure_and_air()));
            }
            PieceKind::Igloo => {
                settings
                    .processors
                    .push(Cow::Owned(known.ignore_structure_block()));
                settings.liquid = LiquidSettings::IgnoreWaterlogging;
            }
            PieceKind::OceanRuin {
                cold,
                integrity_bits,
                ..
            } => {
                settings.processors.push(Cow::Owned(Processor::BlockRot {
                    rottable: None,
                    integrity: f32::from_bits(integrity_bits),
                }));
                settings
                    .processors
                    .push(Cow::Owned(known.ignore_structure_and_air()));
                settings.processors.push(Cow::Borrowed(if cold {
                    &builtins.cold_archy
                } else {
                    &builtins.warm_archy
                }));
            }
            PieceKind::RuinedPortal {
                placement,
                properties,
            } => {
                let b = &builtins.blocks;
                let rule = |input: RuleTest, output: BlockState| ProcessorRule {
                    input,
                    location: RuleTest::AlwaysTrue,
                    position: PosRuleTest::AlwaysTrue,
                    output,
                    modifier: BlockEntityModifier::Passthrough,
                };
                let (air, magma, netherrack) = (
                    builtins.air_state,
                    builtins.magma_state,
                    builtins.netherrack_state,
                );
                let mut rules = vec![
                    rule(RuleTest::RandomBlock(b.gold_block.0, 0.3), air),
                    // `getLavaProcessorRule`
                    if placement == VerticalPlacement::OnOceanFloor {
                        rule(RuleTest::Block(b.lava.0), magma)
                    } else if properties.cold {
                        rule(RuleTest::Block(b.lava.0), netherrack)
                    } else {
                        rule(RuleTest::RandomBlock(b.lava.0, 0.2), magma)
                    },
                ];
                if !properties.cold {
                    rules.push(rule(RuleTest::RandomBlock(b.netherrack.0, 0.07), magma));
                }
                settings
                    .processors
                    .push(Cow::Owned(if properties.air_pocket {
                        known.ignore_structure_block()
                    } else {
                        known.ignore_structure_and_air()
                    }));
                settings.processors.push(Cow::Owned(Processor::Rule(rules)));
                settings.processors.push(Cow::Owned(Processor::BlockAge {
                    mossiness: properties.mossiness(),
                }));
                settings
                    .processors
                    .push(Cow::Owned(Processor::ProtectedBlocks(
                        builtins.cannot_replace.clone(),
                    )));
                settings
                    .processors
                    .push(Cow::Owned(Processor::LavaSubmergedBlock));
                if properties.replace_with_blackstone {
                    settings
                        .processors
                        .push(Cow::Owned(Processor::BlackstoneReplace));
                }
            }
            PieceKind::EndCity { overwrite } => {
                settings.ignore_entities = true;
                settings.processors.push(Cow::Owned(if overwrite {
                    known.ignore_structure_block()
                } else {
                    known.ignore_structure_and_air()
                }));
            }
            PieceKind::Mansion => {
                settings.ignore_entities = true;
                settings
                    .processors
                    .push(Cow::Owned(known.ignore_structure_block()));
            }
        }
        settings
    }

    /// `StructurePiece.createTag` with the piece's own data.
    pub fn create_tag(&self) -> Compound {
        let (min, max) = (self.bounding_box.minimum(), self.bounding_box.maximum());
        let mut tag = Compound::from([
            ("id".into(), Tag::String(self.type_id().into())),
            (
                "BB".into(),
                Tag::IntArray(vec![min[0], min[1], min[2], max[0], max[1], max[2]]),
            ),
            // `setOrientation(Direction.NORTH)`: `get2DDataValue`.
            ("O".into(), Tag::Int(2)),
            ("GD".into(), Tag::Int(self.gen_depth)),
            ("TPX".into(), Tag::Int(self.position.x)),
            ("TPY".into(), Tag::Int(self.position.y)),
            ("TPZ".into(), Tag::Int(self.position.z)),
            (
                "Template".into(),
                Tag::String(self.template.as_str().into()),
            ),
        ]);
        let mut put = |key: &str, value: Tag| {
            tag.insert(key.into(), value);
        };
        let rot = Tag::String(rotation_name(self.rotation).into());
        match self.kind {
            PieceKind::Shipwreck {
                beached,
                height_adjusted,
            } => {
                put("isBeached", bool_tag(beached));
                put("Rot", rot);
                put("height_adjusted", bool_tag(height_adjusted));
            }
            PieceKind::OceanRuin {
                cold,
                integrity_bits,
                large,
            } => {
                put("Rot", rot);
                put("Integrity", Tag::Float(integrity_bits));
                put(
                    "BiomeType",
                    Tag::String(if cold { "COLD" } else { "WARM" }.into()),
                );
                put("IsLarge", bool_tag(large));
            }
            PieceKind::Igloo | PieceKind::NetherFossil => put("Rot", rot),
            PieceKind::RuinedPortal {
                placement,
                properties,
            } => {
                put("Rotation", rot);
                put("Mirror", Tag::String(mirror_name(self.mirror).into()));
                put("VerticalPlacement", Tag::String(placement.name().into()));
                put(
                    "Properties",
                    Tag::compound([
                        ("cold", bool_tag(properties.cold)),
                        ("mossiness", Tag::Float(properties.mossiness_bits)),
                        ("air_pocket", bool_tag(properties.air_pocket)),
                        ("overgrown", bool_tag(properties.overgrown)),
                        ("vines", bool_tag(properties.vines)),
                        (
                            "replace_with_blackstone",
                            bool_tag(properties.replace_with_blackstone),
                        ),
                    ]),
                );
            }
            PieceKind::EndCity { overwrite } => {
                put("Rot", rot);
                put("OW", bool_tag(overwrite));
            }
            PieceKind::Mansion => {
                put("Rot", rot);
                put("Mi", Tag::String(mirror_name(self.mirror).into()));
            }
        }
        tag
    }

    /// The loading constructors (`new XPiece(manager, tag)`): `Err` for what makes vanilla throw
    /// (a missing rotation, an unparsable template name...); the piece is then dropped.
    pub fn load(
        registry: &StructureRegistry,
        type_id: &str,
        tag: &Compound,
    ) -> Result<Self, String> {
        let get = |key: &str| tag.get(&JavaString::from(key));
        let int = |key: &str| get(key).and_then(Tag::int_value).unwrap_or(0);
        let text = |key: &str| match get(key) {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let boolean = |key: &str| match get(key) {
            Some(Tag::Byte(b)) => *b != 0,
            Some(other) => other.int_value().is_some_and(|v| v != 0),
            None => false,
        };
        let rotation = |key: &str| {
            text(key)
                .and_then(|name| rotation_by_name(&name))
                .ok_or_else(|| format!("No {key}"))
        };
        let template = text("Template").unwrap_or_default();
        let position = BlockPos::new(int("TPX"), int("TPY"), int("TPZ"));
        let gen_depth = int("GD");
        // `Identifier.parse(templateName)` for the pieces that use it as the location.
        let location_ok = |name: &str| parse_identifier(name).map(|_| ());
        let (kind, rotation_value, mirror, pivot) = match type_id {
            SHIPWRECK_ID => {
                location_ok(&template)?;
                (
                    PieceKind::Shipwreck {
                        beached: boolean("isBeached"),
                        height_adjusted: boolean("height_adjusted"),
                    },
                    rotation("Rot")?,
                    Mirror::None,
                    SHIPWRECK_PIVOT,
                )
            }
            IGLOO_ID => {
                location_ok(&template)?;
                let pivot = igloo_pivot(&template).ok_or("unknown igloo template")?;
                (PieceKind::Igloo, rotation("Rot")?, Mirror::None, pivot)
            }
            NETHER_FOSSIL_ID => {
                location_ok(&template)?;
                (
                    PieceKind::NetherFossil,
                    rotation("Rot")?,
                    Mirror::None,
                    BlockPos::default(),
                )
            }
            OCEAN_RUIN_ID => {
                location_ok(&template)?;
                let cold = match text("BiomeType").as_deref() {
                    Some("COLD") => true,
                    Some("WARM") => false,
                    _ => return Err("No BiomeType".into()),
                };
                let integrity_bits = match get("Integrity") {
                    Some(Tag::Float(bits)) => *bits,
                    _ => 0.0f32.to_bits(),
                };
                (
                    PieceKind::OceanRuin {
                        cold,
                        integrity_bits,
                        large: boolean("IsLarge"),
                    },
                    rotation("Rot")?,
                    Mirror::None,
                    BlockPos::default(),
                )
            }
            RUINED_PORTAL_ID => {
                location_ok(&template)?;
                let mirror = text("Mirror")
                    .and_then(|name| mirror_by_name(&name))
                    .ok_or("No Mirror")?;
                let placement = text("VerticalPlacement")
                    .and_then(|name| VerticalPlacement::from_name(&name))
                    .ok_or("No VerticalPlacement")?;
                let Some(Tag::Compound(props)) = get("Properties") else {
                    return Err("No Properties".into());
                };
                let flag = |key: &str| match props.get(&JavaString::from(key)) {
                    Some(Tag::Byte(b)) => Ok(*b != 0),
                    _ => Err(format!("No {key}")),
                };
                let mossiness_bits = match props.get(&JavaString::from("mossiness")) {
                    Some(Tag::Float(bits)) => *bits,
                    _ => return Err("No mossiness".into()),
                };
                let properties = PortalProperties {
                    cold: flag("cold")?,
                    mossiness_bits,
                    air_pocket: flag("air_pocket")?,
                    overgrown: flag("overgrown")?,
                    vines: flag("vines")?,
                    replace_with_blackstone: flag("replace_with_blackstone")?,
                };
                let size = template_of(&registry.templates, &template).size();
                (
                    PieceKind::RuinedPortal {
                        placement,
                        properties,
                    },
                    rotation("Rotation")?,
                    mirror,
                    BlockPos::new(size[0] / 2, 0, size[2] / 2),
                )
            }
            END_CITY_ID => (
                PieceKind::EndCity {
                    overwrite: boolean("OW"),
                },
                rotation("Rot")?,
                Mirror::None,
                BlockPos::default(),
            ),
            MANSION_ID => (
                PieceKind::Mansion,
                rotation("Rot")?,
                text("Mi")
                    .and_then(|name| mirror_by_name(&name))
                    .ok_or("No Mi")?,
                BlockPos::default(),
            ),
            other => return Err(format!("not a template piece: {other}")),
        };
        let mut piece = Self::new(
            registry,
            kind,
            template,
            position,
            rotation_value,
            mirror,
            pivot,
        );
        piece.gen_depth = gen_depth;
        Ok(piece)
    }

    /// `TemplateStructurePiece.postProcess` and each subclass's override.
    pub fn post_process<W: FeatureWorld, R: RandomSource>(
        &mut self,
        cx: &mut PieceCx<'_, '_, W, R>,
    ) -> Result<(), ProviderError> {
        match self.kind {
            PieceKind::Shipwreck { .. } => super::shipwreck::post_process(self, cx),
            PieceKind::OceanRuin { .. } => super::ocean_ruin::post_process(self, cx),
            PieceKind::Igloo => super::igloo::post_process(self, cx),
            PieceKind::RuinedPortal { .. } => super::ruined_portal::post_process(self, cx),
            PieceKind::NetherFossil => super::nether_fossil::post_process(self, cx),
            PieceKind::EndCity { .. } | PieceKind::Mansion => self.post_process_base(cx),
        }
    }

    /// `TemplateStructurePiece.postProcess`.
    pub fn post_process_base<W: FeatureWorld, R: RandomSource>(
        &mut self,
        cx: &mut PieceCx<'_, '_, W, R>,
    ) -> Result<(), ProviderError> {
        let tables = cx.tables;
        let lists = tables.processors;
        let mut settings = self.settings(lists);
        settings.bounding_box = Some(*cx.chunk_box);
        settings.known_shape = tables.known_shape;
        self.bounding_box = self.template_box(tables.structures);
        let template = self.template(tables.structures);
        let placed = place_in_world(
            &mut *cx.world,
            PlaceContext {
                known: lists.known(),
                seed: tables.seed,
            },
            template,
            self.position,
            cx.reference,
            &settings,
            &mut *cx.random,
            UPDATE_CLIENTS,
        )
        .map_err(provider_error)?;
        let Some(placed) = placed else {
            return Ok(());
        };
        cx.out.events.push(PieceEvent::Template(placed));
        let blocks = &lists.pieces().blocks;
        let markers = filter_blocks(
            template,
            cx.world.state_data(),
            cx.world.blocks(),
            self.position,
            &settings,
            blocks.structure_block,
            &mut *cx.random,
        )
        .map_err(provider_error)?;
        for marker in markers {
            let Some(nbt) = marker.nbt else { continue };
            let text = |key: &str| match nbt.get(&JavaString::from(key)) {
                Some(Tag::String(s)) => s.to_string_lossy(),
                _ => String::new(),
            };
            // `StructureMode.LEGACY_CODEC`'s "DATA".
            if text("mode") == "DATA" {
                self.handle_data_marker(&text("metadata"), marker.pos, cx)?;
            }
        }
        // Jigsaw blocks become their `final_state`.
        let jigsaws = filter_blocks(
            template,
            cx.world.state_data(),
            cx.world.blocks(),
            self.position,
            &settings,
            cx.world
                .blocks()
                .block_by_name("minecraft:jigsaw")
                .unwrap_or(BlockId(u32::MAX)),
            &mut *cx.random,
        )
        .map_err(provider_error)?;
        for jigsaw in jigsaws {
            let Some(nbt) = jigsaw.nbt else { continue };
            let text = match nbt.get(&JavaString::from("final_state")) {
                Some(Tag::String(s)) => s.to_string_lossy(),
                _ => "minecraft:air".to_owned(),
            };
            let state = parse_block_state(cx.world.blocks(), &text)
                .unwrap_or_else(|| cx.world.blocks().default_state(blocks.air));
            cx.world.set_block(jigsaw.pos, state, UPDATE_ALL)?;
        }
        Ok(())
    }

    /// `handleDataMarker`.
    fn handle_data_marker<W: FeatureWorld, R: RandomSource>(
        &mut self,
        marker: &str,
        pos: BlockPos,
        cx: &mut PieceCx<'_, '_, W, R>,
    ) -> Result<(), ProviderError> {
        match self.kind {
            PieceKind::Shipwreck { .. } => super::shipwreck::data_marker(marker, pos, cx),
            PieceKind::OceanRuin { large, .. } => {
                super::ocean_ruin::data_marker(large, marker, pos, cx)
            }
            PieceKind::Igloo => super::igloo::data_marker(marker, pos, cx),
            PieceKind::EndCity { .. } => super::end_city::data_marker(self, marker, pos, cx),
            PieceKind::Mansion => super::mansion::data_marker(self, marker, pos, cx),
            PieceKind::RuinedPortal { .. } | PieceKind::NetherFossil => Ok(()),
        }
    }
}

/// `BlockStateParser.parseForBlock(lookup, "name[property=value,...]", true)`: the state, `None`
/// where vanilla's parse fails.
fn parse_block_state(blocks: &BlockStates, text: &str) -> Option<crate::block::BlockState> {
    let (name, properties) = match text.split_once('[') {
        Some((name, rest)) => (name, Some(rest.strip_suffix(']')?)),
        None => (text, None),
    };
    let name = parse_identifier(name).ok()?;
    let mut state = blocks.default_state(blocks.block_by_name(&name)?);
    for property in properties.into_iter().flat_map(|p| p.split(',')) {
        let (key, value) = property.split_once('=')?;
        state = blocks.with_property(state, key.trim(), value.trim())?;
    }
    Some(state)
}

impl<W: FeatureWorld, R: RandomSource> PieceCx<'_, '_, W, R> {
    /// `setCraftLootTable(level, pos, random, table)` / `RandomizableContainer.setBlockEntityLootTable`:
    /// where a randomizable container is, it gets the table and `random.nextLong()`.
    pub fn set_loot_table(
        &mut self,
        pos: BlockPos,
        table: &'static str,
    ) -> Result<(), ProviderError> {
        let state = self.world.block_state(pos)?;
        if self
            .world
            .state_data()
            .has(state, FLAG_RANDOMIZABLE_CONTAINER)
        {
            let seed = self.random.next_long();
            self.out.events.push(PieceEvent::Loot { pos, table, seed });
        }
        Ok(())
    }

    /// `StructurePiece.createChest(level, chunkBB, random, pos, table, state)`.
    pub fn create_chest(
        &mut self,
        pos: BlockPos,
        table: &'static str,
        state: crate::block::BlockState,
    ) -> Result<bool, ProviderError> {
        let chest = self.tables.processors.pieces().blocks.chest;
        let there = self.world.block_state(pos)?;
        if self.chunk_box.contains([pos.x, pos.y, pos.z])
            && self.world.blocks().block(there) != chest
        {
            let seed = self.random.next_long();
            self.world.set_block(pos, state, UPDATE_CLIENTS)?;
            self.out.events.push(PieceEvent::Loot { pos, table, seed });
            return Ok(true);
        }
        Ok(false)
    }

    pub fn is_block(&self, pos: BlockPos, block: BlockId) -> Result<bool, ProviderError> {
        let state = self.world.block_state(pos)?;
        Ok(self.world.blocks().block(state) == block)
    }

    /// `level.getHeight(type, x, z)`.
    pub fn height(
        &self,
        kind: crate::world::chunk::serial::Heightmap,
        x: i32,
        z: i32,
    ) -> Result<i32, ProviderError> {
        Ok(self.world.height(kind, x, z)?)
    }

    pub fn blocks(&self) -> &PieceBlocks {
        &self.tables.processors.pieces().blocks
    }
}
