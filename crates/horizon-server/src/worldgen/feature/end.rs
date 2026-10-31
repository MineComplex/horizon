//! The End's features and the small ones that go with them: `chorus_plant`, `end_spike`,
//! `end_island`, `end_platform`, `void_start_platform`, `end_gateway`, `end_podium`,
//! `bonus_chest` and `spike` (the ice spike).
//!
//! The crystal on a spike and the gateway's exit are entity and block entity data, which the proto
//! chunk keeps nowhere yet ([`FeatureWorld::add_end_crystal`]).

use serde_json::value::RawValue;

use super::common::{HORIZONTAL, default_state, face_name, floor, is_air, relative, shuffle};
use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::{FLAG_AIR, FLAG_RANDOMIZABLE_CONTAINER};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::jmath;
use crate::util::random::{LegacyRandom, RandomSource};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{DecodeError, boolean, integer, object, optional, required};
use crate::worldgen::placement::{BlockPredicate, PlacementWorld};
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum End {
    ChorusPlant(Box<ChorusPlant>),
    EndSpike(Box<EndSpike>),
    EndIsland(BlockState),
    EndPlatform {
        obsidian: BlockState,
        air: BlockState,
    },
    VoidStartPlatform {
        cobblestone: BlockState,
        stone: BlockState,
    },
    EndGateway(Box<Gateway>),
    EndPodium(Box<Podium>),
    BonusChest(Box<BonusChest>),
    Spike(Box<IceSpike>),
}

impl End {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        let names = ctx.names;
        let state = |name: &str| default_state(names.blocks, name);
        Ok(Some(match kind {
            "minecraft:chorus_plant" => Self::ChorusPlant(Box::new(ChorusPlant {
                plant: state("minecraft:chorus_plant")?,
                flower: state("minecraft:chorus_flower")?,
                supports_plant: names.block_tag("minecraft:supports_chorus_plant")?,
            })),
            "minecraft:end_spike" => {
                let spikes: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "spikes")?.get())?;
                let mut decoded = Vec::new();
                for spike in &spikes {
                    let s = object(spike)?;
                    let int = |name: &'static str| optional(&s, name).map_or(Ok(0), integer);
                    decoded.push(Spike {
                        center_x: int("centerX")?,
                        center_z: int("centerZ")?,
                        radius: int("radius")?,
                        height: int("height")?,
                        guarded: optional(&s, "guarded").map_or(Ok(false), boolean)?,
                    });
                }
                Self::EndSpike(Box::new(EndSpike {
                    spikes: decoded,
                    crystal_invulnerable: optional(&fields, "crystal_invulnerable")
                        .map_or(Ok(false), boolean)?,
                    crystal_beam_target: optional(&fields, "crystal_beam_target")
                        .map(block_pos)
                        .transpose()?,
                    obsidian: state("minecraft:obsidian")?,
                    air: state("minecraft:air")?,
                    bedrock: state("minecraft:bedrock")?,
                    iron_bars: state("minecraft:iron_bars")?,
                    fire: state("minecraft:fire")?,
                }))
            }
            "minecraft:end_island" => Self::EndIsland(state("minecraft:end_stone")?),
            "minecraft:end_platform" => Self::EndPlatform {
                obsidian: state("minecraft:obsidian")?,
                air: state("minecraft:air")?,
            },
            "minecraft:void_start_platform" => Self::VoidStartPlatform {
                cobblestone: state("minecraft:cobblestone")?,
                stone: state("minecraft:stone")?,
            },
            "minecraft:end_gateway" => Self::EndGateway(Box::new(Gateway {
                gateway: state("minecraft:end_gateway")?,
                bedrock: state("minecraft:bedrock")?,
                air: state("minecraft:air")?,
            })),
            "minecraft:end_podium" => Self::EndPodium(Box::new(Podium {
                active: optional(&fields, "active").map_or(Ok(false), boolean)?,
                bedrock: state("minecraft:bedrock")?,
                end_stone: state("minecraft:end_stone")?,
                air: state("minecraft:air")?,
                portal: state("minecraft:end_portal")?,
                wall_torch: state("minecraft:wall_torch")?,
            })),
            "minecraft:bonus_chest" => Self::BonusChest(Box::new(BonusChest {
                chest: state("minecraft:chest")?,
                torch: state("minecraft:torch")?,
            })),
            "minecraft:spike" => Self::Spike(Box::new(IceSpike {
                state: names.block_state(required(&fields, "state")?)?,
                can_place_on: BlockPredicate::decode(required(&fields, "can_place_on")?, names)?,
                can_replace: BlockPredicate::decode(required(&fields, "can_replace")?, names)?,
            })),
            _ => return Ok(None),
        }))
    }

    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::ChorusPlant(f) => f.place(world, random, origin),
            Self::EndSpike(f) => f.place(world, random, origin),
            Self::EndIsland(end_stone) => end_island(world, random, origin, *end_stone),
            Self::EndPlatform { obsidian, air } => {
                end_platform(world, origin, *obsidian, *air)?;
                Ok(true)
            }
            Self::VoidStartPlatform { cobblestone, stone } => {
                void_start_platform(world, origin, *cobblestone, *stone)
            }
            Self::EndGateway(f) => f.place(world, origin),
            Self::EndPodium(f) => f.place(world, origin),
            Self::BonusChest(f) => f.place(world, random, origin),
            Self::Spike(f) => f.place(world, random, origin),
        }
    }
}

