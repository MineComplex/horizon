//! Placement modifiers (`levelgen.placement`): how a placed feature turns one origin into the
//! positions its feature is placed at, and the [`FeaturePlacer`] that runs the chain.
//!
//! - [`provider`]: the int and height providers the modifiers sample.
//! - [`predicate`]: block predicates.
//! - [`world`]: the narrow view of the world the modifiers read.
//! - [`chain`]: the `FeaturePlacer` loop; [`layer`]: `count_on_every_layer`.
//!
//! Copied from Horizon's `horizon-world::worldgen::placement`, without its work budgets (vanilla
//! has none; the registries are the server owner's) and with the registries decoded eagerly.

use serde_json::value::RawValue;

use crate::block::BlockStates;
use crate::core::position::BlockPos;
use crate::util::random::{InvalidBound, RandomSource};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{
    DecodeError, boolean, double, float, int_range, integer, object, optional, required, string,
};
use crate::worldgen::feature::{FeatureRegistry, PlacedFeatureId};
use crate::worldgen::structure::BiomeSet;

mod chain;
mod layer;
pub mod predicate;
pub mod provider;
pub mod survival;
pub mod world;

#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod tests;

pub use chain::{FeaturePlacer, PlaceError};
pub use predicate::BlockPredicate;
pub use provider::{HeightProvider, IntProvider};
pub use world::{PlacementWorld, RegionWorld, WorldError};

use provider::type_of;

/// The fluid registry in its registration order, which is `StateFluid::fluid`.
pub const FLUIDS: [&str; 5] = [
    "minecraft:empty",
    "minecraft:flowing_water",
    "minecraft:water",
    "minecraft:flowing_lava",
    "minecraft:lava",
];

/// The registries a predicate's holder sets and block states resolve against when decoded.
pub struct DecodeContext<'a> {
    pub blocks: &'a BlockStates,
    pub block_tags: &'a [(String, Vec<i32>)],
    /// `worldgen/biome` ids in registration order, and its tags.
    pub biomes: &'a [String],
    pub biome_tags: &'a [(String, Vec<i32>)],
    pub fluid_tags: &'a [(String, Vec<i32>)],
}

/// `RegistryCodecs.homogeneousList`: `"#tag"`, an id, or a list of ids, as registry ids.
pub(crate) fn holder_set(
    raw: &RawValue,
    registry: &str,
    id: impl Fn(&str) -> Option<u32>,
    tags: &[(String, Vec<i32>)],
) -> Result<BiomeSet, DecodeError> {
    let by_name = |name: &str| {
        let name = crate::worldgen::codec::identifier(name)?;
        id(&name).ok_or_else(|| DecodeError::Registry(format!("Missing {registry}: {name}")))
    };
    if raw.get().starts_with('[') {
        let names: Vec<String> = serde_json::from_str(raw.get())?;
        return names
            .iter()
            .map(|name| by_name(name))
            .collect::<Result<_, _>>()
            .map(BiomeSet::new);
    }
    let name = string(raw)?;
    match name.strip_prefix('#') {
        Some(tag) => {
            let tag = crate::worldgen::codec::identifier(tag)?;
            let (_, ids) = tags.iter().find(|(t, _)| *t == tag).ok_or_else(|| {
                DecodeError::Registry(format!("Missing tag: #{tag} in {registry}"))
            })?;
            Ok(BiomeSet::new(ids.iter().map(|&i| i as u32).collect()))
        }
        None => Ok(BiomeSet::new(vec![by_name(&name)?])),
    }
}