/// `BlockPos.CODEC`: `[x, y, z]`.
fn block_pos(raw: &RawValue) -> Result<BlockPos, DecodeError> {
    let values: Vec<i32> = serde_json::from_str(raw.get())?;
    match values[..] {
        [x, y, z] => Ok(BlockPos::new(x, y, z)),
        _ => Err(DecodeError::Field("block position")),
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ChorusPlant {
    plant: BlockState,
    flower: BlockState,
    supports_plant: BiomeSet,
}

impl ChorusPlant {
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let below = world.block_state(origin.offset(0, -1, 0))?;
        if is_air(world, origin)? && self.supports_plant.contains(world.blocks().block(below).0) {
            self.generate_plant(world, random, origin, 8)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `ChorusPlantBlock.getStateWithConnections`.
    fn with_connections(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<BlockState, ProviderError> {
        let blocks = world.blocks();
        let plant = blocks.block(self.plant);
        let flower = blocks.block(self.flower);
        let at = |direction: Direction| -> Result<_, ProviderError> {
            Ok(blocks.block(world.block_state(relative(pos, direction))?))
        };
        let is_plant_or_flower = |block| block == plant || block == flower;
        let down = at(Direction::Down)?;
        let mut state = self.plant;
        let mut set = |direction: Direction, value: bool| {
            if let Some(next) = blocks.with_property(
                state,
                face_name(direction),
                if value { "true" } else { "false" },
            ) {
                state = next;
            }
        };
        set(
            Direction::Down,
            down == plant || down == flower || self.supports_plant.contains(down.0),
        );
        set(Direction::Up, is_plant_or_flower(at(Direction::Up)?));
        set(Direction::North, is_plant_or_flower(at(Direction::North)?));
        set(Direction::East, is_plant_or_flower(at(Direction::East)?));
        set(Direction::South, is_plant_or_flower(at(Direction::South)?));
        set(Direction::West, is_plant_or_flower(at(Direction::West)?));
        Ok(state)
    }

    fn all_neighbours_empty(
        world: &impl PlacementWorld,
        pos: BlockPos,
        ignore: Option<Direction>,
    ) -> Result<bool, ProviderError> {
        for direction in HORIZONTAL {
            if Some(direction) != ignore && !is_air(world, relative(pos, direction))? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `ChorusFlowerBlock.generatePlant`.
    fn generate_plant<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        target: BlockPos,
        max_spread: i32,
    ) -> Result<(), ProviderError> {
        let state = self.with_connections(world, target)?;
        world.set_block(target, state, 2)?;
        self.grow_tree(world, random, target, target, max_spread, 0)
    }

    fn set_plant<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        let state = self.with_connections(world, pos)?;
        world.set_block(pos, state, 2)?;
        Ok(())
    }

    fn grow_tree<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        current: BlockPos,
        start: BlockPos,
        max_spread: i32,
        depth: i32,
    ) -> Result<(), ProviderError> {
        let mut height = random.next_int_bound(4)? + 1;
        if depth == 0 {
            height += 1;
        }
        for i in 0..height {
            let target = current.offset(0, i + 1, 0);
            if !Self::all_neighbours_empty(world, target, None)? {
                return Ok(());
            }
            self.set_plant(world, target)?;
            self.set_plant(world, target.offset(0, -1, 0))?;
        }
        let mut placed_stem = false;
        if depth < 4 {
            let mut stems = random.next_int_bound(4)?;
            if depth == 0 {
                stems += 1;
            }
            for _ in 0..stems {
                // `Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
                let direction = HORIZONTAL[random.next_int_bound(4)? as usize];
                let target = relative(current.offset(0, height, 0), direction);
                if (target.x - start.x).abs() < max_spread
                    && (target.z - start.z).abs() < max_spread
                    && is_air(world, target)?
                    && is_air(world, target.offset(0, -1, 0))?
                    && Self::all_neighbours_empty(world, target, Some(direction.opposite()))?
                {
                    placed_stem = true;
                    self.set_plant(world, target)?;
                    self.set_plant(world, relative(target, direction.opposite()))?;
                    self.grow_tree(world, random, target, start, max_spread, depth + 1)?;
                }
            }
        }
        if !placed_stem {
            let flower = world
                .blocks()
                .with_property(self.flower, "age", "5")
                .ok_or(ProviderError::Invalid("chorus flower age"))?;
            world.set_block(current.offset(0, height, 0), flower, 2)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Spike {
    center_x: i32,
    center_z: i32,
    radius: i32,
    height: i32,
    guarded: bool,
}

#[derive(Clone, Debug)]
pub struct EndSpike {
    spikes: Vec<Spike>,
    crystal_invulnerable: bool,
    crystal_beam_target: Option<BlockPos>,
    obsidian: BlockState,
    air: BlockState,
    bedrock: BlockState,
    iron_bars: BlockState,
    fire: BlockState,
}

impl EndSpike {
    /// `EndSpikeFeature.getSpikesForLevel`.
    fn spikes_for_level(seed: i64) -> Result<Vec<Spike>, ProviderError> {
        let key = LegacyRandom::new(seed).next_long() & 65535;
        let mut sizes: Vec<i32> = (0..10).collect();
        shuffle(&mut sizes, &mut LegacyRandom::new(key))?;
        let pi = std::f64::consts::PI;
        Ok((0..10)
            .map(|i| {
                let angle = 2.0 * (-pi + (pi / 10.0) * f64::from(i));
                let size = sizes[i as usize];
                Spike {
                    center_x: floor(42.0 * jmath::cos(angle)),
                    center_z: floor(42.0 * jmath::sin(angle)),
                    radius: 2 + size / 3,
                    height: 76 + size * 3,
                    guarded: size == 1 || size == 2,
                }
            })
            .collect())
    }

    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let spikes = if self.spikes.is_empty() {
            Self::spikes_for_level(world.world_seed()?)?
        } else {
            self.spikes.clone()
        };
        for spike in &spikes {
            if origin.x >> 4 == spike.center_x >> 4 && origin.z >> 4 == spike.center_z >> 4 {
                self.place_spike(world, random, spike)?;
            }
        }
        Ok(true)
    }

    fn place_spike<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        spike: &Spike,
    ) -> Result<(), ProviderError> {
        let radius = spike.radius;
        let min_y = world.min_y();
        // `BlockPos.betweenClosed`: x fastest, then y, then z.
        for z in spike.center_z - radius..=spike.center_z + radius {
            for y in min_y..=spike.height + 10 {
                for x in spike.center_x - radius..=spike.center_x + radius {
                    let pos = BlockPos::new(x, y, z);
                    // `distToLowCornerSqr(centerX, y, centerZ)`.
                    let (dx, dz) = (f64::from(x - spike.center_x), f64::from(z - spike.center_z));
                    if dx * dx + dz * dz <= f64::from(radius * radius + 1) && y < spike.height {
                        world.set_block(pos, self.obsidian, 3)?;
                    } else if y > 65 {
                        world.set_block(pos, self.air, 3)?;
                    }
                }
            }
        }
        if spike.guarded {
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    for dy in 0..=3 {
                        let x_side = dx.abs() == 2;
                        let z_side = dz.abs() == 2;
                        let top = dy == 3;
                        if x_side || z_side || top {
                            let x_edge = dx == -2 || dx == 2 || top;
                            let z_edge = dz == -2 || dz == 2 || top;
                            let blocks = world.blocks();
                            let mut state = self.iron_bars;
                            for (face, value) in [
                                ("north", x_edge && dz != -2),
                                ("south", x_edge && dz != 2),
                                ("west", z_edge && dx != -2),
                                ("east", z_edge && dx != 2),
                            ] {
                                state = blocks
                                    .with_property(
                                        state,
                                        face,
                                        if value { "true" } else { "false" },
                                    )
                                    .ok_or(ProviderError::Invalid("iron bars face"))?;
                            }
                            let pos = BlockPos::new(
                                spike.center_x + dx,
                                spike.height + dy,
                                spike.center_z + dz,
                            );
                            world.set_block(pos, state, 3)?;
                        }
                    }
                }
            }
        }
        // The crystal: `EntityTypes.END_CRYSTAL.create`, `snapTo` with a random yaw.
        let crystal = BlockPos::new(spike.center_x, spike.height + 1, spike.center_z);
        world.add_end_crystal(
            crystal,
            self.crystal_beam_target,
            self.crystal_invulnerable,
            random,
        )?;
        world.set_block(crystal.offset(0, -1, 0), self.bedrock, 3)?;
        world.set_block(crystal, self.fire, 3)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

/// `EndIslandFeature.place`.
fn end_island<W: FeatureWorld>(
    world: &mut W,
    random: &mut impl RandomSource,
    origin: BlockPos,
    end_stone: BlockState,
) -> Result<bool, ProviderError> {
    let mut size = random.next_int_bound(3)? as f32 + 4.0;
    let mut y = 0;
    while size > 0.5 {
        for x in floor(f64::from(-size))..=size.ceil() as i32 {
            for z in floor(f64::from(-size))..=size.ceil() as i32 {
                if (x * x + z * z) as f32 <= (size + 1.0) * (size + 1.0) {
                    world.set_block(origin.offset(x, y, z), end_stone, 3)?;
                }
            }
        }
        size -= random.next_int_bound(2)? as f32 + 0.5;
        y -= 1;
    }
    Ok(true)
}

/// `EndPlatformFeature.createEndPlatform(level, origin, false)`.
fn end_platform<W: FeatureWorld>(
    world: &mut W,
    origin: BlockPos,
    obsidian: BlockState,
    air: BlockState,
) -> Result<(), ProviderError> {
    for dz in -2..=2 {
        for dx in -2..=2 {
            for dy in -1..3 {
                let pos = origin.offset(dx, dy, dz);
                let block = if dy == -1 { obsidian } else { air };
                let existing = world.block_state(pos)?;
                if world.blocks().block(existing) != world.blocks().block(block) {
                    world.set_block(pos, block, 3)?;
                }
            }
        }
    }
    Ok(())
}

/// `VoidStartPlatformFeature.place`.
fn void_start_platform<W: FeatureWorld>(
    world: &mut W,
    origin: BlockPos,
    cobblestone: BlockState,
    stone: BlockState,
) -> Result<bool, ProviderError> {
    let (chunk_x, chunk_z) = (origin.x >> 4, origin.z >> 4);
    // `PLATFORM_OFFSET` (8, 3, 8) lies in chunk (0, 0).
    if chunk_x.abs().max(chunk_z.abs()) > 1 {
        return Ok(true);
    }
    let platform = BlockPos::new(8, origin.y + 3, 8);
    for z in chunk_z * 16..=chunk_z * 16 + 15 {
        for x in chunk_x * 16..=chunk_x * 16 + 15 {
            if (platform.x - x).abs().max((platform.z - z).abs()) <= 16 {
                let pos = BlockPos::new(x, platform.y, z);
                world.set_block(pos, if pos == platform { cobblestone } else { stone }, 2)?;
            }
        }
    }
    Ok(true)
}

#[derive(Clone, Debug)]
pub struct Gateway {
    gateway: BlockState,
    bedrock: BlockState,
    air: BlockState,
}

impl Gateway {
    /// `EndGatewayFeature.place`; the exit position is block entity data.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        for z in origin.z - 1..=origin.z + 1 {
            for y in origin.y - 2..=origin.y + 2 {
                for x in origin.x - 1..=origin.x + 1 {
                    let pos = BlockPos::new(x, y, z);
                    let same_x = x == origin.x;
                    let same_y = y == origin.y;
                    let same_z = z == origin.z;
                    let end = (y - origin.y).abs() == 2;
                    let state = if same_x && same_y && same_z {
                        self.gateway
                    } else if same_y {
                        self.air
                    } else if (end && same_x && same_z) || ((same_x || same_z) && !end) {
                        self.bedrock
                    } else {
                        self.air
                    };
                    world.set_block(pos, state, 3)?;
                }
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub struct Podium {
    active: bool,
    bedrock: BlockState,
    end_stone: BlockState,
    air: BlockState,
    portal: BlockState,
    wall_torch: BlockState,
}

impl Podium {
    /// `dropPreviousAndSetBlock`.
    fn drop_previous_and_set<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), ProviderError> {
        let existing = world.block_state(pos)?;
        if world.blocks().block(existing) != world.blocks().block(state) {
            // `level.destroyBlock(pos, true)`: no drops here, the block goes to its fluid.
            if !world.state_data().has(existing, FLAG_AIR) {
                if world.state_data().fluid(existing).fluid != 0 {
                    return Err(ProviderError::Invalid("a podium over a fluid"));
                }
                world.set_block(pos, self.air, 3)?;
            }
            world.set_block(pos, state, 3)?;
        }
        Ok(())
    }

    /// `EndPodiumFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let closer = |pos: BlockPos, distance: f64| {
            let d = |a: i32, b: i32| f64::from(a) - f64::from(b);
            let (dx, dy, dz) = (d(pos.x, origin.x), d(pos.y, origin.y), d(pos.z, origin.z));
            dx * dx + dy * dy + dz * dz < distance * distance
        };
        for z in origin.z - 4..=origin.z + 4 {
            for y in origin.y - 1..=origin.y + 32 {
                for x in origin.x - 4..=origin.x + 4 {
                    let pos = BlockPos::new(x, y, z);
                    let inside_rim = closer(pos, 2.5);
                    if !(inside_rim || closer(pos, 3.5)) {
                        continue;
                    }
                    if y < origin.y {
                        if inside_rim {
                            world.set_block(pos, self.bedrock, 3)?;
                        } else if self.active {
                            self.drop_previous_and_set(world, pos, self.end_stone)?;
                        } else {
                            world.set_block(pos, self.end_stone, 3)?;
                        }
                    } else if y > origin.y {
                        if self.active {
                            self.drop_previous_and_set(world, pos, self.air)?;
                        } else {
                            world.set_block(pos, self.air, 3)?;
                        }
                    } else if !inside_rim {
                        world.set_block(pos, self.bedrock, 3)?;
                    } else if self.active {
                        self.drop_previous_and_set(world, pos, self.portal)?;
                    } else {
                        world.set_block(pos, self.air, 3)?;
                    }
                }
            }
        }
        for y in 0..4 {
            world.set_block(origin.offset(0, y, 0), self.bedrock, 3)?;
        }
        let center = origin.offset(0, 2, 0);
        for face in HORIZONTAL {
            let torch = world
                .blocks()
                .with_property(self.wall_torch, "facing", face_name(face))
                .ok_or(ProviderError::Invalid("wall torch facing"))?;
            world.set_block(relative(center, face), torch, 3)?;
        }
        Ok(true)
    }
}

#[derive(Clone, Debug)]
pub struct BonusChest {
    chest: BlockState,
    torch: BlockState,
}

impl BonusChest {
    /// `BonusChestFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let (min_x, min_z) = (origin.x >> 4 << 4, origin.z >> 4 << 4);
        let mut xs: Vec<i32> = (min_x..=min_x + 15).collect();
        shuffle(&mut xs, random)?;
        let mut zs: Vec<i32> = (min_z..=min_z + 15).collect();
        shuffle(&mut zs, random)?;
        for &x in &xs {
            for &z in &zs {
                let y = world.height(Heightmap::MotionBlockingNoLeaves, x, z)?;
                let chest_pos = BlockPos::new(x, y, z);
                let state = world.block_state(chest_pos)?;
                let no_collision = world
                    .state_data()
                    .collision_shape(state)
                    .ok_or(ProviderError::Invalid("a dynamic shape"))?
                    .is_empty();
                if !(world.state_data().has(state, FLAG_AIR) || no_collision) {
                    continue;
                }
                world.set_block(chest_pos, self.chest, 2)?;
                // `setBlockEntityLootTable`: the chest's block entity takes a loot table seed.
                let there = world.block_state(chest_pos)?;
                if world.state_data().has(there, FLAG_RANDOMIZABLE_CONTAINER) {
                    random.next_long();
                }
                for direction in HORIZONTAL {
                    let torch_pos = relative(chest_pos, direction);
                    if world.block_survives(self.torch, torch_pos)? {
                        world.set_block(torch_pos, self.torch, 2)?;
                    }
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Clone, Debug)]
pub struct IceSpike {
    state: BlockState,
    can_place_on: BlockPredicate,
    can_replace: BlockPredicate,
}

impl IceSpike {
    fn replaceable(
        &self,
        world: &impl PlacementWorld,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let state = world.block_state(pos)?;
        Ok(world.state_data().has(state, FLAG_AIR) || self.can_replace.test(world, pos)?)
    }

    /// `SpikeFeature.place`.
    fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let mut origin = origin;
        while is_air(world, origin)? && origin.y > world.min_y() + 2 {
            origin = origin.offset(0, -1, 0);
        }
        if !self.can_place_on.test(world, origin)? {
            return Ok(false);
        }
        origin = origin.offset(0, random.next_int_bound(4)?, 0);
        let height = random.next_int_bound(4)? + 7;
        let width = height / 4 + random.next_int_bound(2)?;
        if width > 1 && random.next_int_bound(60)? == 0 {
            origin = origin.offset(0, 10 + random.next_int_bound(30)?, 0);
        }
        for y_off in 0..height {
            let scale = (1.0 - y_off as f32 / height as f32) * width as f32;
            let new_width = scale.ceil() as i32;
            for xo in -new_width..=new_width {
                let dx = xo.abs() as f32 - 0.25;
                for zo in -new_width..=new_width {
                    let dz = zo.abs() as f32 - 0.25;
                    if (xo == 0 && zo == 0 || dx * dx + dz * dz <= scale * scale)
                        && (xo != -new_width
                            && xo != new_width
                            && zo != -new_width
                            && zo != new_width
                            || random.next_float() <= 0.75)
                    {
                        let positive = origin.offset(xo, y_off, zo);
                        if self.replaceable(world, positive)? {
                            world.set_block(positive, self.state, 3)?;
                        }
                        if y_off != 0 && new_width > 1 {
                            let negative = origin.offset(xo, -y_off, zo);
                            if self.replaceable(world, negative)? {
                                world.set_block(negative, self.state, 3)?;
                            }
                        }
                    }
                }
            }
        }
        let pillar_width = (width - 1).clamp(0, 1);
        for xo in -pillar_width..=pillar_width {
            for zo in -pillar_width..=pillar_width {
                let mut cursor = origin.offset(xo, -1, zo);
                let mut run_length = 50;
                if xo.abs() == 1 && zo.abs() == 1 {
                    run_length = random.next_int_bound(5)?;
                }
                while cursor.y > 50 {
                    let state = world.block_state(cursor)?;
                    if !world.state_data().has(state, FLAG_AIR)
                        && !self.can_replace.test(world, cursor)?
                        && state != self.state
                    {
                        break;
                    }
                    world.set_block(cursor, self.state, 3)?;
                    cursor = cursor.offset(0, -1, 0);
                    run_length -= 1;
                    if run_length <= 0 {
                        cursor = cursor.offset(0, -(random.next_int_bound(5)? + 1), 0);
                        run_length = random.next_int_bound(5)?;
                    }
                }
            }
        }
        Ok(true)
    }
}