impl DecodeContext<'_> {
    pub fn block_set(&self, raw: &RawValue) -> Result<BiomeSet, DecodeError> {
        let id = |name: &str| self.blocks.block_by_name(name).map(|b| b.0);
        holder_set(raw, "block", id, self.block_tags)
    }

    /// `TagKey.codec(BLOCK)`: a tag no pack defines is empty.
    pub fn block_tag(&self, name: &str) -> Result<BiomeSet, DecodeError> {
        let tag = crate::worldgen::codec::identifier(name.strip_prefix('#').unwrap_or(name))?;
        Ok(BiomeSet::new(
            (self.block_tags.iter())
                .find(|(t, _)| *t == tag)
                .map(|(_, ids)| ids.iter().map(|&i| i as u32).collect())
                .unwrap_or_default(),
        ))
    }

    pub fn biome_set(&self, raw: &RawValue) -> Result<BiomeSet, DecodeError> {
        let id = |name: &str| self.biomes.iter().position(|b| b == name).map(|i| i as u32);
        holder_set(raw, "biome", id, self.biome_tags)
    }

    pub fn fluid_set(&self, raw: &RawValue) -> Result<BiomeSet, DecodeError> {
        let id = |name: &str| FLUIDS.iter().position(|f| *f == name).map(|i| i as u32);
        holder_set(raw, "fluid", id, self.fluid_tags)
    }

    /// `BlockState.CODEC` (26.3's `StateHolder` codec): `{"id": ..., "properties": {...}}`, or a
    /// block's id for its default state.
    pub fn block_state(&self, raw: &RawValue) -> Result<crate::block::BlockState, DecodeError> {
        let (name, properties) = if raw.get().starts_with('"') {
            (string(raw)?, Default::default())
        } else {
            let fields = object(raw)?;
            let properties: std::collections::BTreeMap<String, String> =
                optional(&fields, "properties").map_or(Ok(Default::default()), |raw| {
                    serde_json::from_str(raw.get())
                })?;
            (string(required(&fields, "id")?)?, properties)
        };
        let name = crate::worldgen::codec::identifier(&name)?;
        let block = (self.blocks.block_by_name(&name))
            .ok_or_else(|| DecodeError::Registry(format!("Missing block: {name}")))?;
        self.blocks
            .state_with(
                block,
                properties.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            )
            .ok_or(DecodeError::Field("block state property"))
    }
}

/// A modifier failing: vanilla throws, which stops the generation of the chunk.
#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
    #[error(transparent)]
    Random(#[from] InvalidBound),
    #[error(transparent)]
    World(#[from] WorldError),
    #[error(
        "Tried to biome check an unregistered feature, or a feature that should not restrict the biome"
    )]
    NoTopFeature,
}

/// `PlacementContext`: the world, the generation bounds and the placed feature at the top of the
/// chain (the biome filter's, when the chain is run with a biome check).
pub struct PlacementContext<'a, W> {
    pub world: &'a W,
    pub features: &'a FeatureRegistry,
    pub top: Option<PlacedFeatureId>,
}

impl<W> Clone for PlacementContext<'_, W> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<W> Copy for PlacementContext<'_, W> {}

#[derive(Clone, Debug)]
pub enum PlacementModifier {
    Biome,
    BlockPredicateFilter(BlockPredicate),
    Rarity(i32),
    RandomChance(f32),
    SurfaceRelativeThreshold {
        heightmap: Heightmap,
        min: i32,
        max: i32,
    },
    SurfaceWaterDepth(i32),
    Count(IntProvider),
    NoiseBasedCount {
        ratio: i32,
        factor: f64,
        offset: f64,
    },
    NoiseThresholdCount {
        level: f64,
        below: i32,
        above: i32,
    },
    CountOnEveryLayer {
        count: IntProvider,
        special: layer::SpecialBlocks,
    },
    Cuboid {
        xz_size: IntProvider,
        y_size: IntProvider,
        include_edges: bool,
        include_interior: bool,
    },
    /// `dy` is 1 for `up`, -1 for `down`.
    EnvironmentScan {
        dy: i32,
        target: BlockPredicate,
        allowed: BlockPredicate,
        max_steps: i32,
    },
    Heightmap(Heightmap),
    HeightRange(HeightProvider),
    InSquare,
    Offset([IntProvider; 3]),
    RandomlySelected(Vec<Self>),
    Fixed(Vec<BlockPos>),
}

fn heightmap(name: &str) -> Result<Heightmap, DecodeError> {
    (Heightmap::ALL.into_iter())
        .find(|h| h.key() == name)
        .ok_or(DecodeError::Field("heightmap"))
}

impl PlacementModifier {
    /// `PlacementModifier.CODEC`.
    pub fn decode(raw: &RawValue, names: &DecodeContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let provider =
            |name, min, max| IntProvider::decode_within(required(&fields, name)?, min, max);
        let predicate = |name| BlockPredicate::decode(required(&fields, name)?, names);
        let heightmap_field = || heightmap(&string(required(&fields, "heightmap")?)?);
        Ok(match type_of(&fields)?.as_str() {
            "minecraft:biome" => Self::Biome,
            "minecraft:block_predicate_filter" => {
                Self::BlockPredicateFilter(predicate("predicate")?)
            }
            "minecraft:rarity_filter" => Self::Rarity(int_range(
                required(&fields, "chance")?,
                "chance",
                1,
                i32::MAX,
            )?),
            "minecraft:random_chance" => {
                let chance = float(required(&fields, "chance")?)?;
                if !(0.0..=1.0).contains(&chance) {
                    return Err(DecodeError::Field("chance"));
                }
                Self::RandomChance(chance)
            }
            "minecraft:surface_relative_threshold_filter" => Self::SurfaceRelativeThreshold {
                heightmap: heightmap_field()?,
                min: optional(&fields, "min_inclusive").map_or(Ok(i32::MIN), integer)?,
                max: optional(&fields, "max_inclusive").map_or(Ok(i32::MAX), integer)?,
            },
            "minecraft:surface_water_depth_filter" => {
                Self::SurfaceWaterDepth(integer(required(&fields, "max_water_depth")?)?)
            }
            "minecraft:count" => Self::Count(provider("count", 0, 4096)?),
            "minecraft:noise_based_count" => Self::NoiseBasedCount {
                ratio: integer(required(&fields, "noise_to_count_ratio")?)?,
                factor: double(required(&fields, "noise_factor")?)?,
                offset: optional(&fields, "noise_offset").map_or(Ok(0.0), double)?,
            },
            "minecraft:noise_threshold_count" => Self::NoiseThresholdCount {
                level: double(required(&fields, "noise_level")?)?,
                below: integer(required(&fields, "below_noise")?)?,
                above: integer(required(&fields, "above_noise")?)?,
            },
            "minecraft:count_on_every_layer" => Self::CountOnEveryLayer {
                count: provider("count", 0, 256)?,
                special: layer::SpecialBlocks::resolve(names.blocks)?,
            },
            "minecraft:cuboid" => Self::Cuboid {
                xz_size: provider("xz_size", 1, 16)?,
                y_size: provider("y_size", 1, 16)?,
                include_edges: optional(&fields, "include_edges").map_or(Ok(true), boolean)?,
                include_interior: optional(&fields, "include_interior")
                    .map_or(Ok(true), boolean)?,
            },
            "minecraft:environment_scan" => Self::EnvironmentScan {
                dy: match string(required(&fields, "direction_of_search")?)?.as_str() {
                    "up" => 1,
                    "down" => -1,
                    _ => return Err(DecodeError::Field("direction_of_search")),
                },
                target: predicate("target_condition")?,
                allowed: optional(&fields, "allowed_search_condition")
                    .map_or(Ok(BlockPredicate::True), |raw| {
                        BlockPredicate::decode(raw, names)
                    })?,
                max_steps: int_range(required(&fields, "max_steps")?, "max_steps", 1, 32)?,
            },
            "minecraft:heightmap" => Self::Heightmap(heightmap_field()?),
            "minecraft:height_range" => {
                Self::HeightRange(HeightProvider::decode(required(&fields, "height")?)?)
            }
            "minecraft:in_square" => Self::InSquare,
            "minecraft:offset" => Self::Offset([
                provider("x", -16, 16)?,
                provider("y", -16, 16)?,
                provider("z", -16, 16)?,
            ]),
            "minecraft:randomly_selected" => {
                let items: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "placements")?.get())?;
                if items.is_empty() {
                    return Err(DecodeError::Field("placements"));
                }
                Self::RandomlySelected(
                    (items.iter())
                        .map(|item| Self::decode(item, names))
                        .collect::<Result<_, _>>()?,
                )
            }
            "minecraft:fixed_placement" => {
                let items: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "positions")?.get())?;
                Self::Fixed(
                    (items.iter())
                        .map(|item| {
                            let [x, y, z] = vector(item)?;
                            Ok(BlockPos::new(x, y, z))
                        })
                        .collect::<Result<_, DecodeError>>()?,
                )
            }
            _ => return Err(DecodeError::Field("placement modifier type")),
        })
    }

    /// `PlacementModifier.getPositions` through its `Consumer`: appends the positions this
    /// modifier makes of `origin` to `out`, drawing from `random` as vanilla does.
    pub fn modify<W: PlacementWorld>(
        &self,
        cx: &PlacementContext<'_, W>,
        random: &mut impl RandomSource,
        origin: BlockPos,
        out: &mut Vec<BlockPos>,
    ) -> Result<(), PlacementError> {
        let world = cx.world;
        match self {
            Self::Biome => {
                let top = cx.top.ok_or(PlacementError::NoTopFeature)?;
                if cx.features.biome_has_feature(world.biome(origin)?, top) {
                    out.push(origin);
                }
            }
            Self::BlockPredicateFilter(predicate) => {
                if predicate.test(world, origin)? {
                    out.push(origin);
                }
            }
            Self::Rarity(chance) => {
                if random.next_float() < 1.0 / *chance as f32 {
                    out.push(origin);
                }
            }
            Self::RandomChance(chance) => {
                if random.next_float() < *chance {
                    out.push(origin);
                }
            }
            Self::SurfaceRelativeThreshold {
                heightmap,
                min,
                max,
            } => {
                let surface = i64::from(world.height(*heightmap, origin.x, origin.z)?);
                if surface + i64::from(*min) <= i64::from(origin.y)
                    && i64::from(origin.y) <= surface + i64::from(*max)
                {
                    out.push(origin);
                }
            }
            Self::SurfaceWaterDepth(max) => {
                let floor = world.height(Heightmap::OceanFloor, origin.x, origin.z)?;
                let surface = world.height(Heightmap::WorldSurface, origin.x, origin.z)?;
                if surface.wrapping_sub(floor) <= *max {
                    out.push(origin);
                }
            }
            Self::Count(_) | Self::NoiseBasedCount { .. } | Self::NoiseThresholdCount { .. } => {
                for _ in 0..self.repetitions(cx, random, origin)? {
                    out.push(origin);
                }
            }
            Self::CountOnEveryLayer { count, special } => {
                layer::modify(count, special, world, random, origin, out)?;
            }
            Self::Cuboid {
                xz_size,
                y_size,
                include_edges,
                include_interior,
            } => {
                let height = y_size.sample(random)?;
                let width = xz_size.sample(random)?;
                let length = xz_size.sample(random)?;
                for x in 0..=width {
                    for y in 0..=height {
                        for z in 0..=length {
                            let (bx, by, bz) = (
                                x == 0 || x == width,
                                y == 0 || y == height,
                                z == 0 || z == length,
                            );
                            if (*include_edges || u8::from(bx) + u8::from(by) + u8::from(bz) < 2)
                                && (*include_interior || bx || by || bz)
                            {
                                out.push(origin.offset(x, y, z));
                            }
                        }
                    }
                }
            }
            Self::EnvironmentScan {
                dy,
                target,
                allowed,
                max_steps,
            } => {
                let mut pos = origin;
                if allowed.test(world, pos)? {
                    for _ in 0..*max_steps {
                        if target.test(world, pos)? {
                            out.push(pos);
                            return Ok(());
                        }
                        pos = pos.offset(0, *dy, 0);
                        if world.outside_build_height(pos.y) {
                            return Ok(());
                        }
                        if !allowed.test(world, pos)? {
                            break;
                        }
                    }
                    if target.test(world, pos)? {
                        out.push(pos);
                    }
                }
            }
            Self::Heightmap(kind) => {
                let height = world.height(*kind, origin.x, origin.z)?;
                if height > world.min_y() {
                    out.push(BlockPos::new(origin.x, height, origin.z));
                }
            }
            Self::HeightRange(height) => {
                let y = height.sample(random, world.generation())?;
                out.push(BlockPos::new(origin.x, y, origin.z));
            }
            Self::InSquare => {
                let x = random.next_int_bound(16)?.wrapping_add(origin.x);
                let z = random.next_int_bound(16)?.wrapping_add(origin.z);
                out.push(BlockPos::new(x, origin.y, z));
            }
            Self::Offset([x, y, z]) => {
                let (dx, dy, dz) = (x.sample(random)?, y.sample(random)?, z.sample(random)?);
                out.push(origin.offset(dx, dy, dz));
            }
            Self::RandomlySelected(placements) => {
                let index = random.next_int_bound(placements.len() as i32)?;
                placements[index as usize].modify(cx, random, origin, out)?;
            }
            Self::Fixed(positions) => {
                out.extend(
                    (positions.iter())
                        .filter(|p| p.x >> 4 == origin.x >> 4 && p.z >> 4 == origin.z >> 4),
                );
            }
        }
        Ok(())
    }

    /// `RepeatingPlacement.count`: how many times the origin repeats.
    fn repetitions<W: PlacementWorld>(
        &self,
        cx: &PlacementContext<'_, W>,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<i32, InvalidBound> {
        let noise = |x: f64, z: f64| f64::from(cx.features.info_noise().get2(x, z));
        Ok(match self {
            Self::Count(count) => count.sample(random)?,
            Self::NoiseBasedCount {
                ratio,
                factor,
                offset,
            } => {
                let flower = noise(f64::from(origin.x) / factor, f64::from(origin.z) / factor);
                ((flower + offset) * f64::from(*ratio)).ceil() as i32
            }
            Self::NoiseThresholdCount {
                level,
                below,
                above,
            } => {
                let flower = noise(f64::from(origin.x) / 200.0, f64::from(origin.z) / 200.0);
                if flower < *level { *below } else { *above }
            }
            _ => unreachable!("not a repeating placement"),
        })
    }
}

/// `BlockPos.CODEC`: three ints.
fn vector(raw: &RawValue) -> Result<[i32; 3], DecodeError> {
    let values: Vec<Box<RawValue>> = serde_json::from_str(raw.get())?;
    match values.as_slice() {
        [x, y, z] => Ok([integer(x)?, integer(y)?, integer(z)?]),
        _ => Err(DecodeError::Field("block position")),
    }
}
