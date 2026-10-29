//! The playable game loop: streaming world, player simulation at 20 ticks/s,
//! block interaction, inventory and HUD.
mod advancements;
mod allays;
mod armor_stands;
mod beacon;
mod beehives;
mod block_entity_anims;
mod block_entity_ticks;
mod block_items;
mod blockctl;
mod boat_passengers;
mod boats;
mod books;
mod bossbars;
mod breezes;
mod brush;
mod campfire;
mod campfire_douse;
mod cartography;
mod cat_spawner;
mod chat;
mod chiseled_bookshelf;
mod combat;
mod command_blocks;
mod command_harness;
mod commands;
mod composter;
mod consume;
mod copper_golems;
mod creaking;
mod creatures;
mod crossbow;
pub(crate) mod cushions;
mod decorated_pot;
mod dedicated;
mod deflect;
mod dispense_fixture;
mod dispenser;
mod dragon;
mod enchant_effects;
mod end;
mod explosion;
mod falling_damage;
mod fangs;
mod fireworks;
mod fishing;
mod fixtures;
mod frames;
mod functions;
mod game_mode;
mod ghasts;
mod glowing;
mod golems;
mod growth;
mod hand;
mod initial_spawn;
mod jukebox;
mod keys;
mod lan;
mod lan_actions;
mod lan_attributes;
mod lan_chat;
mod lan_crafting;
mod lan_entities;
mod lan_mobs;
mod lan_mount_menu;
mod lan_particles;
mod lan_pvp;
mod lan_stations;
mod lan_trading;
mod lan_vehicles;
mod lan_waypoints;
mod leads;
mod level_particles;
mod levels;
mod lightning;
mod llamas;
mod loom;
mod mace;
mod machines;
mod maps;
mod minecarts;
mod mob_blocks;
mod mob_gear;
mod mount_menu;
mod mounts;
mod names;
mod natural_spawning;
mod nautili;
mod nether_portal;
mod ominous_item_spawner;
mod paintings;
mod pandas;
mod passengers;
mod persist;
mod pick_entity;
mod piglins;
mod piston_push;
mod player_equip;
mod plugin_host;
mod postprocess;
mod potions;
mod powder_snow;
mod presentation;
mod pumpkin;
mod raids;
mod respawn_anchor;
mod scoreboard;
mod shared;
mod stations;
mod trading;
pub use plugin_host::set_plugins;
mod sculk;
mod sculk_fixture;
mod server_control;
mod server_travel;
mod shield;
mod shoulders;
mod sign_edit;
mod sniffers;
mod spawner;
mod spear;
mod spill;
mod stats;
mod storage;
mod structures;
mod sulfur_cubes;
mod tnt;
mod totem;
mod trial_spawner;
mod trident;
mod vault;
mod villagers;
mod wandering_trader;
mod wind;
mod wither;
mod world_loading;
mod zombie_reinforcements;
mod zombie_villagers;
use crate::game::keys::KeyCode;
use crate::hc::{
    Result,
    assets::Assets,
    biome::Biomes,
    blocks::{BlockFacts, Fluid},
    chunk::{HEIGHT, MIN_Y},
    clock::FixedStep,
    collisions,
    containers::{BlockEntity, Providers},
    crafting::Recipes,
    entities::{Experience, ExperienceOrb, ItemEntity},
    environment::Environment,
    first_person::{Hand, SwingAnimation},
    furnace::FurnaceCooking,
    items::{ItemStack, Items},
    light::LightTables,
    mobs::{Mob, MobType, MobTypes},
    motion::{MotionEvent, MotionResources, PlayerMotion},
    particles::Particle,
    save::{Level, Persistent, SaveDir},
    sprint::Keys,
    streaming::Streaming,
    testgen::TestTerrain,
    vanillagen::VanillaTerrain,
    world::World,
};
use horizon_core::{
    biome::{BiomeZoom, obfuscate_seed},
    geometry::{Aabb, Direction},
    position::BlockPos,
};
use horizon_core::{geometry::Vec3, random::RandomSource};
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    collision_context::{CollisionContext, CollisionEntity, CollisionItem},
    entity_obstruction::{EntityIdentity, EntityObstructionWorld, ObstructionEntity},
    level::{
        Behaviours, Dimension as LevelDimension, EntityQuery, Landing, LevelAccess, LevelEvent,
        PlaceContext, PlaceOutcome, PlayerContext, PlayerEffect, Ticks, flags as level_flags,
    },
    registry::BlockStateId,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

#[derive(Clone)]
struct Options {
    /// `--resources DIR`: the prepared 26.3 data (`horizon-prepare`).
    resources: PathBuf,
    /// The view distance before server.properties is read; `--render-distance`
    /// (a test's) sets it for good.
    distance: i32,
    distance_override: Option<i32>,
    /// `--test-terrain`: simple terrain instead of vanilla generation.
    test_terrain: bool,
    /// `--time`, `--weather` and `--dimension`: the world's at start (a
    /// test's), the spawn dimension of its level.
    time: Option<i64>,
    weather: Option<String>,
    dimension: Option<String>,
    /// `--control-socket PATH`: what `horizon-ctl` and a test client's
    /// `--server-control` ask (game/server_control.rs).
    control: Option<PathBuf>,
    /// `HORIZON_SPIKES=MS`: print the timed spans of ticks slower than MS.
    spikes: Option<f32>,
    /// Vanilla's server arguments (`--nogui`, `--port`, `--world`, `--universe`,
    /// `--initSettings`), after `--dedicated`.
    dedicated: Vec<String>,
}

/// The server's test fixtures: synthetic levels, no players (game/fixtures.rs).
const COMPONENT_FIXTURES: &[&str] = &["HORIZON_DISPENSE_FIXTURE", "HORIZON_SCULK_FIXTURE"];

impl Options {
    fn read(args: Vec<String>) -> Result<Option<Self>> {
        if !args
            .iter()
            .any(|a| a == "--help" || a == "-h" || a == "--dedicated")
            && !COMPONENT_FIXTURES
                .iter()
                .any(|v| std::env::var_os(v).is_some())
        {
            return Err("horizon-server runs dedicated (--dedicated)".into());
        }
        let sibling = std::env::current_exe()?
            .parent()
            .ok_or("missing executable directory")?
            .join("resources");
        let mut o = Self {
            resources: if sibling.join("horizon-resources.json").exists() {
                sibling
            } else {
                std::path::Path::new(".local/horizon-client").join(horizon_core::VANILLA_VERSION)
            },
            distance: 12,
            distance_override: None,
            test_terrain: false,
            time: None,
            weather: None,
            dimension: None,
            control: None,
            spikes: std::env::var("HORIZON_SPIKES")
                .ok()
                .and_then(|v| v.parse().ok()),
            dedicated: Vec::new(),
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    println!(
                        "Horizon server — Minecraft Java Edition {} in Rust\n\n  --resources PATH        Prepared resources (horizon-prepare)\n  --control-socket PATH   Accept control requests (tests, horizon-ctl)\n  --test-terrain          Simple test terrain instead of vanilla generation\n  --time TICKS            The time of day at start\n  --weather clear|rain|thunder  The weather at start\n  --dimension DIM         The level a new world's player starts in\n\nThen vanilla's server options (--nogui, --port, --world, --universe, --initSettings).",
                        horizon_core::VANILLA_VERSION
                    );
                    return Ok(None);
                }
                "--dedicated" => {
                    o.dedicated = args.by_ref().collect();
                    break;
                }
                "--test-terrain" => o.test_terrain = true,
                // Horizon's own commands (/pregen), off by default as on vanilla.
                "--pregen" => crate::hc::command::vanilla::set_horizon_commands(true),
                _ => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("missing value after {arg}"))?;
                    match arg.as_str() {
                        "--resources" => o.resources = value.into(),
                        "--render-distance" | "--radius" => {
                            o.distance = value.parse()?;
                            o.distance_override = Some(o.distance);
                            if !(2..=64).contains(&o.distance) {
                                return Err("render distance must be 2..64".into());
                            }
                        }
                        "--control-socket" => o.control = Some(value.into()),
                        "--time" => o.time = Some(value.parse()?),
                        "--weather" => o.weather = Some(value),
                        "--dimension" => {
                            o.dimension = Some(if value.contains(':') {
                                value
                            } else {
                                format!("minecraft:{value}")
                            })
                        }
                        _ => return Err(format!("unknown option {arg}").into()),
                    }
                }
            }
        }
        Ok(Some(o))
    }
}
/// The menu a player has open, as far as the server's gameplay asks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Playing,
    Inventory,
    Dead,
}
struct Mining {
    pos: [i32; 3],
    progress: f32,
    ticks: u32,
}
struct Hit {
    pos: [i32; 3],
    face: [i32; 3],
    point: [f64; 3],
}
/// What a falling block's tick did (`FallingBlockEntity.tick`).
enum FallStep {
    Falling,
    Gone,
    /// Gave up falling: drops itself as an item here.
    Dropped(Vec3),
    /// Hit the ground in this block.
    Landed([i32; 3]),
}
/// A guest's right click comes as one packet per hand and part
/// (`Minecraft.startUseItem`): `ServerGamePacketListenerImpl.handleUseItemOn`
/// does the block's and `ItemStack.useOn`'s part, `handleUseItem` the
/// item's own (`Item.use`). Running both for either would use the item twice.
#[derive(Clone, Copy)]
enum GuestUse {
    OnBlock(Hand),
    Item(Hand),
    /// `handleInteract`: `Player.interactOn` with the hand, nothing else (the
    /// client sends UseItem itself when the entity passed).
    Entity(Hand),
}

/// The client world as seen by the vanilla block-behaviour library.
struct GameLevel<'w> {
    world: &'w mut World,
    facts: &'w BlockFacts,
    collision: &'w BlockCollisionRegistry<'w>,
    events: &'w mut Vec<LevelEvent>,
    dimension: LevelDimension,
    darken: i32,
    /// The environment of the level's dimension (`SUN_ANGLE`).
    environment: &'w Environment,
    raining: bool,
    thundering: bool,
    player_box: Option<([f64; 3], [f64; 3])>,
    /// The LAN guests in this level who trigger blocks (not spectators).
    guest_boxes: Vec<([f64; 3], [f64; 3])>,
    player_collision: Option<PlayerCollision>,
    /// Minecart boxes (detector rails).
    cart_boxes: Vec<([f64; 3], [f64; 3])>,
    /// The item frames a comparator reads through a conductor.
    item_frames: &'w [crate::hc::entities::ItemFrame],
    /// The minecarts, in `cart_boxes` order, for a detector rail's comparator.
    carts: &'w [minecarts::Minecart],
    /// Arrow and thrown trident boxes (wooden buttons).
    arrow_boxes: Vec<([f64; 3], [f64; 3])>,
    /// The other entities that keep blocks from being placed into them.
    blockers: Blockers<'w>,
    /// Item entities, experience orbs and projectiles, which trigger plates and
    /// tripwires but never keep a block from being placed.
    dropped: &'w [ItemEntity],
    orbs: &'w [ExperienceOrb],
    projectiles: &'w [crate::hc::projectiles::Projectile],
    biomes: &'w Biomes,
    seed: i64,
    behaviours: &'w Behaviours,
    items: &'w Items,
    /// Cooking recipes' experience, which a removed furnace pays out.
    recipes: &'w crate::hc::crafting::Recipes,
    /// Container loot tables (`unpackLootTable`).
    loot: Option<std::sync::Arc<horizon_world::loot::LootData>>,
    registry: &'static horizon_world::registry::BlockRegistry,
    rules: horizon_world::level::GameRules,
    /// `Difficulty.getId()` (peaceful 0 .. hard 3).
    difficulty: i32,
    /// The feet of the players who keep fire spreading around them: the local
    /// player and the LAN guests in this level, spectators aside.
    fire_players: Vec<[f64; 3]>,
    /// Block loot random sequences (`ServerLevel.getRandomSequence`).
    rand_sequences: &'w mut crate::hc::random_sequences::RandomSequences,
    pending: &'w mut spill::Pending,
    spill: &'w mut spill::Spill,
    /// Where the chunk tickets let blocks tick.
    tickets: crate::hc::ticket_levels::Levels,
    random_tick_sections: &'w mut crate::hc::random_tick_sections::RandomTickSections,
    /// Players with each chest open (a trapped chest's signal).
    lid_openers: &'w HashMap<(i32, i32, i32), i32>,
    /// `Level.isClientSide`: a client's copy of the level (a guest of a LAN
    /// world, a window on its server thread, a dedicated server's client).
    client_side: bool,
    /// Plugins hear of blocks changing by themselves (`LevelEvent::Changed`).
    change_events: bool,
}
struct PlayerCollision {
    y: f64,
    descending: bool,
    fall_distance: f64,
    main_hand_item: Arc<str>,
    main_hand_count: i32,
    leather_boots: bool,
}
/// The entities `Entity.blocksBuilding` sets besides the players and
/// minecarts, borrowed for `Level.isUnobstructed`: living entities (mobs,
/// armor stands), boats, end crystals, primed TNT and falling blocks.
#[derive(Clone, Copy)]
struct Blockers<'w> {
    mobs: &'w [Mob],
    armor_stands: &'w [crate::hc::entities::ArmorStand],
    boats: &'w [boats::Boat],
    crystals: &'w [dragon::Crystal],
    tnts: &'w [tnt::PrimedTnt],
    falling: &'w [FallingBlock],
}
/// The boxes of the entities that block building, as obstruction candidates.
struct BuildingObstruction(Vec<Aabb>);
impl EntityObstructionWorld for BuildingObstruction {
    type Error = std::convert::Infallible;
    fn obstruction_candidates(
        &mut self,
        _excluded: Option<EntityIdentity>,
        bounds: Aabb,
    ) -> std::result::Result<Vec<ObstructionEntity>, Self::Error> {
        Ok(self
            .0
            .iter()
            .filter(|b| b.intersects(bounds))
            .enumerate()
            .map(|(i, &bounds)| ObstructionEntity {
                identity: EntityIdentity(i as u64),
                root_vehicle: EntityIdentity(i as u64),
                bounds,
                removed: false,
                spectator: false,
                blocks_building: true,
            })
            .collect())
    }
}
impl GameLevel<'_> {
    /// The living entities that trigger plates and tripwires and don't ignore
    /// block triggers (`Entity.isIgnoringBlockTriggers`: bats and marker armor
    /// stands). Armor stands live apart from `mobs`.
    fn mob_trigger_boxes(&self) -> impl Iterator<Item = Aabb> + '_ {
        self.blockers
            .mobs
            .iter()
            .filter(|m| !m.removed && m.species() != crate::hc::mobs::Species::Bat)
            .map(Mob::bounds)
            .chain(
                self.blockers
                    .armor_stands
                    .iter()
                    .filter(|s| !s.marker)
                    .map(|s| s.bounds()),
            )
    }
    /// The non-living entities that trigger plates and tripwires (boats,
    /// crystals, primed TNT, falling blocks, item entities, experience orbs and
    /// projectiles).
    fn other_trigger_boxes(&self) -> impl Iterator<Item = Aabb> + '_ {
        let b = self.blockers;
        let square = |p: Vec3, size: f64| {
            Aabb::new(
                Vec3::new(p.x - size / 2.0, p.y, p.z - size / 2.0),
                Vec3::new(p.x + size / 2.0, p.y + size, p.z + size / 2.0),
            )
        };
        b.boats
            .iter()
            .map(boats::Boat::bounds)
            .chain(b.crystals.iter().map(|c| {
                // EntityType.END_CRYSTAL: 2 x 2.
                let p = c.position;
                Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                )
            }))
            .chain(b.tnts.iter().map(tnt::PrimedTnt::bounds))
            .chain(
                b.falling
                    .iter()
                    .map(|f| crate::hc::falling_block::bounds(f.position)),
            )
            .chain(self.dropped.iter().map(ItemEntity::bounds))
            // EntityType.EXPERIENCE_ORB: 0.5 x 0.5.
            .chain(self.orbs.iter().map(move |o| square(o.position, 0.5)))
            .chain(
                self.projectiles
                    .iter()
                    .map(move |p| square(p.position, p.kind.size())),
            )
    }
    /// `DetectorRailBlock.getAnalogOutputSignal`: a powered rail reads the
    /// minecart on it, a command block cart first (its success count, which
    /// is 0 here), else the first container cart's fullness.
    fn detector_rail_signal(&self, pos: BlockPos, powered: bool) -> i32 {
        if !powered {
            return 0;
        }
        let (x, y, z) = (f64::from(pos.x), f64::from(pos.y), f64::from(pos.z));
        let area = Aabb::new(
            Vec3::new(x + 0.2, y, z + 0.2),
            Vec3::new(x + 0.8, y + 0.8, z + 0.8),
        );
        let mut container = None;
        for ((lo, hi), cart) in self.cart_boxes.iter().zip(self.carts) {
            let bounds = Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            );
            if !area.intersects(bounds) {
                continue;
            }
            match cart.kind {
                minecarts::CartKind::CommandBlock => return 0,
                minecarts::CartKind::Chest | minecarts::CartKind::Hopper => {
                    // Counted here, for the cart a comparator reads, not for
                    // every cart on every block tick.
                    container = container.or_else(|| Some(self.cart_signal(cart)));
                }
                _ => {}
            }
        }
        container.unwrap_or(0)
    }
    /// A container minecart's comparator signal: its open menu's slots while
    /// someone has it open, else its own.
    fn cart_signal(&self, cart: &minecarts::Minecart) -> i32 {
        let key = (i32::MAX - 15, minecarts::CART_KEY_Y, cart.id as i32);
        let slots = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Chest(open)) => open,
            _ => &cart.items,
        };
        minecarts::Minecart::container_signal(slots, self.items)
    }
    /// Every entity box that blocks building: the players who aren't
    /// spectators, the minecarts and the `Blockers` (a removed mob or a marker
    /// armor stand, whose box is empty, doesn't).
    fn building_blocker_boxes(&self) -> Vec<Aabb> {
        let boxed = |(lo, hi): &([f64; 3], [f64; 3])| {
            Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            )
        };
        let b = self.blockers;
        self.player_box
            .iter()
            .chain(&self.guest_boxes)
            .chain(&self.cart_boxes)
            .map(boxed)
            .chain(b.mobs.iter().filter(|m| !m.removed).map(Mob::bounds))
            .chain(
                b.armor_stands
                    .iter()
                    .filter(|s| !s.marker)
                    .map(|s| s.bounds()),
            )
            .chain(b.boats.iter().map(boats::Boat::bounds))
            .chain(b.crystals.iter().map(|c| {
                // EntityType.END_CRYSTAL: 2 x 2.
                let p = c.position;
                Aabb::new(
                    Vec3::new(p.x - 1.0, p.y, p.z - 1.0),
                    Vec3::new(p.x + 1.0, p.y + 2.0, p.z + 1.0),
                )
            }))
            .chain(b.tnts.iter().map(tnt::PrimedTnt::bounds))
            .chain(
                b.falling
                    .iter()
                    .map(|f| crate::hc::falling_block::bounds(f.position)),
            )
            .collect()
    }
}
impl LevelAccess for GameLevel<'_> {
    fn is_client_side(&self) -> bool {
        self.client_side
    }
    fn reports_changes(&self) -> bool {
        self.change_events
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.world.world_surface(x, z, |s| self.facts.is_air(s))
    }
    fn precipitation(&self, pos: BlockPos) -> Option<horizon_world::level::Precipitation> {
        self.behaviours.precipitation_info(self, pos)
    }
    fn is_raining_at(&self, pos: BlockPos) -> bool {
        self.behaviours.is_raining_at(self, pos)
    }
    fn block_entity_analog_signal(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
    ) -> i32 {
        if let Some(s) = self.registry.state(state)
            && s.name == "minecraft:detector_rail"
        {
            return self.detector_rail_signal(
                pos,
                s.properties.get("powered").is_some_and(|v| v == "true"),
            );
        }
        if let Some(signal) = command_blocks::analog_signal(self.world, (pos.x, pos.y, pos.z)) {
            return signal;
        }
        if let Some(signal) = sculk::sensor_signal(self.world, self.behaviours, state, pos) {
            return signal;
        }
        if let Some((_, comparator)) = jukebox::jukebox_signals(self.world, self.items, pos) {
            return comparator;
        }
        if let Some(signal) = creaking::heart_signal(self.world, pos) {
            return if self.registry.state(state).is_some_and(|s| {
                s.properties
                    .get("creaking_heart_state")
                    .is_some_and(|v| v == "uprooted")
            }) {
                0
            } else {
                signal
            };
        }
        if let Some(signal) = books::lectern_signal(self.world, pos) {
            return signal;
        }
        if let Some(signal) =
            machines::shelf_signal(self.world, self.registry, state, pos, direction)
        {
            return signal;
        }
        machines::container_signal(self.world, self.behaviours, self.items, self.registry, pos)
    }
    fn item_frame_analog_output(&self, pos: BlockPos, facing: Direction) -> Option<i32> {
        let mut frames = self
            .item_frames
            .iter()
            .filter(|f| f.pos == [pos.x, pos.y, pos.z] && f.facing == facing as u8);
        let frame = frames.next().filter(|_| frames.next().is_none())?;
        // ItemFrame.getAnalogOutput.
        Some(
            frame
                .item
                .as_ref()
                .map_or(0, |_| i32::from(frame.rotation % 8) + 1),
        )
    }
    fn block_entity_signal(
        &self,
        pos: BlockPos,
        _state: BlockStateId,
        _direction: Direction,
    ) -> i32 {
        // TrappedChestBlock.getSignal: the players with it open, at most 15.
        if let Some(count) = self.lid_openers.get(&(pos.x, pos.y, pos.z)) {
            return (*count).clamp(0, 15);
        }
        jukebox::jukebox_signals(self.world, self.items, pos).map_or(0, |(signal, _)| signal)
    }
    fn biome_name(&self, pos: BlockPos) -> Option<&str> {
        let chunk = self.world.chunks.get(&(pos.x >> 4, pos.z >> 4))?;
        Some(&self.biomes.get(chunk.biome(pos.x, pos.y, pos.z)).name)
    }
    fn world_seed(&self) -> i64 {
        self.seed
    }
    fn day_time(&self) -> i64 {
        self.world.day_time
    }
    fn game_rules(&self) -> horizon_world::level::GameRules {
        self.rules
    }
    fn difficulty(&self) -> i32 {
        self.difficulty
    }
    /// `ServerLevel.canSpreadFireAround`: a player (not a spectator) closer
    /// than the `fire_spread_radius_around_player` rule to the block's corner
    /// (`ChunkMap.anyPlayerCloseEnoughTo`); -1 spreads everywhere.
    fn can_spread_fire_around(&self, pos: BlockPos) -> bool {
        let radius = self.rules.fire_spread_radius_around_player;
        radius == -1
            || self.fire_players.iter().any(|p| {
                let d = [
                    p[0] - f64::from(pos.x),
                    p[1] - f64::from(pos.y),
                    p[2] - f64::from(pos.z),
                ];
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < f64::from(radius) * f64::from(radius)
            })
    }
    fn increased_fire_burnout(&self, pos: BlockPos) -> bool {
        self.world
            .chunks
            .get(&(pos.x >> 4, pos.z >> 4))
            .is_some_and(|chunk| {
                self.biomes
                    .get(chunk.biome(pos.x, pos.y, pos.z))
                    .increased_fire_burnout
            })
    }
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.block(pos.x, pos.y, pos.z)
    }
    fn set_raw(&mut self, pos: BlockPos, state: BlockStateId) -> Option<BlockStateId> {
        if !self.world.loaded(pos.x, pos.z) || pos.y < MIN_Y || pos.y >= MIN_Y + HEIGHT {
            return None;
        }
        let previous = self.world.block(pos.x, pos.y, pos.z);
        self.world.set(pos.x, pos.y, pos.z, state).ok()?;
        Some(previous)
    }
    fn is_loaded(&self, pos: BlockPos) -> bool {
        self.world.loaded(pos.x, pos.z)
    }
    fn min_y(&self) -> i32 {
        if self.dimension != LevelDimension::Overworld {
            0
        } else {
            MIN_Y
        }
    }
    fn max_y(&self) -> i32 {
        // Exclusive, as LevelAccess::max_y documents.
        if self.dimension != LevelDimension::Overworld {
            256
        } else {
            MIN_Y + HEIGHT
        }
    }
    fn random(&mut self) -> &mut horizon_core::random::LegacyRandom {
        &mut self.world.random
    }
    fn game_time(&self) -> i64 {
        self.world.game_time
    }
    fn block_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.world.light(pos.x, pos.y, pos.z) & 15)
    }
    fn sky_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.world.light(pos.x, pos.y, pos.z) >> 4)
    }
    fn sky_darken(&self) -> i32 {
        self.darken
    }
    fn sun_angle(&self, _pos: BlockPos) -> f32 {
        self.environment
            .sample(self.world.day_time, None, None)
            .sun_angle
    }
    fn is_raining(&self) -> bool {
        self.raining
    }
    fn is_thundering(&self) -> bool {
        self.thundering
    }
    fn dimension(&self) -> LevelDimension {
        self.dimension
    }
    /// `Level.isUnobstructed`: the placed state's collision shape against the player.
    /// `Level.isUnobstructed(state, pos, CollisionContext.of(player))`: no
    /// entity that blocks building is in the state's collision shape.
    fn is_unobstructed(&self, pos: BlockPos, state: BlockStateId) -> bool {
        let player = self.player_collision.as_ref();
        let context = player.map_or_else(
            || CollisionContext::empty(false),
            |player| {
                CollisionContext::for_entity(
                    CollisionEntity {
                        identity: EntityIdentity(0),
                        y: player.y,
                        descending: player.descending,
                        main_hand: Some(CollisionItem {
                            item: &player.main_hand_item,
                            count: player.main_hand_count,
                        }),
                    },
                    false,
                )
            },
        );
        let mut scene = collisions::SceneCollisions {
            world: &*self.world,
            binding: self.collision,
            subject: player.map(|player| crate::hc::collisions::PowderSnowSubject {
                fall_distance: player.fall_distance,
                falling_block: false,
                leather_boots: player.leather_boots,
                walkable_type: false,
                lava_walker: false,
            }),
        };
        let Ok(shape) = self
            .collision
            .collision_shape_with_context(&mut scene, state, pos, &context)
        else {
            return false;
        };
        if shape.is_empty() {
            return true;
        }
        let shape = shape.moved(horizon_core::geometry::Vec3::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
        ));
        horizon_world::entity_obstruction::is_unobstructed(
            &mut BuildingObstruction(self.building_blocker_boxes()),
            None,
            &shape,
        )
        .unwrap_or(false)
    }
    /// `ServerLevel.isPositionTickingWithEntitiesLoaded`: block ticking, and
    /// a ticking chunk.
    fn is_ticking_chunk(&self, chunk: horizon_core::position::ChunkPos) -> bool {
        self.tickets.block_ticking(chunk.x, chunk.z) && ticking_chunk(self.world, chunk.x, chunk.z)
    }
    /// Entities that trigger pressure plates, tripwires, detector rails and
    /// wooden buttons: the players (the host and the LAN guests), the mobs,
    /// the minecarts and the other entities that don't ignore block triggers,
    /// and for `EntityQuery::Arrows` the arrows.
    fn count_entities(&self, area: Aabb, query: EntityQuery) -> usize {
        let hit = |(lo, hi): &([f64; 3], [f64; 3])| {
            area.intersects(Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            ))
        };
        let carts = || self.cart_boxes.iter().filter(|b| hit(b)).count();
        let mobs = || {
            self.mob_trigger_boxes()
                .filter(|b| area.intersects(*b))
                .count()
        };
        let others = || {
            self.other_trigger_boxes()
                .filter(|b| area.intersects(*b))
                .count()
        };
        let arrows = || self.arrow_boxes.iter().filter(|b| hit(b)).count();
        let players = || {
            self.player_box
                .iter()
                .chain(&self.guest_boxes)
                .filter(|b| hit(b))
                .count()
        };
        match query {
            EntityQuery::Minecart => carts(),
            EntityQuery::Arrows => arrows(),
            EntityQuery::Mobs => players() + mobs(),
            EntityQuery::Everything => players() + carts() + mobs() + others(),
        }
    }
    fn emit(&mut self, event: LevelEvent) {
        match event {
            // Block.dropResources draws its loot and spots right away.
            // `popResource` only drops server-side (`!level.isClientSide`):
            // a client's moveBlocks (a piston's block event) skips it.
            LevelEvent::DropResources { pos, state } if !self.client_side => {
                self.drop_resources_now(pos, state)
            }
            LevelEvent::DropResources { .. } => {}
            // `ClientLevel.playSeededSound` plays only when the excluded
            // player is this client's own: the block's `playSound(null, ...)`
            // (a piston's move, a note) reaches it from the server instead.
            LevelEvent::Sound { .. } if self.client_side => {}
            // Block.popResource of the block's own item (tall flowers, flower beds).
            LevelEvent::BlockEvent {
                pos,
                state,
                kind: "pop_block_item",
            } => self.pop_block_item(pos, state),
            // ComposterBlock.extractProduce: bone meal above the composter.
            LevelEvent::Player {
                pos,
                effect: PlayerEffect::DropItem(item),
                ..
            } => self.pop_above(pos, item),
            // Picking a sweet berry bush or cave vines: the harvest loot,
            // popped before the pick sound draws its pitch.
            LevelEvent::Player {
                pos,
                state,
                effect: PlayerEffect::InteractLoot(table),
            } => self.drop_interact_loot(pos, state, table),
            event => self.events.push(event),
        }
    }
    fn remove_block_entity(&mut self, pos: BlockPos, old: BlockStateId, side_effects: bool) -> u8 {
        self.remove_entity(pos, old, side_effects)
    }
    fn load_block_entity(&mut self, pos: BlockPos, mut nbt: horizon_world::nbt::Compound) {
        use horizon_world::nbt::Tag;
        for (k, v) in [("x", pos.x), ("y", pos.y), ("z", pos.z)] {
            nbt.insert(k.into(), Tag::Int(v));
        }
        if let Some((key, entity)) = crate::hc::save::decode_block_entity(&Tag::Compound(nbt)) {
            self.world.block_entities.insert(key, entity);
        }
    }
    fn set_command_block_powered(&mut self, pos: BlockPos, powered: bool) -> Option<(bool, bool)> {
        command_blocks::set_powered(self.world, self.registry, (pos.x, pos.y, pos.z), powered)
    }
}
/// FallingBlockEntity carrying a block state.
#[derive(Clone, Debug)]
struct FallingBlock {
    /// The entity id a server tracks it by (0 until it is tracked).
    id: u32,
    state: BlockStateId,
    /// `DATA_START_POS`: spawn position used to select weighted block models.
    start_pos: BlockPos,
    position: Vec3,
    previous: Vec3,
    velocity: Vec3,
    /// `FallingBlockEntity.time`: signed, a save can start it below zero.
    age: i32,
    /// `Entity.isNoGravity`.
    no_gravity: bool,
    /// `FallingBlockEntity.dropItem`.
    drop_item: bool,
    /// `Entity.fallDistance` (powder snow holds a block that fell far at 0.9).
    fall_distance: f64,
    /// `Entity.stuckSpeedMultiplier`: a cobweb it is in scales its next move.
    stuck: Vec3,
    damage: crate::hc::falling_block::Damage,
    /// The UUID and compound it was loaded with, kept while it moves.
    saved: Option<Box<([i32; 4], horizon_world::nbt::Tag)>>,
}
/// The data loaded once at startup and never changed: blocks, items,
/// recipes, biomes, mobs, collision and motion. Read-only and `Send + Sync`,
/// so the client and an integrated server thread can share one copy.
pub struct Resources {
    core: WorldCore,
    rest: std::sync::OnceLock<WorldRest>,
}

struct WorldCore {
    facts: BlockFacts,
    biomes: Arc<Biomes>,
    environment: Environment,
    collision: BlockCollisionRegistry<'static>,
    motion: MotionResources,
    threads: usize,
}

struct WorldRest {
    items: Items,
    recipes: Recipes,
    transformers: crate::hc::block_transform::Transformers,
    mob_types: HashMap<String, Arc<MobType>>,
    mob_data: crate::hc::mobs::MobData,
    providers: Providers,
    nether_environment: Environment,
    end_environment: Environment,
    behaviours: Behaviours,
    stations: crate::hc::stations::StationData,
}

impl Resources {
    fn rest(&self) -> &WorldRest {
        self.rest
            .get()
            .expect("world resources joined before entry")
    }
    fn facts(&self) -> &BlockFacts {
        &self.core.facts
    }
    fn items(&self) -> &Items {
        &self.rest().items
    }
    fn recipes(&self) -> &Recipes {
        &self.rest().recipes
    }
    fn transformers(&self) -> &crate::hc::block_transform::Transformers {
        &self.rest().transformers
    }
    fn biomes(&self) -> &Arc<Biomes> {
        &self.core.biomes
    }
    fn environment(&self) -> &Environment {
        &self.core.environment
    }
    fn mob_types(&self) -> &HashMap<String, Arc<MobType>> {
        &self.rest().mob_types
    }
    fn mob_data(&self) -> &crate::hc::mobs::MobData {
        &self.rest().mob_data
    }
    fn providers(&self) -> &Providers {
        &self.rest().providers
    }
    fn nether_environment(&self) -> &Environment {
        &self.rest().nether_environment
    }
    fn end_environment(&self) -> &Environment {
        &self.rest().end_environment
    }
    fn behaviours(&self) -> &Behaviours {
        &self.rest().behaviours
    }
    fn collision(&self) -> &BlockCollisionRegistry<'static> {
        &self.core.collision
    }
    fn motion(&self) -> &MotionResources {
        &self.core.motion
    }
    fn threads(&self) -> usize {
        self.core.threads
    }
    fn stations(&self) -> &crate::hc::stations::StationData {
        &self.rest().stations
    }
}

/// What the server reads of the resources: the block registry and models'
/// facts (`Assets`) and the translated names. Cloning shares them.
#[derive(Clone)]
pub struct PackResources {
    assets: Arc<Assets>,
    names: Arc<crate::hc::creative_menu::Names>,
    /// The streamer's (meshless) context: the light tables.
    context: Arc<crate::hc::streaming::NoMesh>,
}
impl PackResources {
    fn load(root: &std::path::Path, assets: Arc<Assets>, light: Arc<LightTables>) -> Self {
        Self {
            assets,
            names: Arc::new(crate::hc::creative_menu::Names::load(root)),
            context: Arc::new(crate::hc::streaming::NoMesh { light }),
        }
    }
}
/// Ctrl+Q reads the physical keyboard, independently of sprint key mappings.
/// A chunk whose ticking future can complete (`ChunkMap.prepareTickingChunk`):
/// lit, with its neighbours loaded, as `postprocess_chunks` waits for.
fn ticking_chunk(world: &World, x: i32, z: i32) -> bool {
    world.chunks.get(&(x, z)).is_some_and(|c| c.lit)
        && (-1..=1).all(|dz| (-1..=1).all(|dx| world.chunks.contains_key(&(x + dx, z + dz))))
}

/// Everything of `Game` but the window (`Frontend`): what a server runs.
struct Sim<'a> {
    /// Block entities' ticking fields (lids, bells, shulker boxes, gateways, conduits).
    be_anims: crate::hc::block_entity_anim::BlockEntityAnims,
    /// Shared with an integrated server; `collision` and `motion` borrow from it.
    res: Arc<Resources>,
    collision: &'a BlockCollisionRegistry<'a>,
    motion: &'a MotionResources,
    /// This game's resource packs (`reload_packs` replaces them).
    pack: PackResources,
    world: World,
    streaming: Streaming,
    options: Options,
    player: Option<PlayerMotion>,
    spawn: Option<Vec3>,
    clock: FixedStep,
    previous_position: Vec3,
    eye: f32,
    previous_eye: f32,
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<ItemStack>>,
    selected: usize,
    carried: Option<ItemStack>,
    craft_grid: Vec<Option<ItemStack>>,
    craft_size: usize,
    /// The crafting table the 3x3 grid belongs to (`CraftingMenu.access`).
    craft_table: Option<[i32; 3]>,
    /// A quick craft drag over inventory slots: (button, game slots so far).
    inv_drag: Option<(i32, Vec<usize>)>,
    /// The last inventory click, for the double click that gathers a stack.
    last_inv_click: Option<(std::time::Instant, usize, bool)>,
    /// Open container block entity, if any.
    container: Option<(i32, i32, i32)>,
    /// Open workstation menu (enchanting table, anvil, ...).
    station: Option<stations::Station>,
    /// Status effects and thrown potions (game/potions.rs).
    potions: potions::PotionState,
    scratch_slot: Option<ItemStack>,
    mining: Option<Mining>,
    /// The other players' cracks (`LevelRenderer.destroyingBlocks`): breaker id to the block,
    /// the stage (0..=9) and the game time it last changed.
    destroying: HashMap<i32, ([i32; 3], u8, i64)>,
    destroy_delay: u32,
    use_delay: u32,
    creative: bool,
    /// The game mode; `creative` mirrors `mode == Creative`.
    mode: crate::hc::command::args::GameMode,
    keys: HashSet<KeyCode>,
    // Physical input is separate from remapped and toggled movement keys.
    left: bool,
    right: bool,
    screen: Screen,
    ticks: u64,
    /// What ends the game with an error; kept as it is, so main can tell a debug crash.
    error: Option<Box<dyn std::error::Error>>,
    message: Option<(String, Instant)>,
    save: Arc<SaveDir>,
    level: Level,
    session_lock: Option<crate::hc::save::SessionLock>,
    /// The sounds the game decided to play (seeds, the guests' capture, the control socket's
    /// counts); `flush_sounds` hands them to `audio`.
    sounds: crate::hc::sound_out::SoundOut,
    /// `LocalPlayer.itemUsed` for the first-person hand heights, applied by the window
    /// before it ticks them.
    hands_used: Vec<crate::hc::first_person::Hand>,
    /// The pointer grab a screen change asked for (`capture`), for the window to apply.
    pointer_grab: Option<bool>,
    dropped: Vec<ItemEntity>,
    next_entity: u32,
    particles: crate::hc::particles::ParticleEngine,
    /// `ItemPickupParticle`s: taken items, orbs and arrows flying to their collector.
    /// Sign text being edited (`SignEditScreen`).
    sign_edit: Option<sign_edit::SignEditor>,
    /// An open book, book and quill or lectern screen.
    book: Option<books::BookScreen>,
    /// Lecterns whose page-turn pulse is running (ticks left).
    lectern_pulses: Vec<((i32, i32, i32), i32)>,
    /// The world spawn search's origin chunk and fallback height, once known.
    spawn_origin: Option<((i32, i32), i32)>,
    spawn_spiral: crate::hc::world_spawn::ChunkSpiral,
    /// A new player's spawn search, while its candidates' chunks load.
    player_spawn: Option<crate::hc::world_spawn::PlayerSpawnFinder>,
    /// Chunks a respawn search waits for this tick, kept loading beside the
    /// player's own (taken by `update_world`).
    spawn_wait: Vec<(i32, i32)>,
    mobs: Vec<Mob>,
    creatures: creatures::Creatures,
    sculk: sculk::SculkState,
    raiding: raids::Raiding,
    attack_ticks: u32,
    /// The main hand's item at the last player tick, whose attack damage and
    /// speed modifiers apply (`LivingEntity.detectEquipmentUpdates`): a hotbar
    /// swap just before an attack keeps them, with the new item's enchantments
    /// (attribute swapping).
    attack_item: Option<Arc<str>>,
    /// Mobs a player hurt (`LivingEntity.lastHurtByPlayer`), with the guest
    /// it was by connection (None: the host).
    last_hurt_by_player: HashMap<u32, Option<u32>>,
    /// Mobs whose death (`LivingEntity.die`) has been handled.
    died: HashSet<u32>,
    /// Item being used (eaten): hotbar slot and ticks used.
    using: Option<(usize, u32)>,
    /// Armor slots: head, chest, legs, feet.
    armor: [Option<ItemStack>; 4],
    /// The off hand (Inventory slot 40, saved as slot 150; menu slot 504 here).
    offhand: Option<ItemStack>,
    /// The hand an item use acts with (`InteractionHand`): the off hand only
    /// for its turn in `use_item` and while its item is being used.
    hand: Hand,
    /// The shield's item cooldown after an axe disabled it (ticks).
    shield_cooldown: u32,
    /// `Player.getCooldowns()`: the `use_cooldown` component's entries, what
    /// `itemDecorations` shades a slot by. A server's players have their own
    /// (`GuestPlay::cooldowns`), swapped in for their turn.
    cooldowns: crate::hc::cooldowns::Cooldowns,
    /// The source of a command run through /execute (the player otherwise).
    command_source: Option<commands::execute::Source>,
    /// This game's world is autosaving this tick (`MinecraftServer.isSaving`).
    autosaving: bool,
    applied_modifiers: Vec<(String, String)>,
    experience: Experience,
    orbs: Vec<ExperienceOrb>,
    xp_delay: u32,
    last_level_up: u64,
    /// The arm swing (`LivingEntity.SwingState`) and how many swings started.
    swing: crate::hc::first_person::SwingState,
    swings_started: u32,
    /// `Minecraft`'s attack handling, attack clicks since the last tick and
    /// the attack strength tickers the hand reads.
    clicks: crate::hc::first_person::Clicks,
    attack_clicks: u32,
    hand_strength: crate::hc::attack_strength::AttackStrength,
    settings: Settings,
    flash: u32,
    /// Bolts that struck here and sky flashes they made, in total (`status`).
    lightning_seen: [u32; 2],
    /// The head of the bed while sleeping; the visual timer also survives waking.
    sleeping: Option<[i32; 3]>,
    sleep_timer: u32,
    /// `SleepStatus`'s active and sleeping players at the last announcement.
    sleep_seen: (i64, i64),
    dim: Dim,
    world_root: PathBuf,
    /// The world is superflat (`ServerLevel.isFlat`, for every dimension).
    world_flat: bool,
    /// A player carried through a portal, re-placed once the destination loads.
    travelling: Option<(PlayerMotion, Vec3)>,
    /// A server's players on their way to another level, each waiting for
    /// the destination to load around them.
    server_trips: Vec<server_travel::Trip>,
    /// Players a command sends to another level, once it has run.
    level_teleports: Vec<server_travel::LevelTeleport>,
    /// A server's levels other than the one in place (game/levels.rs).
    levels: levels::Levels,
    /// Which pass of a server tick runs (`tick_server`): the first one reads
    /// the network, the last one ends the players' tick.
    level_pass: levels::Pass,
    /// Nether portals: the player's, items' and mobs' (game/nether_portal.rs).
    portals: nether_portal::Portals,
    /// The respawn point when it is not the world spawn or a bed: a respawn
    /// anchor (`game/respawn_anchor.rs`).
    respawn_config: Option<respawn_anchor::RespawnConfig>,
    /// A respawn travelling to its anchor's dimension and chunks.
    respawn_travel: Option<respawn_anchor::RespawnConfig>,
    /// The next enter_dimension call comes from an End portal.
    end_portal_travel: bool,
    end: end::EndState,
    block_ticks: Ticks,
    tick_chunks: crate::hc::tick_chunk_membership::TickChunkMembership,
    tickets: crate::hc::ticket_levels::ChunkTickets,
    random_tick_sections: crate::hc::random_tick_sections::RandomTickSections,
    level_events: Vec<LevelEvent>,
    falling: Vec<FallingBlock>,
    paintings: Vec<crate::hc::entities::Painting>,
    painting_variants: paintings::PaintingVariants,
    item_frames: Vec<crate::hc::entities::ItemFrame>,
    armor_stands: Vec<crate::hc::entities::ArmorStand>,
    /// `Cushion` decoration entities (game/cushions.rs).
    cushions: Vec<crate::hc::entities::Cushion>,
    /// Entities of types Horizon does not simulate.
    others: Vec<commands::OtherEntity>,
    boats: Vec<boats::Boat>,
    boat_models: boats::BoatModels,
    carts: Vec<minecarts::Minecart>,
    /// Songs playing from jukeboxes (tracked sound handles).
    jukebox_sounds: HashSet<(i32, i32, i32)>,
    bolts: Vec<lightning::Bolt>,
    maps: maps::Maps,
    /// The ender dragon fight (game/dragon.rs).
    fight: dragon::Fight,
    dragon: Option<dragon::Dragon>,
    crystals: Vec<dragon::Crystal>,
    dragon_fireballs: Vec<dragon::DragonFireball>,
    breath_clouds: Vec<dragon::Breath>,
    /// The entity ids a hosted server gives the knots its mobs hang on, by
    /// block, while a lead is tied there.
    knot_ids: HashMap<[i32; 3], u32>,
    /// The boat the player rides.
    riding: Option<u32>,
    /// The ender inventory (27 slots) and the ender chest showing it.
    ender_items: Vec<Option<ItemStack>>,
    ender_open: Option<(i32, i32, i32)>,
    /// Block entity data of broken blocks waiting for their dropped item.
    pending: spill::Pending,
    /// Item entities spawned and block entities removed while the block
    /// behaviour ran (spilled contents, popped resources).
    spill: spill::Spill,
    /// The horse inventory (game/mount_menu.rs).
    mount_menu: mount_menu::MountMenu,
    /// The player's fishing hook.
    hook: Option<fishing::Hook>,
    /// The other players' hooks a server tracks to this client.
    remote_hooks: Vec<fishing::RemoteHook>,
    tridents: Vec<trident::ThrownTrident>,
    rockets: Vec<fireworks::Rocket>,
    /// Ominous trial spawners' item spawners.
    ominous_spawners: Vec<ominous_item_spawner::OminousItemSpawner>,
    creaking: creaking::CreakingState,
    /// Monster spawners' display mobs (`game/spawner.rs`).
    spawners: spawner::SpawnerState,
    /// Reused candidate buffers for natural spawning.
    spawn_scratch: natural_spawning::SpawnScratch,
    tnts: Vec<tnt::PrimedTnt>,
    hoppers: HashMap<(i32, i32, i32), machines::HopperState>,
    /// Where item entities are, while the block entities tick (hoppers).
    item_columns: Option<machines::ItemColumns>,
    /// The running agent action from the control socket (game/agent.rs).
    agent: server_control::Agent,
    /// The control endpoint a test or horizon-ctl asks the server through (`--control-socket`).
    control: Option<crate::hc::control::Server>,
    /// The overworld's last `sky_darken` and its inputs (time, rain, thunder).
    sky_darken_memo: std::cell::Cell<Option<((i64, u32, u32), i32)>>,
    persist: persist::EntityMemory,
    /// This world is open to the network (game/lan.rs).
    lan: Option<lan::LanHost>,
    /// horizon-server's plugins (game/plugin_host.rs), what they asked for
    /// that waits for the hub, and the players they last saw.
    plugins: Option<Box<horizon_server_api::PluginHost>>,
    plugin_actions: Vec<horizon_server_api::Action>,
    plugin_seen: Vec<horizon_server_api::PlayerRef>,
    /// Blocks a plugin kept from a player's break: the connection, where, the state.
    plugin_resend: Vec<(u32, [i32; 3], i32)>,
    /// Players whose inventory a plugin kept as it was (a cancelled drop):
    /// their client is sent it again.
    plugin_resync: Vec<u32>,
    /// Where each player last was for `PlayerMoveEvent`, by connection.
    plugin_last_moves: HashMap<u32, horizon_server_api::Location>,
    /// MultiPlayerGameMode.previousLocalPlayerMode (F3+N goes back to it).
    previous_mode: Option<crate::hc::command::args::GameMode>,
    /// The block a guest is acting on (game/lan_actions.rs).
    forced_hit: Option<Hit>,
    /// The part of a right click a guest's packet asks for (game/lan_actions.rs).
    guest_use: Option<GuestUse>,
    /// Level events of this tick for the guests, and the guest acting now.
    lan_events: Vec<(i32, [i32; 3], i32, Option<u32>)>,
    /// Jukebox level events 1010 (song name) and 1011 (`None`) for the guests;
    /// the song's network id is only known once the hub is in hand.
    lan_songs: Vec<([i32; 3], Option<String>)>,
    lan_cushion_breaks: Vec<(horizon_world::registry::BlockStateId, [f64; 3])>,
    /// Block events (position, b0, b1, state) and entity events (id, event,
    /// position) for the guests.
    lan_block_events: Vec<([i32; 3], i32, i32, u32)>,
    lan_entity_events: Vec<(i32, i8, [f64; 3])>,
    /// Successful crits (attacking guest, hit entity), queued while the hub is taken.
    lan_critical_hits: Vec<(Option<u32>, i32, bool)>,
    /// Spear stabs that landed (the stabbing guest), queued while the hub is
    /// taken: `broadcastEntityEvent(player, 2)`.
    lan_kinetic_hits: Vec<Option<u32>>,
    /// `LivingEntity.recentKineticEnemies`: when each player (a guest, or the
    /// window's own) last stabbed each entity this use.
    stabbed: HashMap<(Option<u32>, u32), i64>,
    /// The movement a guest's client reported this tick
    /// (`ServerPlayer.getKnownMovement`), for their turn.
    known_movement: Vec3,
    /// `LivingEntity.lastKineticHitFeedbackTime` of the entities that felt a
    /// spear's hit (`spear::OWN` is this window's player), in game time.
    kinetic_feedback: HashMap<i32, i64>,
    /// Global level events for every guest (`ServerLevel.globalLevelEvent`).
    lan_global_events: Vec<(i32, [i32; 3], i32)>,
    /// `ContainerOpenersCounter`: how many players (the host and the guests)
    /// have each chest open.
    lid_openers: HashMap<(i32, i32, i32), i32>,
    lan_acting: Option<u32>,
    /// The name of the guest `lan_acting` acts for (`Player.getName`).
    lan_acting_name: Option<String>,
    /// Their UUID (`Player.getUUID`).
    lan_acting_uuid: Option<u128>,
    /// Command feedback to forward to a guest.
    chat_capture: Option<Vec<crate::hc::text::Text>>,
    /// Packets for the guest acting now, sent once its action returns.
    guest_packets: Vec<horizon_protocol::packets::play::Clientbound>,
    /// The acting guest's cooldowns, as (item, ticks): their packet needs the
    /// hub's item registry, which the guest's turn does not hold.
    guest_cooldowns: Vec<(Arc<str>, i32)>,
    /// The acting guest's own entity events (`ServerPlayer.connection.send`),
    /// sent with their entity id, which the guest's turn does not hold.
    guest_entity_events: Vec<i8>,
    /// `broadcastToAdmins` lines a guest's command queued, sent once it
    /// returns: the line, whether operators get it and whether the console
    /// logs it.
    admin_feedback: Vec<(crate::hc::text::Text, bool, bool)>,
    /// The server console is running a command: whether its feedback goes to
    /// the operators (`DedicatedServer.shouldInformAdmins`,
    /// `broadcast-console-to-ops`).
    console_broadcast: Option<bool>,
    /// State of `/spectate`, `/posteffect`, `/debug`, `/waypoint` and friends.
    cmds: commands::extra::CommandExtras,
    ui: chat::UiState,
    /// Game rules, difficulty, the clock and the command registries (vanilla's `ServerLevelData`).
    world_data: crate::hc::world_data::WorldData,
    combat: combat::CombatTracker,
    /// Each guest's own `CombatTracker`, out of `combat` between turns.
    guest_combat: std::collections::HashMap<u32, combat::CombatTracker>,
    mount: mounts::MountState,
    stats: stats::Stats,
    score: scoreboard::ScoreState,
    fns: functions::FnState,
    bosses: bossbars::BossState,
    exec: commands::execute::ExecState,
    rand_sequences: crate::hc::random_sequences::RandomSequences,
    /// `BrushableBlockEntity`'s brush counts of the blocks being brushed.
    brushing: brush::Brushing,
    border: crate::hc::worldborder::WorldBorder,
    adv: advancements::AdvState,
    restore_player_state: bool,
    fangs: fangs::FangState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Dim {
    Overworld,
    Nether,
    End,
}
impl Dim {
    fn id(self) -> &'static str {
        match self {
            Self::Overworld => "minecraft:overworld",
            Self::Nether => "minecraft:the_nether",
            Self::End => "minecraft:the_end",
        }
    }
    fn from_id(id: &str) -> Self {
        match id {
            "minecraft:the_nether" => Self::Nether,
            "minecraft:the_end" => Self::End,
            _ => Self::Overworld,
        }
    }
    /// `Level.getMinY`: the dimension type's minimum y.
    fn min_y(self) -> i32 {
        match self {
            Self::Overworld => MIN_Y,
            Self::Nether | Self::End => 0,
        }
    }
    /// `DimensionType.logicalHeight`.
    fn logical_height(self) -> i32 {
        match self {
            Self::Overworld => HEIGHT,
            Self::Nether => 128,
            Self::End => 256,
        }
    }
}
fn facing_offset(facing: &str) -> [i32; 3] {
    match facing {
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        "up" => [0, 1, 0],
        _ => [0, -1, 0],
    }
}
/// How far along `dir` (within `reach`) a ray from `eye` enters `b`.
fn ray_box(eye: [f64; 3], dir: [f64; 3], reach: f64, b: Aabb) -> Option<f64> {
    let (min, max) = (b.min_position(), b.max_position());
    let (lo, hi) = ([min.x, min.y, min.z], [max.x, max.y, max.z]);
    let (mut t0, mut t1) = (0.0f64, reach);
    for a in 0..3 {
        if dir[a].abs() < 1e-9 {
            if eye[a] < lo[a] || eye[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
    }
    (t0 <= t1).then_some(t0)
}
fn view_vector(yaw: f32, pitch: f32) -> [f64; 3] {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
}
impl crate::game::Sim<'_> {
    fn eye_position(&self) -> Option<[f64; 3]> {
        let p = self.player.as_ref()?;
        Some([
            p.position.x,
            p.position.y + f64::from(p.dimensions.eye_height),
            p.position.z,
        ])
    }
    fn reach(&self) -> f64 {
        if self.creative { 5.0 } else { 4.5 }
    }
    /// Voxel traversal against non-air, non-fluid blocks, clipped against
    /// each block's outline shape (`BlockGetter.clip` with `ClipContext.Block.OUTLINE`).
    fn pick(&self) -> Option<Hit> {
        // A guest of a hosted world acts on the block their client targeted.
        if let Some(hit) = &self.forced_hit {
            return Some(Hit {
                pos: hit.pos,
                face: hit.face,
                point: hit.point,
            });
        }
        let origin = self.eye_position()?;
        self.clip_ray(origin, view_vector(self.yaw, self.pitch), self.reach())
    }
    /// The first block outline hit within `reach` along the unit `dir`.
    fn clip_ray(&self, origin: [f64; 3], dir: [f64; 3], reach: f64) -> Option<Hit> {
        let mut pos = origin.map(|v| v.floor() as i32);
        let step = dir.map(|d| if d > 0.0 { 1 } else { -1 });
        let delta = dir.map(|d| {
            if d == 0.0 {
                f64::INFINITY
            } else {
                (1.0 / d).abs()
            }
        });
        let mut t_max = [0.0; 3];
        for i in 0..3 {
            let boundary = if dir[i] > 0.0 {
                f64::from(pos[i]) + 1.0
            } else {
                f64::from(pos[i])
            };
            t_max[i] = if dir[i] == 0.0 {
                f64::INFINITY
            } else {
                (boundary - origin[i]) / dir[i]
            };
        }
        let mut face = [0; 3];
        let mut t = 0.0;
        while t <= reach {
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let f = self.res.facts().get(state);
            if !self.res.facts().is_air(state)
                && (f.fluid == Fluid::None
                    || f.waterlogged
                    || &*f.name == "minecraft:kelp"
                    || f.render_shape == crate::hc::blocks::RenderShape::Model)
                && !matches!(
                    &*f.name,
                    "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column"
                )
            {
                // The nearest entry into one of the outline's boxes (slab method).
                let mut best: Option<(f64, [i32; 3])> = None;
                for b in self.outline_boxes(pos) {
                    let (mut t0, mut t1, mut axis_in, mut sign) = (0.0f64, f64::INFINITY, None, 0);
                    let mut miss = false;
                    for a in 0..3 {
                        if dir[a] == 0.0 {
                            miss |= origin[a] < b[a] || origin[a] > b[a + 3];
                            continue;
                        }
                        let (lo, hi) =
                            ((b[a] - origin[a]) / dir[a], (b[a + 3] - origin[a]) / dir[a]);
                        let (near, far) = if lo < hi { (lo, hi) } else { (hi, lo) };
                        if near > t0 {
                            t0 = near;
                            axis_in = Some(a);
                            sign = if dir[a] > 0.0 { -1 } else { 1 };
                        }
                        t1 = t1.min(far);
                    }
                    if miss || t0 > t1 || t0 > reach {
                        continue;
                    }
                    let mut normal = face;
                    if let Some(a) = axis_in {
                        normal = [0; 3];
                        normal[a] = sign;
                    }
                    if best.is_none_or(|(bt, _)| t0 < bt) {
                        best = Some((t0, normal));
                    }
                }
                if let Some((t, face)) = best {
                    return Some(Hit {
                        pos,
                        face,
                        point: [
                            origin[0] + dir[0] * t,
                            origin[1] + dir[1] * t,
                            origin[2] + dir[2] * t,
                        ],
                    });
                }
            }
            let axis = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] { 0 } else { 2 }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            t = t_max[axis];
            t_max[axis] += delta[axis];
            pos[axis] += step[axis];
            face = [0; 3];
            face[axis] = -step[axis];
        }
        None
    }
    /// The stack in the hand that acts (`getItemInHand(hand)`).
    fn held(&self) -> Option<&ItemStack> {
        self.stack_in(self.held_slot())
    }
    fn held_mut(&mut self) -> &mut Option<ItemStack> {
        self.stack_in_mut(self.held_slot())
    }
    /// The slot of the hand that acts: the selected one, or `OFFHAND_SLOT`.
    /// `LivingEntity.canGlideUsing`: a glider in the chest slot that will
    /// not break with its next point of wear.
    fn glider_worn(&self) -> bool {
        self.armor[1].as_ref().is_some_and(|s| {
            self.res
                .items()
                .get(&s.item)
                .is_some_and(|i| i.glider && (i.max_damage == 0 || s.damage + 1 < i.max_damage))
        })
    }
    /// `LivingEntity.canFreeze`'s armor half: any worn piece in
    /// `freeze_immune_wearables`.
    pub(crate) fn wears_freeze_immune(&self) -> bool {
        let immune = self
            .res
            .items()
            .item_tags
            .get("minecraft:freeze_immune_wearables");
        self.armor
            .iter()
            .flatten()
            .any(|s| immune.is_some_and(|t| t.contains(&*s.item)))
    }
    fn held_slot(&self) -> usize {
        match self.hand {
            Hand::Main => self.selected,
            Hand::Off => shield::OFFHAND_SLOT,
        }
    }
    /// `LivingEntity.getUsedItemHand`: the hand of the item in use.
    fn used_hand(&self) -> Option<Hand> {
        self.using.map(|(slot, _)| {
            if slot == shield::OFFHAND_SLOT {
                Hand::Off
            } else {
                Hand::Main
            }
        })
    }
    /// Run `f` with `hand` acting (`held`, `consume_held`, `swing_arm`).
    fn with_hand<T>(&mut self, hand: Hand, f: impl FnOnce(&mut Self) -> T) -> T {
        let prior = std::mem::replace(&mut self.hand, hand);
        let out = f(self);
        self.hand = prior;
        out
    }
    /// Inventory.add: returns the stack that did not fit.
    fn give(&mut self, mut stack: ItemStack) -> Option<ItemStack> {
        let max = self
            .res
            .items()
            .get(&stack.item)
            .map_or(64, |i| i.max_stack);
        // getSlotWithRemainingSpace: the selected slot, the off hand, then
        // the hotbar and the main inventory.
        let order: Vec<usize> = std::iter::once(self.selected)
            .chain(std::iter::once(shield::OFFHAND_SLOT))
            .chain((0..36).filter(|&i| i != self.selected))
            .collect();
        for &i in &order {
            let slot = if i == shield::OFFHAND_SLOT {
                &mut self.offhand
            } else {
                &mut self.inventory[i]
            };
            if let Some(slot) = slot
                && slot.same_item_same_components(&stack)
                && slot.count < max
            {
                let moved = (max - slot.count).min(stack.count);
                slot.count += moved;
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        for i in 0..36 {
            if self.inventory[i].is_none() {
                let moved = stack.count.min(max);
                self.inventory[i] = Some(ItemStack {
                    count: moved,
                    ..stack.clone()
                });
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }
    /// `TerrainParticle` at `pos`: the model's particle sprite and tint source
    /// 0's `colorAsTerrainParticle` (white when the block has none).
    /// `HoneyBlock.showSlideParticles` (entity event 53): five honey block
    /// particles at the entity.
    fn honey_slide_particles(&mut self, at: [f64; 3]) {
        use crate::hc::particles::{ParticleOptions, ParticleType};
        let Some(honey) = self
            .pack
            .assets
            .registry
            .default_state("minecraft:honey_block")
        else {
            return;
        };
        let block = [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ];
        if let Some((sprite, tint)) = self.particle_sprite(honey, block) {
            for _ in 0..5 {
                self.particles.add(
                    &ParticleOptions::block(ParticleType::Block, sprite.clone(), tint),
                    at,
                    [0.0; 3],
                );
            }
        }
    }
    /// ClientLevel.addDestroyBlockEffect: a burst per outline box.
    fn destroy_particles(&mut self, pos: [i32; 3], state: BlockStateId) {
        let Some((sprite, tint)) = self.particle_sprite(state, pos) else {
            return;
        };
        let boxes = self.particle_shape(pos, state);
        crate::hc::particles::destroy_positions(pos, &boxes, |p, v| {
            let particle = Particle::terrain(self.particles.random(), p, v, sprite.clone(), tint);
            self.particles.push(particle);
        });
    }
    /// ClientLevel.addBreakingParticles: one crumb on the face being mined.
    fn crack_particle(&mut self, pos: [i32; 3], face: [i32; 3], state: BlockStateId) {
        let Some((sprite, tint)) = self.particle_sprite(state, pos) else {
            return;
        };
        let boxes = self.particle_shape(pos, state);
        // Keep client effects separate from the authoritative world RNG.
        let r = self.particles.random();
        let Some(p) = crate::hc::particles::breaking_position(r, pos, face, &boxes) else {
            return;
        };
        let mut particle = Particle::terrain(r, p, [0.0; 3], sprite, tint);
        particle.set_power(0.2);
        particle.scale(0.6);
        self.particles.push(particle);
    }
    /// `Player.dropEquipment` for the player who died: unless keep_inventory
    /// is on, `destroyVanishingCursedItems`, then `Inventory.dropAll` from
    /// `at` (the eyes less 0.3): slots 0..36, armor feet to head (36..39),
    /// the off hand (40), each thrown in a random direction.
    fn drop_inventory(&mut self, at: [f64; 3]) {
        if self.world_data.rules.bool("keep_inventory") {
            return;
        }
        let mut stacks: Vec<ItemStack> =
            self.inventory.iter_mut().filter_map(Option::take).collect();
        stacks.extend(self.armor.iter_mut().rev().filter_map(Option::take));
        stacks.extend(self.offhand.take());
        stacks.retain(|s| s.enchantment_level("minecraft:vanishing_curse") == 0);
        self.apply_equipment();
        for stack in stacks {
            let r = &mut self.world.random;
            let strength = f64::from(r.next_float()) * 0.5;
            let angle = f64::from(r.next_float()) * std::f64::consts::TAU;
            let v = [-angle.sin() * strength, 0.2, angle.cos() * strength];
            self.spawn_item(stack, at, v, 40);
        }
    }
    fn spawn_item(
        &mut self,
        stack: ItemStack,
        position: [f64; 3],
        velocity: [f64; 3],
        pickup_delay: u32,
    ) {
        self.next_entity += 1;
        let p = Vec3::new(position[0], position[1], position[2]);
        // ItemEntity.bobOffs comes from the entity's own random, not the level's.
        let bob = self.particles.random().next_float() * std::f32::consts::PI * 2.0;
        self.dropped.push(ItemEntity {
            health: 5,
            fire_ticks: 0,
            thrower: None,
            target: None,
            id: self.next_entity,
            stack,
            position: p,
            previous: p,
            velocity: Vec3::new(velocity[0], velocity[1], velocity[2]),
            on_ground: false,
            age: 0,
            pickup_delay,
            bob_offset: bob,
            ticks: 0,
        });
    }
    /// `Block.getDrops` for a block a player breaks: the block's loot table
    /// with the player as `this`, drawing from its server random sequence
    /// (`LootContext.Builder.create`), else the level random.
    fn block_loot_at(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        held: Option<&ItemStack>,
    ) -> Vec<ItemStack> {
        let registry = self.pack.assets.registry;
        let dynamic = self.pending.dynamic(pos);
        match self.res.items().block_loot_sequence(registry, state) {
            Some(sequence) => {
                let random = self.rand_sequences.get(&sequence, self.level.seed);
                self.res
                    .items()
                    .block_drops_by_player(registry, state, held, &dynamic, random)
            }
            None => self.res.items().block_drops_by_player(
                registry,
                state,
                held,
                &dynamic,
                &mut self.world.random,
            ),
        }
    }
    /// Block.popResource
    fn pop_resource(&mut self, pos: [i32; 3], mut stack: ItemStack) {
        self.pending.attach(pos, &mut stack);
        // Block.popResource: Mth.nextDouble offsets from the level random; the
        // motion is `new ItemEntity(..)`'s own, from the item's random.
        let p = spill::pop_position(&mut self.world.random, pos);
        let v = dispenser::new_item_motion();
        self.spawn_item(stack, p, v, 10);
    }
    /// Player.drop: thrown from the eyes along the view direction.
    fn drop_held(&mut self, all: bool) {
        if self.eye_position().is_none() {
            return;
        }
        let spectator = self.spectator();
        let before = self.inventory[self.selected].clone();
        let Some(thrown) =
            crate::hc::items::take_held_drop(&mut self.inventory[self.selected], all, spectator)
        else {
            return;
        };
        // PlayerDropItemEvent: a plugin may keep it in the hand.
        if !self.plugin_drop_item(&thrown) {
            self.inventory[self.selected] = before;
            return;
        }
        self.player_drop(thrown);
    }
    /// `Player.drop`: the statistics and a thrown item entity in front of the player.
    pub(super) fn player_drop(&mut self, thrown: ItemStack) {
        let Some(eye) = self.eye_position() else {
            return;
        };
        // LivingEntity.drop: every drop swings the main hand, a menu's
        // THROW and a creative tab's drop included.
        self.swing_hand(Hand::Main, SwingAnimation::DEFAULT);
        // Player.drop: the drop statistics.
        self.stats.add("dropped", &thrown.item, thrown.count as i32);
        self.stats.custom("drop", 1);
        self.throw_from_eyes(thrown, eye, true);
    }
    /// `Player.createItemStackToDrop`: an item entity thrown from the eyes
    /// along the view direction. `thrown_from_hand` (`drop(stack, true)`, the
    /// drop key) names the player as the thrower; `drop(stack, false)`, for
    /// what no longer fits in the inventory, does not.
    fn throw_from_eyes(&mut self, thrown: ItemStack, eye: [f64; 3], thrown_from_hand: bool) {
        let (yaw, pitch) = (
            f64::from(self.yaw).to_radians(),
            f64::from(self.pitch).to_radians(),
        );
        let r = &mut self.world.random;
        let spread = f64::from(r.next_float()) * std::f64::consts::TAU;
        let strength = 0.02 * f64::from(r.next_float());
        let v = [
            -yaw.sin() * pitch.cos() * 0.3 + spread.cos() * strength,
            -pitch.sin() * 0.3 + 0.1 + f64::from(r.next_float() - r.next_float()) * 0.1,
            yaw.cos() * pitch.cos() * 0.3 + spread.sin() * strength,
        ];
        self.spawn_item(thrown, [eye[0], eye[1] - 0.3, eye[2]], v, 40);
        if thrown_from_hand {
            let uuid = self.acting_player_uuid();
            self.dropped.last_mut().unwrap().thrower = Some(uuid);
        }
    }
    /// The UUID of the player acting: the guest whose turn it is, else the host.
    pub(super) fn acting_player_uuid(&mut self) -> [i32; 4] {
        match self.lan_acting_uuid {
            Some(guest) => crate::hc::lan::playerdata::uuid_ints(guest),
            None => self.level.ensure_player_uuid(),
        }
    }
    fn tick_items(&mut self) {
        let mut items = std::mem::take(&mut self.dropped);
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        let mut removed = vec![false; items.len()];
        let mut cells = crate::hc::entities::ItemCells::new(&items);
        let catalog = self.res.items();
        let max_stack =
            |stack: &ItemStack| catalog.get(&stack.item).map_or(64, |i| i.max_stack).max(1);
        // A LAN or server guest only shows the host's items.
        let merging = true;
        for i in 0..items.len() {
            if removed[i] {
                continue;
            }
            let item = &mut items[i];
            if !ticking(
                item.position.x.floor() as i32,
                item.position.z.floor() as i32,
            ) {
                continue;
            }
            // ItemEntity.tick's noPhysics (servers only): out of a block
            // placed on it, towards the closest open side (#1252).
            let no_physics = merging && item.inside_block(&self.world, self.collision);
            if no_physics {
                let speed = self.world.random.next_float() * 0.2 + 0.1;
                item.move_towards_closest_space(&self.world, self.res.facts(), speed);
            }
            let alive = item.tick(
                &self.world,
                self.res.facts(),
                self.collision,
                no_physics,
                &self.res.motion().fluids,
            );
            // Entity.applyEffectsFromBlocks: bubble columns lift or drag the item.
            crate::hc::motion::bubble_column_push(
                &self.world,
                self.pack.assets.registry,
                self.res.facts(),
                self.collision,
                item.bounds(),
                &mut item.velocity,
            );
            cells.moved(i, item.previous, item.position);
            // ItemEntity.tick: merge every 2 ticks while moving between
            // blocks, every 40 otherwise, before the lifetime check.
            let block = |v: Vec3| (v.x.floor(), v.y.floor(), v.z.floor());
            let rate = if block(item.previous) == block(item.position) {
                40
            } else {
                2
            };
            if merging
                && (alive || item.age == 6000 && item.position.y > -128.0)
                && item.ticks.is_multiple_of(rate)
            {
                crate::hc::entities::merge_with_neighbours(
                    &mut items,
                    &mut removed,
                    &mut cells,
                    i,
                    &max_stack,
                );
            }
            let item = &mut items[i];
            if !alive || removed[i] {
                removed[i] = true;
                cells.remove(i, item.position);
                continue;
            }
            let environment = item.environment_tick(
                &self.world,
                self.res.facts(),
                self.pack.assets.registry,
                self.res.items(),
            );
            // shortcut: age is the fresh-drop clock; use entity ticks for saved/frozen-age phase parity.
            if environment.should_play_lava_hurt_sound(item.age) {
                let pitch = 2.0 + self.world.random.next_float() * 0.4;
                self.sounds.play(
                    "minecraft:entity.generic.burn",
                    Some([item.position.x, item.position.y, item.position.z]),
                    0.4,
                    pitch,
                );
            }
            if !environment.alive {
                removed[i] = true;
                cells.remove(i, item.position);
            }
        }
        let mut removed = removed.into_iter();
        items.retain(|_| !removed.next().unwrap_or(false));
        let Some(p) = self.player.as_ref() else {
            self.dropped = items;
            return;
        };
        if p.health.dead_or_dying() || self.spectator() {
            self.dropped = items;
            return;
        }
        let w = f64::from(p.dimensions.width) / 2.0;
        let (min, max) = (
            [
                p.position.x - w - 1.0,
                p.position.y - 0.5,
                p.position.z - w - 1.0,
            ],
            [
                p.position.x + w + 1.0,
                p.position.y + f64::from(p.dimensions.height) + 0.5,
                p.position.z + w + 1.0,
            ],
        );
        let feet = [p.position.x, p.position.y, p.position.z];
        let own = self.level.ensure_player_uuid();
        let mut kept = Vec::with_capacity(items.len());
        for mut item in items {
            let b = item.bounds();
            let touching = b.min_position().x < max[0]
                && b.max_position().x > min[0]
                && b.min_position().y < max[1]
                && b.max_position().y > min[1]
                && b.min_position().z < max[2]
                && b.max_position().z > min[2];
            // ItemEntity.playerTouch: no pickup delay, and the target (if any) is this player.
            if touching && item.pickup_delay == 0 && item.target.is_none_or(|t| t == own) {
                let before = item.stack.count;
                let kind = item.stack.item.clone();
                match self.give(item.stack.clone()) {
                    None => {
                        self.stats.add("picked_up", &kind, before as i32);
                        let pitch = ((self.world.random.next_float()
                            - self.world.random.next_float())
                            * 0.7
                            + 1.0)
                            * 2.0;
                        self.sounds
                            .play("minecraft:entity.item.pickup", Some(feet), 0.2, pitch);
                        continue;
                    }
                    Some(rest) => {
                        if rest.count < before {
                            self.stats
                                .add("picked_up", &kind, (before - rest.count) as i32);
                            // handleTakeItemEntity: the random pitch of a full pickup.
                            let pitch = ((self.world.random.next_float()
                                - self.world.random.next_float())
                                * 0.7
                                + 1.0)
                                * 2.0;
                            self.sounds.play(
                                "minecraft:entity.item.pickup",
                                Some(feet),
                                0.2,
                                pitch,
                            );
                        }
                        item.stack = rest;
                    }
                }
            }
            kept.push(item);
        }
        self.dropped = kept;
    }
    /// Run vanilla block behaviour against the world through `LevelAccess`.
    fn with_level<R>(
        &mut self,
        f: impl FnOnce(&Behaviours, &mut GameLevel<'_>, &mut Ticks) -> R,
    ) -> R {
        let darken = self.sky_darken();
        let (rain, thunder) = self.world.weather.levels(1.0);
        // Spectators ignore block triggers (Entity.isIgnoringBlockTriggers).
        let spectator = self.spectator();
        let player_box = self.player.as_ref().filter(|_| !spectator).map(|p| {
            let w = f64::from(p.dimensions.width) / 2.0;
            (
                [p.position.x - w, p.position.y, p.position.z - w],
                [
                    p.position.x + w,
                    p.position.y + f64::from(p.dimensions.height),
                    p.position.z + w,
                ],
            )
        });
        let player_collision = self.player.as_ref().filter(|_| !spectator).map(|p| {
            let main_hand = self.inventory.get(self.selected).and_then(Option::as_ref);
            PlayerCollision {
                y: p.position.y,
                descending: p.velocity.y < 0.0,
                fall_distance: p.fall_distance,
                main_hand_item: main_hand.map_or_else(
                    || Arc::from("minecraft:air"),
                    |stack| Arc::clone(&stack.item),
                ),
                main_hand_count: main_hand.map_or(0, |stack| stack.count as i32),
                leather_boots: self.armor[3]
                    .as_ref()
                    .is_some_and(|stack| stack.item.as_ref() == "minecraft:leather_boots"),
            }
        });
        let cart_boxes = self
            .carts
            .iter()
            .map(|c| {
                let p = c.position;
                (
                    [p.x - 0.49, p.y, p.z - 0.49],
                    [p.x + 0.49, p.y + 0.7, p.z + 0.49],
                )
            })
            .collect();
        let arrow_boxes = self
            .creatures
            .projectiles
            .iter()
            .filter(|p| p.arrow.is_some())
            .map(|p| p.position)
            .chain(self.tridents.iter().map(|t| t.position))
            .map(|at| {
                let b = crate::hc::arrows::arrow_bounds(at);
                (b.min_position().components(), b.max_position().components())
            })
            .collect();
        let guest_boxes = self.lan_block_trigger_boxes();
        let dimension = match self.dim {
            Dim::Overworld => LevelDimension::Overworld,
            Dim::Nether => LevelDimension::Nether,
            Dim::End => LevelDimension::End,
        };
        let rules = self.level_rules();
        let fire_players: Vec<[f64; 3]> = self
            .player
            .iter()
            .filter(|_| !spectator)
            .map(|p| [p.position.x, p.position.y, p.position.z])
            .chain(self.lan.iter().flat_map(|h| {
                h.hub
                    .guests
                    .iter()
                    .filter(|g| h.hub.here(g) && !g.spectator())
                    .map(|g| g.position)
            }))
            .collect();
        let blockers = Blockers {
            mobs: &self.mobs,
            armor_stands: &self.armor_stands,
            boats: &self.boats,
            crystals: &self.crystals,
            tnts: &self.tnts,
            falling: &self.falling,
        };
        let change_events = self.plugin_change_events();
        let mut level = GameLevel {
            cart_boxes,
            item_frames: &self.item_frames,
            carts: &self.carts,
            arrow_boxes,
            blockers,
            dropped: &self.dropped,
            orbs: &self.orbs,
            projectiles: &self.creatures.projectiles,
            world: &mut self.world,
            facts: self.res.facts(),
            collision: self.collision,
            events: &mut self.level_events,
            dimension,
            darken,
            environment: match dimension {
                LevelDimension::Overworld => self.res.environment(),
                LevelDimension::Nether => self.res.nether_environment(),
                LevelDimension::End => self.res.end_environment(),
            },
            raining: rain > 0.2,
            thundering: thunder > 0.9,
            player_box,
            guest_boxes,
            player_collision,
            biomes: self.res.biomes(),
            seed: self.level.seed,
            behaviours: self.res.behaviours(),
            items: self.res.items(),
            recipes: self.res.recipes(),
            // A client rolls no loot (LootTable.getRandomItems needs a
            // ServerLevel): it doesn't load the tables on its game thread.
            loot: { crate::hc::structure_blocks::loot_data(&self.options.resources) },
            registry: self.pack.assets.registry,
            rules,
            difficulty: i32::from(self.world_data.difficulty),
            fire_players,
            rand_sequences: &mut self.rand_sequences,
            pending: &mut self.pending,
            spill: &mut self.spill,
            tickets: self.tickets.levels(),
            random_tick_sections: &mut self.random_tick_sections,
            lid_openers: &self.lid_openers,
            client_side: false,
            change_events,
        };
        let result = f(self.res.behaviours(), &mut level, &mut self.block_ticks);
        self.flush_spill();
        result
    }
    /// Plugins hear of blocks growing, spreading, forming and fading.
    fn plugin_change_events(&self) -> bool {
        false
    }
    /// Plugins hear of pistons moving (`BlockPistonExtendEvent`,
    /// `BlockPistonRetractEvent`).
    fn plugin_piston_events(&self) -> bool {
        false
    }
    /// `ServerLevel.runBlockEvents` with the plugins asked before each piston
    /// moves, then `tickBlockEntities`.
    fn run_block_events_asking(&mut self) -> Result<()> {
        loop {
            let next =
                self.with_level(|b, level, ticks| b.run_block_events_to_piston(level, ticks));
            if !self.level_events.is_empty() {
                self.process_level_events()?;
            }
            let Some(mv) = next else {
                break;
            };
            let allowed = {
                let _ = mv;
                true
            };
            self.with_level(|b, level, ticks| b.run_piston_move(level, ticks, allowed));
        }
        self.with_level(|b, level, ticks| b.tick_block_entities(level, ticks));
        Ok(())
    }
    /// Level.setBlock(pos, state, UPDATE_ALL) with neighbour/shape updates.
    fn level_set(&mut self, pos: [i32; 3], state: BlockStateId) -> Result<()> {
        self.with_level(|b, level, ticks| {
            b.set_block(
                level,
                ticks,
                BlockPos::new(pos[0], pos[1], pos[2]),
                state,
                level_flags::UPDATE_ALL,
            );
        });
        self.process_level_events()
    }
    fn attack_block(&mut self, pos: [i32; 3]) -> Result<()> {
        let note = self.with_level(|b, level, ticks| {
            b.attack_block(level, ticks, BlockPos::new(pos[0], pos[1], pos[2]))
        });
        // NoteBlock.attack awards the statistic.
        if note {
            self.stats.custom("play_noteblock", 1);
        }
        self.process_level_events()
    }
    /// Track which chunks are ticking (Ticks needs chunk load/unload).
    fn sync_tick_chunks(&mut self) {
        if !self.tick_chunks.synchronize(&self.world.chunks) {
            return;
        }
        // Move only the added keys out so registration can borrow all of Game.
        let added = self.tick_chunks.take_added();
        for &key in &added {
            self.register_chunk_ticks(key);
        }
        for &key in self.tick_chunks.removed() {
            self.random_tick_sections.remove(&key);
            self.portals.sections.remove(&key);
            self.block_ticks
                .remove_chunk(horizon_core::position::ChunkPos::new(key.0, key.1));
        }
        self.tick_chunks.recycle_added(added);
    }
    /// `DistanceManager`'s tickets of the level in place, from every player in
    /// it (the local player, LAN guests, a server's players) and its forced
    /// chunks. A game with neither a server nor a player (the headless
    /// fixtures) ticks everywhere.
    fn update_chunk_tickets(&mut self) {
        let mut players: Vec<(i32, i32)> = self.lan.as_ref().map_or_else(Vec::new, |h| {
            h.hub
                .guests
                .iter()
                .filter(|g| h.hub.here(g))
                .map(|g| g.chunk())
                .collect()
        });
        if let Some(p) = &self.player {
            players.push((
                (p.position.x.floor() as i32) >> 4,
                (p.position.z.floor() as i32) >> 4,
            ));
        }
        let sources = Some(crate::hc::ticket_levels::Sources {
            players,
            forced: self.streaming.forced.iter().copied().collect(),
            view_distance: self.settings.render_distance.clamp(2, 32),
            simulation_distance: self.settings.simulation_distance.max(2),
        });
        self.tickets.update(sources);
    }
    /// `ServerLevel.startTickingChunk`: a loaded chunk's saved ticks are
    /// scheduled from the game time at which it becomes a ticking chunk
    /// (block ticking, with its neighbours loaded). Chunks become ticking
    /// between ticks in vanilla: before this tick's clock advanced.
    fn start_ticking_chunks(&mut self) {
        let levels = self.tickets.levels();
        let world = &self.world;
        let started = self
            .tick_chunks
            .start_ticking(|(x, z)| levels.loaded_ticking(x, z) && ticking_chunk(world, x, z));
        let time = self.world.game_time - 1;
        for (x, z) in started {
            let pos = horizon_core::position::ChunkPos::new(x, z);
            self.block_ticks.blocks.unpack_chunk(pos, time);
            self.block_ticks.fluids.unpack_chunk(pos, time);
        }
    }
    /// ServerLevel.tick block phases: scheduled ticks, random ticks in the
    /// chunks the tickets simulate, block events and moving pistons.
    fn tick_blocks(&mut self) -> Result<()> {
        self.sync_tick_chunks();
        self.start_ticking_chunks();
        let levels = self.tickets.levels();
        // Keep the world map's chunk order, including empty chunks
        // (tick_chunk_random still draws for precipitation in those).
        let ticking: Vec<_> = self
            .world
            .chunks
            .keys()
            .copied()
            .filter(|&(x, z)| levels.random_ticking(x, z) && ticking_chunk(&self.world, x, z))
            .collect();
        // Entity.checkInsideBlocks for the plates, tripwires, detector rails and
        // buttons under the players (the host and the LAN guests), the minecarts
        // and the arrows (the mobs and other entities are added in `with_level`
        // below).
        let mut inside = Vec::new();
        let guests = self.lan_block_trigger_boxes();
        let boxes = self
            .player
            .iter()
            .filter(|_| !self.spectator())
            .map(|p| {
                let w = f64::from(p.dimensions.width) / 2.0;
                let h = f64::from(p.dimensions.height);
                (
                    [p.position.x - w, p.position.y, p.position.z - w],
                    [p.position.x + w, p.position.y + h, p.position.z + w],
                )
            })
            .chain(self.carts.iter().map(|c| {
                let p = c.position;
                (
                    [p.x - 0.49, p.y, p.z - 0.49],
                    [p.x + 0.49, p.y + 0.7, p.z + 0.49],
                )
            }))
            .chain(guests);
        for (lo, hi) in boxes {
            for x in (lo[0] + 1e-7).floor() as i32..=(hi[0] - 1e-7).floor() as i32 {
                for y in (lo[1] + 1e-7).floor() as i32..=(hi[1] - 1e-7).floor() as i32 {
                    for z in (lo[2] + 1e-7).floor() as i32..=(hi[2] - 1e-7).floor() as i32 {
                        inside.push(BlockPos::new(x, y, z));
                    }
                }
            }
        }
        // AbstractArrow.applyEffectsFromBlocks: the blocks every arrow passed
        // through this tick (a wooden button is pressed by one inside it), and
        // the ones a thrown trident is in.
        for p in self
            .creatures
            .projectiles
            .iter()
            .filter(|p| p.arrow.is_some())
        {
            inside.extend(crate::hc::arrows::blocks_inside(p.previous, p.position));
        }
        for t in &self.tridents {
            inside.extend(crate::hc::arrows::blocks_inside(t.position, t.position));
        }
        // BigDripleafBlock.entityInside also asks whether the player stands on
        // the ground and how high its feet are: the leaves its box's feet
        // layer is inside (the host and the LAN guests).
        let mut standing = Vec::new();
        let feet = self
            .player
            .iter()
            .filter(|_| !self.spectator())
            .map(|p| {
                (
                    [p.position.x, p.position.y, p.position.z],
                    p.ground.on_ground,
                    f64::from(p.dimensions.width) / 2.0,
                )
            })
            .chain(
                self.lan_guest_feet()
                    .into_iter()
                    .map(|(at, on_ground)| (at, on_ground, 0.3)),
            );
        for (at, on_ground, w) in feet {
            let y = (at[1] + 1e-7).floor() as i32;
            for x in (at[0] - w + 1e-7).floor() as i32..=(at[0] + w - 1e-7).floor() as i32 {
                for z in (at[2] - w + 1e-7).floor() as i32..=(at[2] + w - 1e-7).floor() as i32 {
                    standing.push((BlockPos::new(x, y, z), on_ground, at[1]));
                }
            }
        }
        self.with_level(|b, level, ticks| {
            // The mobs and the other entities (items, orbs, projectiles...) too.
            let mut touched: Vec<BlockPos> = Vec::new();
            for bounds in level.mob_trigger_boxes().chain(level.other_trigger_boxes()) {
                let (lo, hi) = (bounds.min_position(), bounds.max_position());
                for x in (lo.x + 1e-7).floor() as i32..=(hi.x - 1e-7).floor() as i32 {
                    for y in (lo.y + 1e-7).floor() as i32..=(hi.y - 1e-7).floor() as i32 {
                        for z in (lo.z + 1e-7).floor() as i32..=(hi.z - 1e-7).floor() as i32 {
                            touched.push(BlockPos::new(x, y, z));
                        }
                    }
                }
            }
            touched.sort_unstable_by_key(|p| (p.x, p.y, p.z));
            touched.dedup();
            for pos in inside.into_iter().chain(touched) {
                b.entity_inside(level, ticks, pos);
            }
            for (pos, on_ground, feet_y) in standing {
                b.entity_on_big_dripleaf(level, ticks, pos, on_ground, feet_y);
            }
        });
        self.process_level_events()?;
        self.tick_scheduled()?;
        // Plugins hear of each piston before it moves: the queue runs a step
        // at a time out here.
        let asking = self.plugin_piston_events();
        self.with_level(|b, level, ticks| {
            for (x, z) in ticking {
                // The sections are read again only after a block changed.
                let mut read = None;
                let mut sections = Default::default();
                let pos = horizon_core::position::ChunkPos::new(x, z);
                b.tick_chunk_random(level, ticks, pos, |level, from| {
                    if read != Some(level.world.revision) {
                        read = Some(level.world.revision);
                        sections = level
                            .world
                            .chunks
                            .get(&(x, z))
                            .map_or_else(Default::default, |chunk| {
                                level.random_tick_sections.get((x, z), chunk, level.facts)
                            });
                    }
                    sections.from(from)
                });
            }
            if !asking {
                b.run_block_events(level, ticks);
                b.tick_block_entities(level, ticks);
            }
        });
        if asking {
            self.process_level_events()?;
            self.run_block_events_asking()?;
        }
        // `PistonMovingBlockEntity.tick` pushes what its block just ran into,
        // with the progress it is about to take.
        let pushes = std::mem::take(&mut self.block_ticks.piston_pushes);
        self.push_piston_entities(pushes);
        self.process_level_events()?;
        self.sync_moving_pistons();
        Ok(())
    }
    /// Copies the moving-piston block entities into the world, where the
    /// collision queries and the renderer read them.
    fn sync_moving_pistons(&mut self) {
        let previous = std::mem::take(&mut self.world.moving_pistons);
        let current: Vec<_> = self
            .block_ticks
            .moving_pistons
            .iter()
            .map(|(p, piston)| ([p.x, p.y, p.z], piston.shape_state()))
            .collect();
        self.world.moving_piston_fallback = previous
            .into_iter()
            .filter(|(pos, _)| !current.iter().any(|(next, _)| next == pos))
            .collect();
        self.world.moving_pistons = current;
    }
    /// `ServerLevel.tick`'s scheduled block then fluid ticks, one at a time:
    /// the side effects of each (a shrieker summoning a warden, drops, game
    /// events) happen before the next tick runs, as they do inline in vanilla.
    pub(super) fn tick_scheduled(&mut self) -> Result<()> {
        for fluids in [false, true] {
            if !self.with_level(|b, level, ticks| b.scheduled_begin(level, ticks, fluids)) {
                continue;
            }
            // BlockFromToEvent: plugins hear where each fluid tick flows first.
            loop {
                let more =
                    self.with_level(|b, level, ticks| b.scheduled_step(level, ticks, fluids));
                if !self.level_events.is_empty() {
                    self.process_level_events()?;
                }
                if !more {
                    break;
                }
            }
            self.with_level(|b, _, ticks| b.scheduled_finish(ticks, fluids));
        }
        Ok(())
    }
    /// Side effects reported by the behaviour library, in vanilla order.
    fn process_level_events(&mut self) -> Result<()> {
        let events = std::mem::take(&mut self.level_events);
        self.lan_record_events(&events);
        dispense_fixture::record_events(&events);
        for event in events {
            match event {
                LevelEvent::LevelEvent {
                    id: 2001,
                    pos,
                    data,
                } => {
                    // LevelEventHandler: destroy particles and the block's
                    // break sound (none for air).
                    let state = BlockStateId(data as u32);
                    let p = [pos.x, pos.y, pos.z];
                    self.destroy_particles(p, state);
                    if self.res.facts().is_air(state) {
                        continue;
                    }
                    let sounds = self.res.facts().get(state).sounds.clone();
                    let c = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    self.sounds.play(
                        &sounds.break_sound,
                        Some(c),
                        (sounds.volume + 1.0) / 2.0,
                        sounds.pitch * 0.8,
                    );
                }
                LevelEvent::LevelEvent {
                    id: 3006,
                    pos,
                    data,
                } => self.sculk_charge_event(pos, data),
                LevelEvent::LevelEvent { id: 3007, pos, .. } => self.shriek_event(pos),
                // LevelEventHandler 1010 and 1011: the parrots nearby dance or stop.
                LevelEvent::LevelEvent {
                    id: id @ (1010 | 1011),
                    pos,
                    ..
                } => self.notify_record_playing([pos.x, pos.y, pos.z], id == 1010),
                // LevelEventHandler 2019 and 2020: the crack on a face of the
                // block being mined, with its hit sound for 2020.
                LevelEvent::LevelEvent {
                    id: id @ (2019 | 2020),
                    pos,
                    data,
                } => {
                    let face = horizon_core::geometry::Direction::ALL[data.clamp(0, 5) as usize];
                    self.breaking_block_effects([pos.x, pos.y, pos.z], face.normal(), id == 2020);
                }
                LevelEvent::LevelEvent {
                    id: 3008,
                    pos,
                    data,
                } => self.brush_complete_event([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent { id: 2004, pos, .. } => {
                    self.spawner_burst([pos.x, pos.y, pos.z])
                }
                // LevelEventHandler 1505: the growth particles, then the sound.
                LevelEvent::LevelEvent {
                    id: 1505,
                    pos,
                    data,
                } => {
                    self.growth_particles(pos, data);
                    let at = [pos.x, pos.y, pos.z].map(|v| f64::from(v) + 0.5);
                    self.sounds
                        .play("minecraft:item.bone_meal.use", Some(at), 1.0, 1.0);
                }
                LevelEvent::LevelEvent {
                    id: 2015,
                    pos,
                    data,
                } => self.dragon_egg_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2016,
                    pos,
                    data,
                } => self.shulker_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2017,
                    pos,
                    data,
                } => self.consume_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent {
                    id: 2018,
                    pos,
                    data,
                } => self.enderman_teleport_particles([pos.x, pos.y, pos.z], data),
                LevelEvent::LevelEvent { id: 2003, pos, .. } => {
                    self.eye_break_particles([pos.x, pos.y, pos.z]);
                }
                LevelEvent::LevelEvent {
                    id: 2013,
                    pos,
                    data,
                } => self.smash_dust(pos, data),
                // LevelEventHandler 2002 and 2007: a potion's splash.
                LevelEvent::LevelEvent {
                    id: 2002,
                    pos,
                    data,
                } => self.potion_splash_particles(pos, data, false),
                LevelEvent::LevelEvent {
                    id: 2007,
                    pos,
                    data,
                } => self.potion_splash_particles(pos, data, true),
                LevelEvent::LevelEvent {
                    id: id @ 3002..=3005,
                    pos,
                    data,
                } => self.block_particle_event(id, pos, data),
                LevelEvent::LevelEvent {
                    id: 1500,
                    pos,
                    data,
                } => self.composter_fill_event(pos, data == 1),
                // LevelEventHandler 1504: the stalactite's drip particle.
                LevelEvent::LevelEvent { id: 1504, pos, .. } => self.dripstone_drip_particle(pos),
                // LevelEventHandler 2006: the dragon fireball's burst.
                LevelEvent::LevelEvent {
                    id: 2006,
                    pos,
                    data,
                } => self.dragon_fireball_burst(pos, data),
                // LevelEventHandler 2009: a wet sponge drying.
                LevelEvent::LevelEvent { id: 2009, pos, .. } => {
                    self.puffs_above_block(pos, crate::hc::particles::ParticleType::Cloud)
                }
                // LevelEventHandler 2012: a turtle egg laid.
                LevelEvent::LevelEvent {
                    id: 2012,
                    pos,
                    data,
                } => self.turtle_egg_particles(pos, data),
                // LevelEventHandler 2014: a straw bed destroyed, with no sound.
                LevelEvent::LevelEvent {
                    id: 2014,
                    pos,
                    data,
                } => self.destroy_particles([pos.x, pos.y, pos.z], BlockStateId(data as u32)),
                // LevelEventHandler 3009: a sniffer egg cracking.
                LevelEvent::LevelEvent { id: 3009, pos, .. } => {
                    self.sniffer_egg_crack_particles(pos)
                }
                // The trial spawner's and vault's flames and smoke: the clients draw them.
                LevelEvent::LevelEvent {
                    id: 3011..=3021, ..
                } => {}
                LevelEvent::LevelEvent { id, pos, .. } => {
                    let c = Some([
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ]);
                    let sound = match id {
                        1501 => Some(("minecraft:block.lava.extinguish", 0.5)),
                        1009 => Some(("minecraft:block.fire.extinguish", 0.5)),
                        1502 => Some(("minecraft:block.redstone_torch.burnout", 0.5)),
                        1033 => Some(("minecraft:block.chorus_flower.grow", 1.0)),
                        1034 => Some(("minecraft:block.chorus_flower.death", 1.0)),
                        1046 => Some((
                            "minecraft:block.pointed_dripstone.drip_lava_into_cauldron",
                            2.0,
                        )),
                        1047 => Some((
                            "minecraft:block.pointed_dripstone.drip_water_into_cauldron",
                            2.0,
                        )),
                        // LevelEvent.SOUND_ZOMBIE_WOODEN_DOOR (attack, break).
                        1019 => Some(("minecraft:entity.zombie.attack_wooden_door", 2.0)),
                        1021 => Some(("minecraft:entity.zombie.break_wooden_door", 2.0)),
                        1030 => Some(("minecraft:block.anvil.use", 1.0)),
                        1029 => Some(("minecraft:block.anvil.destroy", 1.0)),
                        1031 => Some(("minecraft:block.anvil.land", 0.3)),
                        1042 => Some(("minecraft:block.grindstone.use", 1.0)),
                        1044 => Some(("minecraft:block.smithing_table.use", 1.0)),
                        // LevelEvent.SOUND_PAGE_TURN (a lectern's page).
                        1043 => Some(("minecraft:item.book.page_turn", 1.0)),
                        // LevelEvent.SOUND_SPLASH_POTION and its instant twin.
                        1053 | 1054 => Some(("minecraft:entity.splash_potion.break", 1.0)),
                        _ => None,
                    };
                    if let Some((name, volume)) = sound {
                        let pitch = match id {
                            1009 => {
                                2.6 + (self.world.random.next_float()
                                    - self.world.random.next_float())
                                    * 0.8
                            }
                            // LevelEventHandler draws these from the client
                            // level's random, before 1501's and 1502's smoke.
                            1501 | 1502 => {
                                let r = self.particles.level_random();
                                2.6 + (r.next_float() - r.next_float()) * 0.8
                            }
                            1046 | 1047 => self.particles.level_random().next_float() * 0.1 + 0.9,
                            1029 | 1030 | 1042 | 1043 | 1044 | 1053 | 1054 => {
                                self.world.random.next_float() * 0.1 + 0.9
                            }
                            1019 | 1021 => {
                                (self.world.random.next_float() - self.world.random.next_float())
                                    * 0.2
                                    + 1.0
                            }
                            _ => 1.0,
                        };
                        if matches!(id, 1019 | 1021 | 1029 | 1030 | 1053 | 1054) {
                            // Clients play the anvil sound from its level event;
                            // capturing it also sends a duplicate sound packet.
                            self.sounds.play_unshared(name, c, volume, pitch);
                        } else {
                            self.sounds.play(name, c, volume, pitch);
                        }
                    }
                    // LevelEventHandler 1501 and 1502 draw their smoke after the sound.
                    match id {
                        1501 => self
                            .puffs_above_block(pos, crate::hc::particles::ParticleType::LargeSmoke),
                        1502 => self.torch_burnout_smoke(pos),
                        _ => {}
                    }
                }
                LevelEvent::Sound {
                    pos,
                    sound,
                    volume,
                    pitch,
                } => {
                    let c = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    let name = if sound.contains(':') {
                        sound.to_string()
                    } else {
                        format!("minecraft:{sound}")
                    };
                    self.sounds.play(&name, Some(c), volume, pitch);
                }
                // Handled as the behaviour emits it (GameLevel::emit).
                LevelEvent::DropResources { pos, state } => {
                    self.with_level(|_, level, _| level.drop_resources_now(pos, state));
                }
                LevelEvent::FallingBlock { pos, state } => {
                    let p = Vec3::new(
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y),
                        f64::from(pos.z) + 0.5,
                    );
                    self.falling.push(FallingBlock {
                        id: 0,
                        state,
                        start_pos: pos,
                        position: p,
                        previous: p,
                        velocity: Vec3::ZERO,
                        age: 0,
                        no_gravity: false,
                        drop_item: true,
                        fall_distance: 0.0,
                        stuck: Vec3::ZERO,
                        damage: crate::hc::falling_block::Damage::falling(
                            &self.res.facts().get(state).name,
                        ),
                        saved: None,
                    });
                }
                LevelEvent::Player { pos, effect, .. } => match effect {
                    PlayerEffect::OpenMenu => self.open_menu([pos.x, pos.y, pos.z]),
                    PlayerEffect::Sleep if self.beds_destroy_on_use() => {
                        self.destroy_bed_on_use([pos.x, pos.y, pos.z])?
                    }
                    PlayerEffect::Sleep => self.try_sleep([pos.x, pos.y, pos.z]),
                    PlayerEffect::GiveItem(item) => {
                        if let Some(rest) = self.give(ItemStack {
                            item,
                            count: 1,
                            damage: 0,
                            extra: None,
                        }) {
                            self.pop_resource([pos.x, pos.y, pos.z], rest);
                        }
                    }
                    // Popped as the behaviour emits it (GameLevel::emit).
                    PlayerEffect::DropItem(_) => {}
                    PlayerEffect::EatCake => {
                        // CakeBlock.eat
                        self.stats.custom("eat_cake_slice", 1);
                        if let Some(p) = self.player.as_mut() {
                            let food = &mut p.health.food;
                            food.food = (food.food + 2).min(20);
                            food.saturation = (food.saturation + 0.4).min(food.food as f32);
                        }
                    }
                    PlayerEffect::RespawnAnchor => {
                        self.respawn_anchor_used([pos.x, pos.y, pos.z])?
                    }
                    PlayerEffect::InteractLoot(_) => {}
                },
                LevelEvent::PrimeTnt { pos, rot } => self.add_primed_tnt(pos, rot, 80),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "dispense",
                } => self.dispense_from(pos, state)?,
                LevelEvent::BlockEvent {
                    pos, kind: "golem", ..
                } => self.check_golem_spawn([pos.x, pos.y, pos.z])?,
                LevelEvent::BlockEvent {
                    pos,
                    kind: "command_block",
                    ..
                } => self.command_block_tick(pos),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "brushable",
                    ..
                } => self.brushable_tick([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos, kind: "hatch", ..
                } => {
                    // FrogspawnBlock.hatchFrogspawn: 2-5 tadpoles.
                    let at = [
                        f64::from(pos.x) + 0.5,
                        f64::from(pos.y) + 0.5,
                        f64::from(pos.z) + 0.5,
                    ];
                    self.sounds
                        .play("minecraft:block.frogspawn.hatch", Some(at), 1.0, 1.0);
                    let count = 2 + self.world.random.next_int_bound(4).unwrap_or(0);
                    for _ in 0..count {
                        // getRandomTadpolePositionOffset: 0.2..0.8 inside the block.
                        let r = &mut self.world.random;
                        let off = |r: &mut horizon_core::random::LegacyRandom| {
                            f64::from(r.next_float()) * 0.6 + 0.2
                        };
                        let (dx, dz) = (off(r), off(r));
                        let spawn = [
                            f64::from(pos.x) + dx,
                            f64::from(pos.y) - 0.5,
                            f64::from(pos.z) + dz,
                        ];
                        let _ = self.spawn_mob("minecraft:tadpole", spawn, true);
                    }
                }
                LevelEvent::BlockEvent {
                    pos,
                    kind: "hatch_sniffer",
                    ..
                } => self.hatch_sniffer([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "hatch_turtles",
                } => self.hatch_turtles([pos.x, pos.y, pos.z], state),
                LevelEvent::BlockEvent {
                    pos,
                    kind: "portal_spawn",
                    ..
                } => self.portal_spawn([pos.x, pos.y, pos.z]),
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "spawn_ghastling",
                } => self.spawn_ghastling([pos.x, pos.y, pos.z], state),
                LevelEvent::GameEvent { pos, event, state } => {
                    self.sculk_level_event(pos, event, state)
                }
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "shrieker_respond",
                } => self.shrieker_block_event(pos, state),
                // LeavesDecay, BlockBurn, Form and FadeEvent before the change.
                LevelEvent::Pending(change) => {
                    let _ = change;
                }
                // BlockGrow, Spread, Form and FadeEvent.
                LevelEvent::Changed { cause, changes } => {
                    let _ = (cause, changes);
                }
                LevelEvent::BlockEntityEvent { pos, a, b, .. } => {
                    self.block_entity_event([pos.x, pos.y, pos.z], a, b)
                }
                // NoteBlock.triggerEvent: a NOTE particle above, coloured by
                // the note (`note / 24.0` in the x speed).
                LevelEvent::BlockEvent {
                    pos,
                    state,
                    kind: "note_particle",
                } => {
                    use crate::hc::particles::{ParticleOptions, ParticleType};
                    let note = self
                        .pack
                        .assets
                        .registry
                        .state(state)
                        .and_then(|s| s.properties.get("note")?.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    self.particles.add(
                        &ParticleOptions::simple(ParticleType::Note),
                        [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 1.2,
                            f64::from(pos.z) + 0.5,
                        ],
                        [note / 24.0, 0.0, 0.0],
                    );
                }
                LevelEvent::GrowTree { .. }
                | LevelEvent::PlaceFeature { .. }
                | LevelEvent::BlockEvent { .. } => {}
            }
        }
        Ok(())
    }
    /// Menus opened by PlayerEffect::OpenMenu.
    fn open_menu(&mut self, pos: [i32; 3]) {
        let name = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name
            .clone();
        let key = (pos[0], pos[1], pos[2]);
        if self.chest_menu_blocked(key, &name) {
            // The use still succeeds, but no menu, stat or piglin anger.
            return;
        }
        if &*name == "minecraft:lectern" {
            self.open_lectern(pos);
            return;
        }
        if crate::hc::command_block::Mode::of_block(&name).is_some() {
            self.open_command_block(key);
            return;
        }
        // SignBlock.useWithoutItem: the text editor, not a container.
        if crate::hc::signs::sign_kind(&name).is_some() {
            self.use_sign_empty_hand(pos);
            return;
        }
        if &*name == "minecraft:crafting_table" {
            self.craft_size = 3;
            self.craft_table = Some(pos);
        } else if &*name == "minecraft:ender_chest" {
            self.open_ender_chest(key);
            // EnderChestBlock.useWithoutItem
            self.stats.custom("open_enderchest", 1);
            self.anger_piglins(true);
            if self.container_opener_event(key, true) {
                self.container_edge_sound(key, true);
            }
        } else if &*name == "minecraft:beacon" && self.open_station(pos, &name) {
            self.world.block_entities.get_or_insert_with(key, || {
                BlockEntity::for_block(&name).unwrap_or(BlockEntity::Chest(Vec::new()))
            });
        } else if storage::is_shulker_box(&name) && !self.shulker_can_open(pos) {
            // ShulkerBoxBlock.canOpen: the lid has no room; nothing opens.
        } else if let Some(entity) = BlockEntity::for_block(&name) {
            self.world.block_entities.get_or_insert_with(key, || entity);
            let centre = [
                f64::from(key.0) + 0.5,
                f64::from(key.1) + 0.5,
                f64::from(key.2) + 0.5,
            ];
            self.resolve_container_maps(key, centre);
            self.container = Some(key);
            let first = self.container_opener_event(key, true);
            self.post_block_game_event(
                "minecraft:container_open",
                [key.0, key.1, key.2],
                Some(crate::hc::mobs::Who::Player),
                None,
            );
            let stat = match &*name {
                "minecraft:chest" => "open_chest",
                "minecraft:trapped_chest" => "trigger_trapped_chest",
                "minecraft:barrel" => "open_barrel",
                "minecraft:hopper" => "inspect_hopper",
                "minecraft:dropper" => "inspect_dropper",
                "minecraft:dispenser" => "inspect_dispenser",
                n if storage::is_shulker_box(n) => "open_shulker_box",
                _ => "",
            };
            if !stat.is_empty() {
                self.stats.custom(stat, 1);
            }
            // Opening a piglin-guarded container angers the piglins who see it.
            if self.guarded_by_piglins(&name) {
                self.anger_piglins(true);
            }
            if first {
                self.container_edge_sound(key, true);
            }
        } else if self.open_station(pos, &name) {
            // Workstation menus: game/stations.rs.
        } else {
            self.message = Some((
                format!(
                    "{} is not available yet",
                    name.trim_start_matches("minecraft:").replace('_', " ")
                ),
                Instant::now(),
            ));
            return;
        }
        self.screen = Screen::Inventory;
        self.capture(false);
        self.right = false;
    }
    /// FallingBlockEntity.tick: fall with 0.04 gravity and 0.98 drag, then land.
    fn tick_falling(&mut self) -> Result<()> {
        let falling = std::mem::take(&mut self.falling);
        let mut kept = Vec::with_capacity(falling.len());
        let mut collision_error = None;
        let entity_drops = self.world_data.rules.bool("entity_drops");
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        // FallingBlockEntity.tick places or drops each block in its own tick,
        // so a block falling onto one that just landed lands on top of it.
        for mut f in falling {
            if !ticking(f.position.x.floor() as i32, f.position.z.floor() as i32) {
                kept.push(f);
                continue;
            }
            let original = f.clone();
            match self.fall_step(&mut f, entity_drops) {
                Ok(FallStep::Falling) => kept.push(f),
                Ok(FallStep::Gone) => {}
                Ok(FallStep::Dropped(at)) => self.drop_falling_block(f.state, at),
                Ok(FallStep::Landed(pos)) => self.finish_falling_block(f, pos),
                Err(error) => {
                    kept.push(original);
                    collision_error.get_or_insert(error);
                }
            }
        }
        // Blocks that started falling meanwhile tick from the next tick on.
        let started = std::mem::replace(&mut self.falling, kept);
        self.falling.extend(started);
        self.process_level_events()?;
        if let Some(error) = collision_error {
            return Err(error);
        }
        Ok(())
    }
    /// One `FallingBlockEntity.tick` move of `f`.
    fn fall_step(&self, f: &mut FallingBlock, entity_drops: bool) -> Result<FallStep> {
        f.previous = f.position;
        f.age = f.age.wrapping_add(1);
        if !f.no_gravity {
            f.velocity.y -= 0.04;
        }
        let mut wanted = f.velocity;
        // Entity.move: a cobweb it was in scales this move, which then
        // starts from rest.
        if f.stuck.length_squared() > 1.0e-7 {
            wanted = wanted.multiply(f.stuck);
            f.stuck = Vec3::ZERO;
            f.velocity = Vec3::ZERO;
        }
        let mut scene = collisions::SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: Some(collisions::PowderSnowSubject {
                fall_distance: f.fall_distance,
                falling_block: true,
                ..Default::default()
            }),
        };
        let context = CollisionContext::for_entity(
            CollisionEntity {
                identity: EntityIdentity(0),
                y: f.position.y,
                descending: wanted.y < 0.0,
                main_hand: None,
            },
            false,
        );
        // On an error the caller keeps the block as it was before this tick.
        let movement = scene.resolve_with_context(
            crate::hc::falling_block::bounds(f.position),
            wanted,
            0.0,
            false,
            context,
        )?;
        let next = Vec3::new(
            f.position.x + movement.x,
            f.position.y + movement.y,
            f.position.z + movement.z,
        );
        let on_ground = wanted.y < 0.0 && movement.y != wanted.y;
        // FallingBlockEntity.tick: off the ground, it gives up after 600
        // ticks (100 outside the world) and drops itself as an item.
        let y = next.y.floor() as i32;
        let outside = y <= crate::hc::chunk::MIN_Y || y >= crate::hc::chunk::MIN_Y + 384;
        if !on_ground && (f.age > 100 && outside || f.age > 600) {
            return Ok(if f.drop_item && entity_drops {
                FallStep::Dropped(next)
            } else {
                FallStep::Gone
            });
        }
        if next.y < f.position.y {
            f.fall_distance += f.position.y - next.y;
        }
        let from = f.position;
        f.position = next;
        if on_ground {
            let pos = [
                f.position.x.floor() as i32,
                f.position.y.floor() as i32,
                f.position.z.floor() as i32,
            ];
            return Ok(FallStep::Landed(pos));
        }
        // applyEffectsFromBlocks: WebBlock.entityInside makes it stuck
        // (Entity.makeStuckInBlock, which forgets the fall). Entity.move
        // records the move unless it was blocked short of a tiny one.
        let moved = movement.length_squared();
        let recorded = moved > 1.0e-7 || wanted.length_squared() - moved < 1.0e-7;
        let world = &self.world;
        let facts = self.res.facts();
        if crate::hc::falling_block::passes_cobweb(
            from,
            f.position,
            recorded.then_some(wanted),
            |p| &*facts.get(world.block(p.x, p.y, p.z)).name == "minecraft:cobweb",
        ) {
            f.fall_distance = 0.0;
            f.stuck = crate::hc::falling_block::web_multiplier();
        }
        if movement.x != wanted.x {
            f.velocity.x = 0.0;
        }
        if movement.y != wanted.y {
            f.velocity.y = 0.0;
        }
        if movement.z != wanted.z {
            f.velocity.z = 0.0;
        }
        f.velocity = Vec3::new(
            f.velocity.x * 0.98,
            f.velocity.y * 0.98,
            f.velocity.z * 0.98,
        );
        Ok(FallStep::Falling)
    }
    /// `Entity.spawnAtLocation`: an item entity where the block was.
    fn drop_falling_block(&mut self, state: BlockStateId, at: Vec3) {
        // Entity.spawnAtLocation: an item entity where the block was.
        let item = self.res.facts().get(state).item.clone();
        if &*item != "minecraft:air" {
            let r = &mut self.world.random;
            let v = [
                r.next_double() * 0.2 - 0.1,
                0.2,
                r.next_double() * 0.2 - 0.1,
            ];
            let stack = ItemStack {
                item,
                count: 1,
                damage: 0,
                extra: None,
            };
            self.spawn_item(stack, [at.x, at.y, at.z], v, 10);
        }
    }
    /// `DecoratedPotBlock.playerWillDestroy`: a pot mined with an item in
    /// `breaks_decorated_pots`, unless an enchantment prevents shattering, is
    /// cracked first (flags 260), so it shatters into its sherds.
    fn crack_mined_pot(&mut self, pos: [i32; 3], state: BlockStateId) -> BlockStateId {
        let registry = &self.pack.assets.registry;
        let Some(mut pot) = registry
            .state(state)
            .filter(|s| s.name == "minecraft:decorated_pot")
            .cloned()
        else {
            return state;
        };
        let Some(held) = self.held() else {
            return state;
        };
        let breaks = self
            .res
            .items()
            .item_tags
            .get("minecraft:breaks_decorated_pots")
            .is_some_and(|t| t.contains(&*held.item));
        let enchantments = &self.res.stations().enchantments;
        let kept = held
            .enchantments()
            .iter()
            .any(|(e, _)| enchantments.in_tag("minecraft:prevents_decorated_pot_shattering", e));
        if !breaks || kept {
            return state;
        }
        pot.properties.insert("cracked".into(), "true".into());
        let Ok(cracked) = registry.resolve(&pot) else {
            return state;
        };
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, cracked, 260));
        cracked
    }
    /// `ItemStack.mineBlock` for the held stack once a survival player has
    /// mined the block `name`: an item with a `tool` component counts as used
    /// and loses its `damage_per_block` on a block that doesn't break
    /// instantly (`ShearsItem`: on any block but fire).
    fn mine_block_with_held(&mut self, name: &str, hardness: f32) {
        let Some(stack) = self.inventory[self.selected].clone() else {
            return;
        };
        let Some((per_block, max)) = self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| Some((i.tool_damage_per_block?, i.max_damage)))
        else {
            return;
        };
        let wears = if &*stack.item == "minecraft:shears" {
            !self.res.items().block_in_tag("minecraft:fire", name)
        } else {
            hardness != 0.0
        };
        if wears && per_block > 0 && max > 0 {
            let loss = self.durability_loss(&stack, per_block);
            if let Some(held) = self.inventory[self.selected].as_mut() {
                held.damage += loss;
                if held.damage >= max {
                    self.inventory[self.selected] = None;
                }
            }
        }
        self.stats.add("used", &stack.item, 1);
    }
    fn break_block(&mut self, pos: [i32; 3]) -> Result<()> {
        self.with_player_actor(|game| game.break_block_as_player(pos))
    }
    fn break_block_as_player(&mut self, pos: [i32; 3]) -> Result<()> {
        if !self
            .res
            .items()
            .can_destroy_block(self.held(), self.creative)
        {
            return Ok(());
        }
        // BlockBreakEvent (a server's player): a plugin may keep the block, or
        // break it without its drops.
        let Some(drops) = self.plugin_block_break(pos) else {
            return Ok(());
        };
        if drops {
            return self.destroy_block_as_player(pos);
        }
        let rule = self.world_data.rules.bool("block_drops");
        let no = crate::hc::gamerules::Value::Bool(false);
        self.world_data.rules.set("block_drops", no);
        let result = self.destroy_block_as_player(pos);
        let was = crate::hc::gamerules::Value::Bool(rule);
        self.world_data.rules.set("block_drops", was);
        result
    }
    /// ServerPlayerGameMode.destroyBlock once the block may go.
    fn destroy_block_as_player(&mut self, pos: [i32; 3]) -> Result<()> {
        // ServerPlayerGameMode.destroyBlock.
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let broken = state;
        let facts = self.res.facts().get(state).clone();
        let state = self.crack_mined_pot(pos, state);
        // Block.playerWillDestroy: mining their gold angers all nearby piglins.
        if self.guarded_by_piglins(&facts.name) {
            self.anger_piglins(false);
        }
        // The block entity as it was, for what the drops copy from it.
        let key = (pos[0], pos[1], pos[2]);
        if let Some(entity) = self.world.block_entities.get(&key).cloned() {
            let name = self.world.block_names.get(&key).cloned();
            self.pending.record(pos, &entity, &facts.name, name);
        } else if let Some(tag) = spill::saved_entity(&self.world, pos) {
            self.pending.record_saved(pos, &tag);
        }
        self.creaking_heart_broken(pos, true);
        let hive = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Beehive(h)) => self.hive_state(pos).map(|s| ((**h).clone(), s)),
            _ => None,
        };
        // playerWillDestroy and Level.removeBlock through the vanilla behaviour
        // library: the block entity's side effects (containers spill), neighbour
        // and shape updates, and the destroy event (particles, sound).
        let creative = self.creative;
        // TripWireBlock.playerWillDestroy: shears disarm the string first (flags
        // 260), so taking it down does not trip the hook.
        if &*facts.name == "minecraft:tripwire"
            && self.held().is_some_and(|s| &*s.item == "minecraft:shears")
            && let Some(disarmed) = self.with_props(state, &[("disarmed", "true")])
        {
            let at = BlockPos::new(pos[0], pos[1], pos[2]);
            self.with_level(|b, level, ticks| b.set_block(level, ticks, at, disarmed, 260));
            self.post_block_game_event("minecraft:shear", pos, self.sculk_actor(), None);
        }
        // BeehiveBlock.playerWillDestroy: a creative break still drops the hive
        // with its bees and honey level.
        if creative
            && self.world_data.rules.bool("block_drops")
            && let Some((h, (honey, _))) = &hive
            && (!h.bees.is_empty() || honey.is_some_and(|l| l > 0))
            && let Some(stack) = self.picked_stack(pos, true)
        {
            let at = pos.map(f64::from);
            self.spawn_item(stack, at, dispenser::new_item_motion(), 10);
        }
        let correct_tool =
            !facts.requires_tool || self.res.items().correct_for_drops(self.held(), &facts.name);
        let removed = self.with_level(|b, level, ticks| {
            b.player_destroy_block(
                level,
                ticks,
                BlockPos::new(pos[0], pos[1], pos[2]),
                creative,
                correct_tool,
            )
        });
        if removed && !creative {
            let held = self.held().cloned();
            self.mine_block_with_held(&facts.name, facts.hardness);
            let harvest = !facts.requires_tool
                || self
                    .res
                    .items()
                    .correct_for_drops(held.as_ref(), &facts.name);
            if harvest {
                // Block.playerDestroy: the mined statistic, the exhaustion and the drops.
                self.stats.add("mined", &facts.name, 1);
                if let Some(p) = self.player.as_mut() {
                    p.cause_food_exhaustion(0.005);
                }
                let drops = self.block_loot_at(pos, state, held.as_ref());
                let silk = held
                    .as_ref()
                    .is_some_and(|h| h.enchantment_level("minecraft:silk_touch") > 0);
                for d in drops {
                    if &*d.item != "minecraft:air" {
                        self.pop_resource(pos, d);
                    }
                }
                // BeehiveBlock.playerDestroy: silk touch keeps the bees in, so
                // the trigger counts them; otherwise they are out already.
                if let Some((hive, (honey, facing))) = hive {
                    let bees = if silk { hive.bees.len() as i64 } else { 0 };
                    if !silk {
                        self.hive_broken_by_player(pos, hive, honey, facing);
                    }
                    self.trigger_bee_nest_destroyed(broken, held.as_ref(), bees);
                }
                // InfestedBlock.spawnAfterBreak: its silverfish comes out unless the
                // tool has silk touch (#prevents_infested_spawns) or blocks don't drop.
                if facts.name.starts_with("minecraft:infested_")
                    && !silk
                    && self.world_data.rules.bool("block_drops")
                {
                    let at = [
                        f64::from(pos[0]) + 0.5,
                        f64::from(pos[1]),
                        f64::from(pos[2]) + 0.5,
                    ];
                    if let Some(id) = self.spawn_mob("minecraft:silverfish", at, false) {
                        self.mob_poof(id);
                    }
                }
                // SpawnerBlock.spawnAfterBreak pops its experience whatever the tool.
                let xp = if silk && &*facts.name != "minecraft:spawner" {
                    0
                } else {
                    self.block_experience(&facts.name)
                };
                if xp > 0 {
                    self.spawn_orbs(
                        [
                            f64::from(pos[0]) + 0.5,
                            f64::from(pos[1]) + 0.5,
                            f64::from(pos[2]) + 0.5,
                        ],
                        xp,
                    );
                }
            }
            // IceBlock.playerDestroy: the ice melts back into water (the
            // #prevents_ice_melting tool tag holds only silk touch).
            let prevents_melting = held
                .as_ref()
                .is_some_and(|h| h.enchantment_level("minecraft:silk_touch") > 0);
            self.with_level(|b, level, ticks| {
                b.ice_destroyed_by_player(
                    level,
                    ticks,
                    BlockPos::new(pos[0], pos[1], pos[2]),
                    broken,
                    prevents_melting,
                );
            });
        }
        self.pending.clear(pos);
        self.process_level_events()?;
        Ok(())
    }
    /// The hand whose use ended the click, if any.
    fn use_item(&mut self) -> Result<Option<Hand>> {
        self.with_player_actor(|game| game.use_item_as_player())
    }
    /// `Minecraft.startUseItem`: the main hand, then the off hand.
    fn use_item_as_player(&mut self) -> Result<Option<Hand>> {
        // Spectators only look (MultiPlayerGameMode.useItemOn/useItem pass).
        if self.spectator() {
            return Ok(None);
        }
        // A carrot or warped fungus on a stick boosts the ridden pig or strider.
        // A guest's use packets name one hand and touch no entity: the client
        // sends one per hand and part of its click, and Interact for entities.
        let guest = self.guest_use;
        if let Some(GuestUse::Entity(hand)) = guest {
            return Ok(self
                .with_hand(hand, |g| g.interact_entity())?
                .then_some(hand));
        }
        let on_block = matches!(guest, Some(GuestUse::OnBlock(_)));
        if !on_block
            && self.riding.is_some()
            && let Some(item) = self.held().map(|s| s.item.clone())
            && self.use_steering_stick(&item)
        {
            return Ok(Some(Hand::Main));
        }
        if guest.is_none() && self.interact_entity()? {
            return Ok(Some(Hand::Main));
        }
        // An EntityHitResult may fall through to Item.use, never to useItemOn.
        let block_hit = if matches!(guest, Some(GuestUse::Item(_))) || self.entity_targeted() {
            None
        } else {
            self.pick()
        };
        let hands = match guest {
            Some(GuestUse::OnBlock(hand) | GuestUse::Item(hand) | GuestUse::Entity(hand)) => {
                vec![hand]
            }
            None => Hand::BOTH.to_vec(),
        };
        let entity = guest.is_none() && self.entity_targeted();
        for hand in hands {
            // The off hand's turn starts on the entity too (Minecraft.startUseItem).
            if entity && hand == Hand::Off && self.with_hand(hand, |g| g.interact_entity())? {
                return Ok(Some(hand));
            }
            if self.with_hand(hand, |game| game.use_hand(block_hit.as_ref()))? {
                return Ok(Some(hand));
            }
        }
        Ok(None)
    }
    /// `Player.interactOn` for the entity under the crosshair with the
    /// acting hand: whether it took the click.
    fn interact_entity(&mut self) -> Result<bool> {
        Ok(self.interact_frame()
            || self.interact_armor_stand()
            || self.interact_cushion()
            || self.interact_boat()
            || self.interact_cart()
            || self.interact_mob()?
            || self.interact_knot())
    }
    /// `ItemStack.applyAfterUseComponentSideEffects`: a used stack with a
    /// `use_cooldown` component starts its entry (`UseCooldown.apply`). A
    /// server's guest also gets `ClientboundCooldownPacket`
    /// (`ServerItemCooldowns.onCooldownStarted`), queued for their turn's end.
    pub(super) fn apply_use_cooldown(&mut self, stack: &ItemStack) {
        let default = self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.use_cooldown.clone());
        let Some(component) = crate::hc::cooldowns::component(stack, default.as_ref()) else {
            return;
        };
        let group = crate::hc::cooldowns::group(stack, default.as_ref());
        self.cooldowns.add(Arc::clone(&group), component.ticks());
        if self.lan_acting.is_some() {
            self.guest_cooldowns.push((group, component.ticks()));
        }
    }
    /// One hand's turn in `startUseItem`: `useItemOn` the block aimed at,
    /// then `useItem`. Whether the click ends here (it acted or failed).
    fn use_hand(&mut self, block_hit: Option<&Hit>) -> Result<bool> {
        // isSecondaryUseActive with something in either hand keeps blocks
        // from reacting.
        let sneaking = self.keys.contains(&KeyCode::ShiftLeft);
        let hands_full = self.stack_in(self.selected).is_some() || self.offhand.is_some();
        let mut anchor_passed = false;
        // MultiPlayerGameMode.useItemOn: blocks react first.
        if !(sneaking && hands_full)
            && let Some(hit) = block_hit
        {
            let held = self.held().cloned();
            if self.use_lectern(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_sign_applicator(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_golem_statue(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_jukebox(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if let Some(swings) = self.use_chiseled_bookshelf(hit, held.as_ref()) {
                if swings {
                    self.swing_arm();
                }
                return Ok(true);
            }
            if self.use_composter(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_vault(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_decorated_pot(hit.pos, held.as_ref()) {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_pumpkin(hit.pos, hit.face, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_beehive(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_campfire_douse(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            if self.use_redstone_ore(hit.pos, held.as_ref())? {
                self.swing_arm();
                return Ok(true);
            }
            match self.use_respawn_anchor(hit.pos, held.as_ref())? {
                respawn_anchor::AnchorUse::Charged => {
                    self.swing_arm();
                    return Ok(true);
                }
                // The main hand passes; its item is used, then the off hand's glowstone.
                respawn_anchor::AnchorUse::Pass => anchor_passed = true,
                respawn_anchor::AnchorUse::Other => {}
            }
            // CeilingHangingSignBlock.useItemOn: a hanging sign aimed at the
            // underside passes, so the next sign chains below instead of
            // opening the editor (signs here carry no click commands).
            let chains_sign = hit.face == [0, -1, 0]
                && held
                    .as_ref()
                    .is_some_and(|s| s.item.ends_with("_hanging_sign"))
                && {
                    let name = &self
                        .res
                        .facts()
                        .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
                        .name;
                    name.ends_with("_hanging_sign") && !name.contains("_wall_")
                }
                && matches!(
                    self.world
                        .block_entities
                        .get(&(hit.pos[0], hit.pos[1], hit.pos[2])),
                    Some(BlockEntity::Sign(_))
                );
            // TRY_WITH_EMPTY_HAND: only the main hand's turn uses the block
            // itself (useWithoutItem).
            if !anchor_passed && !chains_sign && self.hand == Hand::Main {
                let face = match hit.face {
                    [0, 1, 0] => Direction::Up,
                    [0, -1, 0] => Direction::Down,
                    [0, 0, -1] => Direction::North,
                    [0, 0, 1] => Direction::South,
                    [-1, 0, 0] => Direction::West,
                    _ => Direction::East,
                };
                let player = self.use_context();
                let at = Vec3::new(hit.point[0], hit.point[1], hit.point[2]);
                let item = held.as_ref().map(|s| s.item.to_string());
                let result = self.with_level(|b, level, ticks| {
                    b.use_item_on(
                        level,
                        ticks,
                        BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
                        face,
                        at,
                        player,
                        item.as_deref(),
                    )
                });
                self.process_level_events()?;
                // NoteBlock.useWithoutItem (tuned) and BellBlock.onHit (rung).
                if result == horizon_world::level::InteractionResult::Success {
                    let name = self
                        .res
                        .facts()
                        .get(self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]))
                        .name
                        .clone();
                    match &*name {
                        "minecraft:note_block" => self.stats.custom("tune_noteblock", 1),
                        "minecraft:bell" => self.stats.custom("bell_ring", 1),
                        _ => {}
                    }
                }
                if result.consumes_action() {
                    self.swing_arm();
                    return Ok(true);
                }
            }
        }
        let Some(stack) = self.held().cloned() else {
            return Ok(false);
        };
        let Some(info) = self.res.items().get(&stack.item).cloned() else {
            return Ok(false);
        };
        // ServerPlayerGameMode.useItem and useItemOn: an item on cooldown is
        // not used (PASS, so the other hand may still act).
        if self.cooldowns.is_on_cooldown(&crate::hc::cooldowns::group(
            &stack,
            info.use_cooldown.as_ref(),
        )) {
            return Ok(false);
        }
        if block_hit.is_some() && (self.use_bone_meal(&stack)? || self.ignite_tnt(&stack)?) {
            return Ok(true);
        }
        // A guest's UseItemOn stops at ItemStack.useOn: what Item.use does
        // (throw, drink, eat, raise, equip) waits for its UseItem.
        let on_block = matches!(self.guest_use, Some(GuestUse::OnBlock(_)));
        let spawn_egg = stack.item.ends_with("_spawn_egg");
        use crate::hc::first_person::UseResult;
        let swings = self.swings_started;
        match self.use_tool(&stack, block_hit.is_some())? {
            UseResult::Success { swing } => {
                if (UseResult::Success { swing }).should_swing(swings, self.swings_started) {
                    self.swing_arm();
                }
                return Ok(true);
            }
            UseResult::Fail => return Ok(true),
            UseResult::Pass => {}
        }
        if (!on_block && self.use_potion_item(&stack)?)
            || ((block_hit.is_some() || !spawn_egg)
                && (!on_block || spawn_egg)
                && self.throw_item(&stack)?)
        {
            return Ok(true);
        }
        // BrushItem.useOn: brushing starts on the block aimed at (CONSUME).
        if block_hit.is_some() && &*stack.item == "minecraft:brush" {
            if self.using.is_none() {
                self.using = Some((self.held_slot(), 0));
            }
            return Ok(true);
        }
        let held = self.held_slot();
        if !on_block
            && (self.raise_shield(held)
                || self.use_fishing_rod(&stack)
            || self.use_crossbow(&stack)
            || self.use_trident(&stack)
            || self.use_spear(&stack)
            || self.boost_firework(&stack)
            || self.use_empty_map(&stack)
                || self.use_book(&stack)
                // Block items (heads, carved pumpkins) aimed at a block are
                // placed first (useItemOn before useItem).
                || (!(info.block.is_some() && block_hit.is_some())
                    && self.equip_from_hand(&stack)))
        {
            return Ok(true);
        }
        if !on_block && &*stack.item == "minecraft:spyglass" {
            self.use_spyglass();
            return Ok(true);
        }
        // BundleItem.use: startUsingItem, then onUseTick empties it.
        if !on_block && info.bundle {
            if self.using.is_none() {
                self.using = Some((self.held_slot(), 0));
            }
            self.swing_arm();
            return Ok(true);
        }
        if !on_block && &*stack.item == "minecraft:bow" {
            if self.using.is_none()
                && (self.creative || self.projectile_slot("minecraft:bow").is_some())
            {
                self.using = Some((self.held_slot(), 0));
            }
            return Ok(true);
        }
        // Block foods attempt placement before starting consumption.
        if !on_block && (info.block.is_none() || block_hit.is_none()) && self.start_eating(&info) {
            return Ok(true);
        }
        // BoatItem.use also performs its own fluid/block raycast.
        if !on_block && self.place_boat(&stack) {
            return Ok(true);
        }
        if let Some(hit) = block_hit
            && (self.launch_firework(hit, &stack)
                || self.place_minecart(hit, &stack)
                || self.place_painting(hit, &stack)
                || self.place_item_frame(hit, &stack)
                || self.use_lodestone_compass(hit, &stack)
                || self.place_armor_stand(hit, &stack)
                || self.place_cushion(hit, &stack)
                || self.place_end_crystal(hit, &stack))
        {
            return Ok(true);
        }
        if self.use_ender_eye(&stack, block_hit.is_some())? {
            return Ok(true);
        }
        let Some(hit) = block_hit else {
            return Ok(false);
        };
        if info.block.is_none() {
            return Ok(false);
        }
        if !self.may_build() {
            // ItemStack.useOn passes in adventure mode; useItem eats.
            return Ok(!on_block && self.start_eating(&info));
        }
        let face = match hit.face {
            [0, 1, 0] => Direction::Up,
            [0, -1, 0] => Direction::Down,
            [0, 0, -1] => Direction::North,
            [0, 0, 1] => Direction::South,
            [-1, 0, 0] => Direction::West,
            _ => Direction::East,
        };
        // BlockItem.place through the vanilla placement rules.
        let ctx = PlaceContext {
            clicked_pos: BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]),
            face,
            hit: Vec3::new(hit.point[0], hit.point[1], hit.point[2]),
            inside: false,
            yaw: self.yaw,
            pitch: self.pitch,
            sneaking: self.keys.contains(&KeyCode::ShiftLeft),
            item: stack.item.to_string(),
            can_use_game_master_blocks: self.can_use_game_master_blocks(),
        };
        // What the placement may replace, for BlockPlaceEvent's undo.
        let before = {
            let c = hit.pos;
            let next = [c[0] + hit.face[0], c[1] + hit.face[1], c[2] + hit.face[2]];
            [
                (c, self.world.block(c[0], c[1], c[2])),
                (next, self.world.block(next[0], next[1], next[2])),
            ]
        };
        let outcome = self.with_level(|b, level, ticks| b.place_block(level, ticks, &ctx));
        // BlockPlaceEvent (a server's player): a plugin may take the block back.
        if let PlaceOutcome::Placed { pos, state } = &outcome {
            let target = [pos.x, pos.y, pos.z];
            let replaced = before
                .iter()
                .find(|(p, _)| *p == target)
                .map_or(self.world.air, |(_, s)| *s);
            let item = Some(horizon_server_api::ItemStack::new(&stack.item, 1));
            if !self.plugin_block_place(target, *state, replaced, item) {
                let at = BlockPos::new(target[0], target[1], target[2]);
                self.with_level(|b, level, ticks| b.set_block(level, ticks, at, replaced, 3));
                return Ok(true);
            }
        }
        let PlaceOutcome::Placed { pos, state } = outcome else {
            self.process_level_events()?;
            // BlockItem.useOn: a consumable that could not be placed is used,
            // which ItemStack.useOn counts as a use of the item.
            if self.start_eating(&info) {
                self.stats.add("used", &stack.item, 1);
            }
            return Ok(true);
        };
        let target = [pos.x, pos.y, pos.z];
        let state = self.apply_item_block_state(target, state, &stack)?;
        if let Some(entity) = BlockEntity::for_block(&self.res.facts().get(state).name) {
            let key = (target[0], target[1], target[2]);
            self.world.block_entities.insert(key, entity);
            let game_master = self.can_use_game_master_blocks();
            self.apply_placed_item(target, &stack, game_master);
            // Block.setPlacedBy.
            match self.world.block_entities.get(&key) {
                Some(BlockEntity::CommandBlock(_)) => {
                    let data = stack.data().is_some_and(|d| {
                        d.other(crate::hc::block_entity_item::BLOCK_ENTITY_DATA)
                            .is_some()
                    });
                    self.command_block_placed(key, !data);
                }
                Some(BlockEntity::Sign(sign)) if !sign.waxed => self.open_sign_editor(key, true),
                _ => {}
            }
        }
        self.swing_arm();
        // BlockItem.place's sound is among the level events;
        // SolidBucketItem.getPlaceSound replaces it with the bucket's own.
        let solid_bucket = &*stack.item == "minecraft:powder_snow_bucket";
        if solid_bucket {
            for event in &mut self.level_events {
                if let LevelEvent::Sound { pos: at, sound, .. } = event
                    && *at == pos
                {
                    *sound = "minecraft:item.bucket.empty_powder_snow".into();
                }
            }
        }
        self.process_level_events()?;
        // BlockItem.place: the used statistic and the placed_block trigger.
        self.stats.add("used", &stack.item, 1);
        self.trigger_block("minecraft:placed_block", target, &stack);
        self.check_wither_spawn(target)?;
        self.check_golem_spawn(target)?;
        self.consume_held();
        // SolidBucketItem.useOn: BucketItem.getEmptySuccessItem in hand.
        if solid_bucket && !self.creative {
            *self.held_mut() = Some(ItemStack::new("minecraft:bucket", 1));
        }
        Ok(true)
    }
    /// The player as a block's `useItemOn` / `useWithoutItem` sees them.
    fn use_context(&self) -> PlayerContext {
        PlayerContext {
            yaw: self.yaw,
            pitch: self.pitch,
            may_build: self.may_build(),
            can_eat: self.creative
                || self
                    .player
                    .as_ref()
                    .is_some_and(|p| p.health.food.food < 20),
            game_master: self.can_use_game_master_blocks(),
        }
    }
    /// `Consumable.startConsuming` for a held consumable: food needs
    /// `Player.canEat` (`Consumable.canConsume`), anything else is always
    /// consumed. The right click is held for `consume_seconds`.
    fn start_eating(&mut self, info: &crate::hc::items::ItemInfo) -> bool {
        if info.consumable.is_none()
            || (info.food.is_some()
                && !self.player.as_ref().is_some_and(|p| {
                    info.can_always_eat || p.health.food.food < 20 || self.creative
                }))
        {
            return false;
        }
        if self.using.is_none() {
            self.using = Some((self.held_slot(), 0));
        }
        true
    }
    fn apply_consume_effect(&mut self, effect: &crate::hc::items::ConsumeEffect) {
        use crate::hc::items::ConsumeEffect;
        match effect {
            ConsumeEffect::ApplyEffects {
                effects,
                probability,
            } => {
                // ApplyStatusEffectsConsumeEffect.apply: the user's random
                // (the level's stands in, as for every player draw) is drawn
                // even at probability 1.
                if self.world.random.next_float() < *probability {
                    for eff in effects {
                        if let Some(def) = crate::hc::brewing::effect(&eff.id) {
                            if def.instant {
                                if let Some(a) =
                                    crate::hc::effects::instant(def.id, eff.amplifier, 1.0)
                                {
                                    self.apply_effect_action(a);
                                }
                            } else {
                                self.add_player_effect(crate::hc::effects::EffectInstance::new(
                                    def.id,
                                    eff.duration,
                                    eff.amplifier,
                                ));
                            }
                        }
                    }
                }
            }
            ConsumeEffect::ClearAllEffects => {
                self.potions.effects.clear();
                self.apply_effect_status();
            }
            ConsumeEffect::RemoveEffects(to_remove) => {
                for target in to_remove {
                    let id = target.trim_start_matches("minecraft:");
                    self.potions.effects.list.retain(|e| e.id != id);
                }
                self.apply_effect_status();
            }
            ConsumeEffect::TeleportRandomly(teleport) => self.teleport_randomly(teleport),
            ConsumeEffect::PlaySound(sound) => {
                // PlaySoundConsumeEffect: at the user's block position.
                let at = self.player.as_ref().map(|p| {
                    let b = horizon_core::position::BlockPos::containing(p.position);
                    [b.x, b.y, b.z].map(|v| f64::from(v) + 0.5)
                });
                self.sounds.play(sound, at, 1.0, 1.0);
            }
        }
    }
    fn consume_held(&mut self) {
        if self.creative {
            return;
        }
        let cell = self.held_mut();
        if let Some(s) = cell.as_mut() {
            s.count -= 1;
            if s.count == 0 {
                *cell = None;
            }
        }
    }
    fn tick_using(&mut self) {
        let Some(hand) = self.used_hand() else {
            return;
        };
        self.with_hand(hand, Self::tick_used_item);
    }
    /// `LivingEntity.updatingUsingItem` for the item in use, its hand acting.
    fn tick_used_item(&mut self) {
        let Some((slot, ticks)) = self.using else {
            return;
        };
        let Some(stack) = self.stack_in(slot).cloned() else {
            self.using = None;
            return;
        };
        // Switching hotbar slots only ends a main-hand use.
        let hand_changed = slot != self.selected && slot != shield::OFFHAND_SLOT;
        if !self.right || hand_changed || self.screen != Screen::Playing {
            self.using = None;
            self.release_used_item(&stack, ticks, hand_changed);
            return;
        }
        // ServerPlayer.updateUsingItem: USING_ITEM every tick of the use.
        self.trigger_using_item(&stack);
        let Some(info) = self.res.items().get(&stack.item).cloned() else {
            self.using = None;
            return;
        };
        if info.blocks_attacks.is_some() {
            // ShieldItem: used for 72000 ticks, i.e. until released.
            self.using = Some((slot, ticks + 1));
            return;
        }
        if self.tick_drinking(slot, ticks, &stack) {
            return;
        }
        if &*stack.item == "minecraft:bow" {
            self.using = Some((slot, ticks + 1));
            return;
        }
        if &*stack.item == "minecraft:crossbow" {
            self.tick_crossbow(slot, ticks, &stack);
            return;
        }
        if &*stack.item == "minecraft:trident" {
            self.using = Some((slot, ticks + 1));
            return;
        }
        // ItemStack.onUseTick: a spear stabs what its speed allows, until released.
        if crate::hc::spear::Kinetic::of(&stack.item).is_some() {
            self.spear_tick(&stack, ticks);
            self.using = Some((slot, ticks + 1));
            return;
        }
        if &*stack.item == "minecraft:spyglass" {
            // SpyglassItem.USE_DURATION, then finishUsingItem.
            if ticks + 1 >= 1200 {
                self.send_use_item_complete();
                self.stop_spyglass();
                self.using = None;
            } else {
                self.using = Some((slot, ticks + 1));
            }
            return;
        }
        if &*stack.item == "minecraft:brush" {
            self.tick_brush(slot, ticks);
            return;
        }
        if info.bundle {
            self.tick_bundle(slot, ticks);
            return;
        }
        let Some(consumable) = info.consumable.as_ref() else {
            self.using = None;
            return;
        };
        // ItemStack.onUseTick: Consumable.shouldEmitParticlesAndSounds with
        // the remaining ticks before this tick counts them down.
        let duration = consumable.ticks().max(0) as u32;
        let remaining = duration - ticks.min(duration);
        let ticks = ticks + 1;
        if consumable.should_emit(remaining as i32) {
            self.emit_consume(consumable, &stack, 5);
        }
        if ticks >= duration {
            self.using = None;
            self.send_use_item_complete();
            // ItemStack.finishUsingItem's applyAfterUseComponentSideEffects.
            self.apply_use_cooldown(&stack);
            self.finish_consuming(&info, consumable, &stack);
        } else {
            self.using = Some((slot, ticks));
        }
    }
    /// `BundleItem.onUseTick` and `getUseDuration` (200): the first tick and
    /// then every second tick from the 11th on drop one stack out of the
    /// bundle in `slot`; the use ends after 200 ticks.
    fn tick_bundle(&mut self, slot: usize, ticks: u32) {
        const USE_DURATION: u32 = 200;
        let remaining = USE_DURATION - ticks.min(USE_DURATION);
        if ticks == 0 || (remaining < USE_DURATION - 10 && remaining.is_multiple_of(2)) {
            self.drop_bundle_content(slot);
        }
        self.using = (ticks + 1 < USE_DURATION).then_some((slot, ticks + 1));
    }
    /// `BundleItem.dropContent`: `removeOneItemFromBundle` and `Player.drop`,
    /// then the drop sound and the used statistic.
    fn drop_bundle_content(&mut self, slot: usize) {
        let items = self.res.items();
        let Some(mut bundle) = self.stack_in(slot).cloned() else {
            return;
        };
        let Some(mut contents) = crate::hc::bundle::Contents::of(items, &bundle) else {
            return;
        };
        let Some(removed) = contents.remove_one(items) else {
            return;
        };
        contents.store(&mut bundle);
        let item = Arc::clone(&bundle.item);
        *self.stack_in_mut(slot) = Some(bundle);
        let Some(position) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let at = [position.x, position.y, position.z];
        // playRemoveOneSound, then Player.drop and playDropContentsSound at
        // the player's block.
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.sounds
            .play(crate::hc::bundle::REMOVE_ONE, Some(at), 0.8, pitch);
        self.player_drop(removed);
        let block = BlockPos::containing(position);
        let pitch = 0.8 + self.world.random.next_float() * 0.4;
        self.sounds.play(
            crate::hc::bundle::DROP_CONTENTS,
            Some([block.x, block.y, block.z].map(|v| f64::from(v) + 0.5)),
            0.8,
            pitch,
        );
        self.stats.add("used", &item, 1);
    }
    /// `LivingEntity.releaseUsingItem` (`Item.releaseUsing`) for `stack`,
    /// used for `ticks`, its hand acting: a bow or a trident lets go (not
    /// when its hand changed), a spyglass stops scoping.
    pub(super) fn release_used_item(&mut self, stack: &ItemStack, ticks: u32, hand_changed: bool) {
        match &*stack.item {
            "minecraft:bow" if !hand_changed => self.release_bow(ticks),
            "minecraft:trident" if !hand_changed => self.release_trident(ticks),
            "minecraft:spyglass" => self.stop_spyglass(),
            _ => {}
        }
    }
    /// `ServerPlayerGameMode.tick` while a player digs: level event 2020 every
    /// fourth tick since the START (the hit sound), else 2019 (the crack only),
    /// for everyone around, the digger included (`levelEvent(null, ...)`).
    fn destroy_progress_event(&mut self, pos: [i32; 3], face: [i32; 3], ticks: u32) {
        let id = if ticks.is_multiple_of(4) { 2020 } else { 2019 };
        let direction = horizon_core::geometry::Direction::ALL
            .iter()
            .position(|d| d.normal() == face)
            .unwrap_or(1);
        self.level_event_at(id, pos, direction as i32);
    }
    fn tick_mining(&mut self) -> Result<()> {
        if self.destroy_delay > 0 {
            self.destroy_delay -= 1;
        }
        if !self.left || self.screen != Screen::Playing {
            self.mining = None;
            return Ok(());
        }
        let Some(hit) = self.pick() else {
            self.mining = None;
            return Ok(());
        };
        // GameType.isBlockPlacingRestricted: adventure and spectator.
        if !self.may_build()
            || !self
                .res
                .items()
                .can_destroy_block(self.held(), self.creative)
        {
            self.mining = None;
            return Ok(());
        }
        if self.creative {
            if self.destroy_delay == 0 {
                self.break_block(hit.pos)?;
                self.destroy_delay = 5;
            }
            return Ok(());
        }
        if self.destroy_delay > 0 {
            return Ok(());
        }
        if self.mining.as_ref().is_none_or(|m| m.pos != hit.pos) {
            self.attack_block(hit.pos)?;
            self.mining = Some(Mining {
                pos: hit.pos,
                progress: 0.0,
                ticks: 0,
            });
        }
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let facts = self.res.facts().get(state);
        if facts.hardness < 0.0 {
            // The server still counts its destroy ticks on an unbreakable block.
            let m = self.mining.as_mut().ok_or("mining")?;
            m.ticks += 1;
            let (pos, ticks) = (m.pos, m.ticks);
            self.destroy_progress_event(pos, hit.face, ticks);
            return Ok(());
        }
        // Player.getDestroySpeed / BlockBehaviour.getDestroyProgress.
        let held = self.held();
        let mut speed = self.res.items().mining_speed(held, &facts.name);
        speed = self.modify_destroy_speed(speed);
        let player = self.player.as_ref().ok_or("no player")?;
        if !player.ground.on_ground {
            speed /= 5.0;
        }
        let eye = self.eye_position().unwrap_or_default();
        if self
            .res
            .facts()
            .get(self.world.block(
                eye[0].floor() as i32,
                eye[1].floor() as i32,
                eye[2].floor() as i32,
            ))
            .fluid
            == Fluid::Water
        {
            speed *= self.submerged_mining_speed();
        }
        let harvest = !facts.requires_tool || self.res.items().correct_for_drops(held, &facts.name);
        let progress = if facts.hardness == 0.0 {
            1.0
        } else {
            speed / facts.hardness / if harvest { 30.0 } else { 100.0 }
        };
        self.swing_arm();
        let m = self.mining.as_mut().ok_or("mining")?;
        m.progress += progress;
        m.ticks += 1;
        // An instant break at START sends no destroy events.
        if m.progress < 1.0 || m.ticks > 1 {
            let (pos, ticks) = (m.pos, m.ticks);
            self.destroy_progress_event(pos, hit.face, ticks);
        }
        let m = self.mining.as_mut().ok_or("mining")?;
        // ServerPlayerGameMode breaks a player's block at once only when it is instant (START's
        // insta-mine); otherwise it waits for the client's STOP_DESTROY_BLOCK
        // (`guest_stop_digging`), never breaking it on its own count. Breaking it here would
        // take the block from under the player's crack before the player's client predicts the
        // break, so it gets neither particles nor sound.
        if m.progress >= 1.0 && (self.lan_acting.is_none() || m.ticks == 1) {
            let pos = m.pos;
            self.mining = None;
            self.break_block(pos)?;
            self.destroy_delay = 5;
        }
        Ok(())
    }
    /// A column whose surface is a full solid, non-fluid, non-leaf block open to the sky.
    fn land_column(&self, x: i32, z: i32) -> Option<i32> {
        if self.dim == Dim::Nether {
            // Below the bedrock ceiling: a solid floor with two air blocks above.
            for y in (32..120).rev() {
                let floor = self.res.facts().get(self.world.block(x, y - 1, z));
                if floor.full_collision
                    && floor.fluid == Fluid::None
                    && self.res.facts().is_air(self.world.block(x, y, z))
                    && self.res.facts().is_air(self.world.block(x, y + 1, z))
                {
                    return Some(y);
                }
            }
            return None;
        }
        let mut y = MIN_Y + HEIGHT - 1;
        while y > MIN_Y {
            let s = self.world.block(x, y, z);
            if !self.res.facts().is_air(s) {
                let f = self.res.facts().get(s);
                let ok = f.full_collision
                    && f.fluid == Fluid::None
                    && !f.name.contains("leaves")
                    && f.hardness >= 0.0;
                return ok.then_some(y + 1);
            }
            y -= 1;
        }
        None
    }
    /// Landing rules (farmland trampling, turtle eggs) wrote or removed the block
    /// raw: finish `Level.setBlock`'s neighbour updates, so a crop on trampled
    /// farmland breaks. The local player's and every server player's landings.
    fn landing_block_updates(&mut self, event: &crate::hc::landing::FallEvent) {
        use crate::hc::landing::FallEvent;
        let (position, flags) = match *event {
            FallEvent::Write {
                position, flags, ..
            } => (position, flags as i32),
            FallEvent::Destroy { position, .. } => (position, level_flags::UPDATE_ALL),
            _ => return,
        };
        self.with_level(|b, level, ticks| b.notify_written(level, ticks, position, flags));
    }
    /// A server's tick in its first level (`MinecraftServer.tickServer`'s shared parts): the
    /// clock, the weather, the autosave, mods, then the level itself (`tick_world`). A server
    /// has no player of its own: its players are connections (`tick_guests`).
    fn tick_level(&mut self) -> Result<()> {
        self.drop_window_output();
        self.ticks += 1;
        // `/tick freeze` stops the world but not the player.
        let running = self.clock.world_runs();
        if running {
            self.world.game_time += 1;
            if self.world_data.rules.bool("advance_weather") {
                self.world.weather.tick_timers(&mut self.world.random);
            }
            self.world.weather.tick_levels();
            self.tick_weather_effects();
        }
        self.autosaving = self.ticks.is_multiple_of(6000);
        if self.autosaving {
            self.save_game_with(false)?;
        }
        if running {
            self.advance_clock();
        }
        if self.use_delay > 0 {
            self.use_delay -= 1;
        }
        self.tick_totem();
        self.update_totem_ready();
        self.tick_command_extras();
        self.tick_world(running)
    }
    /// What a server tick runs in each level: effects, the
    /// world's entities and blocks, and the network hub.
    fn tick_world(&mut self, running: bool) -> Result<()> {
        self.share_spawn_conditions();
        self.update_chunk_tickets();
        crate::hc::spans::time("tick_effects", || self.tick_effects(running));
        // Player.tick's ItemCooldowns.tick for the window's player (a
        // server's players count theirs down in `tick_guests`).
        self.shield_cooldown = self.shield_cooldown.saturating_sub(1);
        self.cooldowns.tick();
        crate::hc::spans::time("tick_using", || self.tick_using());
        // The players' digging came in with their packets, before the level
        // ticks; ServerLevel.tick then runs the scheduled and random block
        // ticks before any entity, so what they drop or spawn (dispensed
        // items, popped crops) moves on the tick it appears, as in vanilla.
        crate::hc::spans::time("postprocess_chunks", || self.postprocess_chunks())?;
        if running {
            crate::hc::spans::time("tick_blocks", || self.tick_blocks())?;
        }
        if running {
            crate::hc::spans::time("tick_paintings", || self.tick_paintings());
            crate::hc::spans::time("tick_item_frames", || self.tick_item_frames());
            crate::hc::spans::time("tick_armor_stands", || self.tick_armor_stands());
            crate::hc::spans::time("tick_cushions", || self.tick_cushions());
            crate::hc::spans::time("tick_boats", || self.tick_boats());
            crate::hc::spans::time("tick_carts", || self.tick_carts());
            crate::hc::spans::time("tick_thunder", || self.tick_thunder());
            crate::hc::spans::time("tick_lecterns", || self.tick_lecterns());
            crate::hc::spans::time("tick_maps", || self.tick_maps());
            self.tick_lodestone_compasses();
            crate::hc::spans::time("tick_bolts", || self.tick_bolts());
            crate::hc::spans::time("tick_fishing", || self.tick_fishing());
            crate::hc::spans::time("tick_tridents", || self.tick_tridents());
            crate::hc::spans::time("tick_rockets", || self.tick_rockets());
            crate::hc::spans::time("tick_ominous_item_spawners", || {
                self.tick_ominous_item_spawners()
            });
        }
        if running {
            crate::hc::spans::time("entity_portals", || self.handle_entity_portals());
            crate::hc::spans::time("tick_items", || self.tick_items());
            crate::hc::spans::time("touch_arrows", || self.touch_arrows());
        }
        crate::hc::spans::time("spawn_loaded_entities", || self.spawn_loaded_entities());
        if running {
            crate::hc::spans::time("tick_mobs", || self.tick_mobs())?;
            crate::hc::spans::time("recheck_golem_openers", || self.recheck_golem_openers());
            crate::hc::spans::time("tick_experience", || self.tick_experience());
            crate::hc::spans::time("entity_portals", || {
                self.find_entity_portals();
                self.resolve_arrivals();
            });
        }
        crate::hc::spans::time("tick_lan", || self.tick_lan(false));
        crate::hc::spans::time("tick_sleep", || self.tick_sleep());
        self.announce_sleep_status();
        if running {
            crate::hc::spans::time("tick_end", || self.tick_end())?;
        }
        if running {
            crate::hc::spans::time("tick_falling", || self.tick_falling())?;
            crate::hc::spans::time("tick_tnt", || self.tick_tnt())?;
        }
        if running {
            crate::hc::spans::time("tick_block_entities", || self.tick_block_entities())?;
            crate::hc::spans::time("tick_sculk_entities", || self.tick_sculk_entities());
            crate::hc::spans::time("tick_sculk_blocks", || self.tick_sculk_blocks())?;
        }
        if running {
            // The server tickers of conduits and end gateways (a client animates the rest).
            self.tick_conduit_servers();
            self.tick_gateway_servers();
        }
        // ServerBossEvent broadcasts, after this level's entities have ticked.
        self.publish_boss_bars();
        Ok(())
    }
    /// Slot ids: 0..36 player inventory, 100..109 crafting grid, 200 result.
    fn container_kind(&self) -> Option<&BlockEntity> {
        self.container
            .and_then(|k| self.world.block_entities.get(&k))
    }
    fn slot_mut(&mut self, i: usize) -> &mut Option<ItemStack> {
        if i >= mount_menu::SADDLE {
            return self.mount_slot_mut(i);
        }
        if i >= stations::INPUT {
            return self.station_slot_mut(i);
        }
        if (500..504).contains(&i) {
            return &mut self.armor[i - 500];
        }
        if i == 504 {
            return &mut self.offhand;
        }
        if i >= 300 {
            return self.container_slot_mut(i - 300);
        }
        if (100..109).contains(&i) {
            &mut self.craft_grid[i - 100]
        } else {
            &mut self.inventory[i]
        }
    }
    /// The recipe id and result of the crafting grid.
    /// `RecipeCraftingHolder.awardUsedRecipes`: a crafted recipe becomes known
    /// (special ones never do).
    fn award_crafted(&mut self, recipes: &[String]) {
        let index = &self.res.stations().recipe_index;
        let (_, unlocked) = self.level.recipe_book.add_recipes(index, recipes);
        self.recipe_unlocked(&unlocked);
    }
    /// `AbstractContainerScreen` keeps `isQuickCrafting` and the drag it
    /// belongs to on the screen. Horizon keeps both on `Game` (`inv_clicks`,
    /// `inv_drag`, `last_inv_click`), so they are dropped when the pointer
    /// changes hands: a screen that opens or closes starts with its own
    /// state, and a press whose release never reached the screen -- the
    /// pointer left the window, or the screen closed first -- would
    /// otherwise leave every later press on every later screen swallowed.
    pub(super) fn reset_inventory_clicks(&mut self) {
        self.inv_drag = None;
        self.last_inv_click = None;
    }
    pub(super) fn close_container(&mut self) {
        self.reset_inventory_clicks();
        self.close_station();
        self.mount_closed();

        let mut returned: Vec<ItemStack> = self.carried.take().into_iter().collect();
        returned.extend(self.craft_grid.iter_mut().filter_map(Option::take));
        for stack in returned {
            if let Some(rest) = self.give(stack)
                && let Some(eye) = self.eye_position()
            {
                // AbstractContainerMenu.dropOrPlaceInInventory: `drop(stack, false)`.
                self.throw_from_eyes(rest, eye, false);
            }
        }
        self.craft_size = 2;
        self.craft_table = None;
        if let Some(key) = self.container.take() {
            // A cart's or chest boat's items go back to it.
            self.close_cart_container(key);
            let last = self.container_opener_event(key, false);
            self.post_block_game_event(
                "minecraft:container_close",
                [key.0, key.1, key.2],
                Some(crate::hc::mobs::Who::Player),
                None,
            );
            if last {
                self.container_edge_sound(key, false);
            }
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        }
        self.close_ender_chest();
    }
    /// A sound a menu click made the player play (a bundle's insert or
    /// remove): `Entity.playSound` at the player.
    pub(super) fn player_menu_sound(&mut self, sound: &str) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let at = [p.position.x, p.position.y, p.position.z];
        let (volume, pitch) = if sound == crate::hc::bundle::INSERT_FAIL {
            (1.0, 1.0)
        } else {
            (0.8, 0.8 + self.world.random.next_float() * 0.4)
        };
        self.sounds.play(sound, Some(at), volume, pitch);
    }
    /// The open menu as the verified engine (`crate::hc::menu`): the
    /// game's slots are copied in, `run` acts on it, and the slots, carried
    /// stack, menu data and drops are copied back when it returns Some(true)
    /// (Some(false): handled, nothing kept; None: not handled). False when the
    /// open screen is not one the engine models or `run` did not handle it.
    pub(super) fn menu_session(
        &mut self,
        run: impl FnOnce(
            &mut crate::hc::menu::Menu,
            &crate::hc::menu::Env,
            &dyn Fn(usize) -> Option<usize>,
        ) -> Option<bool>,
    ) -> bool {
        use crate::hc::menu::{Env, Kind, Menu};
        #[derive(Clone, Copy)]
        struct Map {
            container: usize,
            table: bool,
            cells: usize,
            plain: bool,
        }
        impl Map {
            fn main(&self) -> usize {
                match (self.plain, self.table) {
                    (false, _) => self.container,
                    (true, true) => 10,
                    (true, false) => 9,
                }
            }
            fn to_menu(self, g: usize) -> Option<usize> {
                let main = self.main();
                if self.plain {
                    match g {
                        200 => Some(0),
                        100..109 if g - 100 < self.cells => Some(1 + g - 100),
                        500..504 if !self.table => Some(5 + g - 500),
                        504 if !self.table => Some(45),
                        9..36 => Some(main + g - 9),
                        0..9 => Some(main + 27 + g),
                        _ => None,
                    }
                } else {
                    match g {
                        300.. if g < 300 + self.container => Some(g - 300),
                        9..36 => Some(main + g - 9),
                        0..9 => Some(main + 27 + g),
                        _ => None,
                    }
                }
            }
            fn to_game(self, i: usize) -> Option<usize> {
                let main = self.main();
                if self.plain {
                    match i {
                        0 => None,
                        1..=9 if i - 1 < self.cells => Some(100 + i - 1),
                        5..9 if !self.table => Some(500 + i - 5),
                        45 if !self.table => Some(504),
                        _ if i >= main && i < main + 27 => Some(9 + i - main),
                        _ if i >= main + 27 && i < main + 36 => Some(i - main - 27),
                        _ => None,
                    }
                } else if i < self.container {
                    Some(300 + i)
                } else if i < self.container + 27 {
                    Some(9 + i - self.container)
                } else {
                    Some(i - self.container - 27)
                }
            }
        }
        // Menu slot -> game slot, for every menu slot.
        let mut plain_map = None;
        let crafter_open = matches!(self.container_kind(), Some(BlockEntity::Crafter(_)));
        let loom_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Loom);
        let merchant_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Merchant);
        let stonecutter_open = self
            .station
            .as_ref()
            .is_some_and(|s| s.kind == stations::Kind::Stonecutter);
        let station_kind = self.station.as_ref().map(|s| s.kind);
        let grindstone_open = station_kind == Some(stations::Kind::Grindstone);
        let cartography_open = station_kind == Some(stations::Kind::Cartography);
        let anvil_open = station_kind == Some(stations::Kind::Anvil);
        let smithing_open = station_kind == Some(stations::Kind::Smithing);
        let enchanting_open = station_kind == Some(stations::Kind::Enchanting);
        let beacon_open = station_kind == Some(stations::Kind::Beacon);
        let (kind, game_of): (Kind, Vec<Option<usize>>) = if self.creative_tab_open() {
            // ItemPickerMenu: the display slots (clicked by the screen), then the hotbar.
            let mut v = vec![None; 45];
            v.extend((0..9).map(Some));
            (Kind::ItemPicker, v)
        } else if let Some(columns) = self
            .mount_menu_columns()
            .filter(|_| false && self.mount_active())
        {
            // A server's HorseInventoryMenu: the saddle, the body armour,
            // the chest, then the inventory.
            let mut v = vec![Some(mount_menu::SADDLE), Some(mount_menu::ARMOR)];
            v.extend((0..columns * 3).map(|i| Some(mount_menu::CHEST + i)));
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Mount(columns), v)
        } else if merchant_open {
            // MerchantMenu: two payment slots, the result, then the inventory.
            let mut v: Vec<Option<usize>> = vec![
                Some(stations::INPUT),
                Some(stations::INPUT + 1),
                Some(stations::RESULT),
            ];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Merchant, v)
        } else if crafter_open {
            // CrafterMenu: the grid, the inventory, then the result.
            let mut v: Vec<Option<usize>> = (300..309).map(Some).collect();
            v.extend((9..36).chain(0..9).map(Some));
            v.push(None);
            (Kind::CrafterBlock, v)
        } else if loom_open {
            // LoomMenu: banner, dye, pattern, result, then the inventory.
            let mut v: Vec<Option<usize>> = (0..3).map(|i| Some(stations::INPUT + i)).collect();
            v.push(Some(stations::RESULT));
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Loom, v)
        } else if stonecutter_open {
            // StonecutterMenu: the input, the result, then the inventory.
            let mut v = vec![Some(stations::INPUT), Some(stations::RESULT)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Stonecutter, v)
        } else if grindstone_open || cartography_open {
            // GrindstoneMenu and CartographyTableMenu: the two inputs, the
            // result, then the inventory.
            let mut v = vec![
                Some(stations::INPUT),
                Some(stations::INPUT + 1),
                Some(stations::RESULT),
            ];
            v.extend((9..36).chain(0..9).map(Some));
            let kind = if grindstone_open {
                Kind::Grindstone
            } else {
                Kind::Cartography
            };
            (kind, v)
        } else if enchanting_open {
            // EnchantmentMenu: the item, the lapis, then the inventory.
            let mut v = vec![Some(stations::INPUT), Some(stations::INPUT + 1)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Enchanting, v)
        } else if beacon_open {
            // BeaconMenu: the payment, then the inventory.
            let mut v = vec![Some(stations::INPUT)];
            v.extend((9..36).chain(0..9).map(Some));
            (Kind::Beacon, v)
        } else if anvil_open || smithing_open {
            // ItemCombinerMenu: the inputs, the result, then the inventory.
            let inputs = if anvil_open { 2 } else { 3 };
            let mut v: Vec<Option<usize>> =
                (0..inputs).map(|i| Some(stations::INPUT + i)).collect();
            v.push(Some(stations::RESULT));
            v.extend((9..36).chain(0..9).map(Some));
            (
                if anvil_open {
                    Kind::Anvil
                } else {
                    Kind::Smithing
                },
                v,
            )
        } else {
            let (kind, map) = if self.container.is_some() {
                let size = self.container_size();
                let (kind, count) = match self.container_kind() {
                    Some(BlockEntity::Chest(_)) => {
                        let shulker = self.container.is_some_and(|k| {
                            let id = self.world.block(k.0, k.1, k.2);
                            storage::is_shulker_box(&self.res.facts().get(id).name)
                        });
                        let kind = match size {
                            5 => Kind::Hopper,
                            9 => Kind::Dispenser,
                            27 if shulker => Kind::Shulker,
                            n => Kind::Chest((n / 9).clamp(1, 6)),
                        };
                        (kind, size)
                    }
                    Some(BlockEntity::Brewing(_)) => (Kind::Brewing, 5),
                    Some(BlockEntity::Furnace(f)) => (
                        Kind::Furnace(
                            match f.kind.recipe_type().trim_start_matches("minecraft:") {
                                "blasting" => "blasting",
                                "smoking" => "smoking",
                                _ => "smelting",
                            },
                        ),
                        3,
                    ),
                    _ => return false,
                };
                let map = Map {
                    container: count,
                    table: false,
                    cells: 0,
                    plain: false,
                };
                (kind, map)
            } else if self.station.is_none() && !self.mount_active() {
                let table = self.craft_size == 3;
                let map = Map {
                    container: 0,
                    table,
                    cells: self.craft_size * self.craft_size,
                    plain: true,
                };
                (
                    if table {
                        Kind::Crafting
                    } else {
                        Kind::Inventory
                    },
                    map,
                )
            } else {
                return false;
            };
            let total = Menu::new(kind).slot_count();
            plain_map = Some(map);
            (kind, (0..total).map(|i| map.to_game(i)).collect())
        };
        // The plain map also takes the crafting result slot (a shift-click on it).
        let to_menu = |g: usize| match plain_map {
            Some(map) => map.to_menu(g),
            None => game_of.iter().position(|x| *x == Some(g)),
        };
        let mut menu = Menu::new(kind);
        if matches!(kind, Kind::Mount(_)) {
            menu.mount = self.mount_menu_entity();
        }
        let total = menu.slot_count();
        let before: Vec<Option<ItemStack>> = game_of
            .iter()
            .map(|g| g.and_then(|g| self.slot_mut(g).clone()))
            .collect();
        let carried = self.carried.clone();
        let creative = self.creative;
        // CreativeModeInventoryScreen clicks the menu itself and sends every
        // changed slot, which the server sets by player.
        let station_menu = self.station.as_ref().map(|s| {
            (
                s.loom.selected.map_or(-1, |i| i as i32),
                s.loom.patterns.clone(),
            )
        });
        let stone = self
            .station
            .as_ref()
            .map(|s| (s.selected.map_or(-1, |i| i as i32), s.stone.clone()));
        let crafter_data = match self.container_kind() {
            Some(BlockEntity::Crafter(c)) => Some((c.disabled, c.triggered)),
            _ => None,
        };
        // The merchant's session, lent to the menu for this click.
        let mut session = self
            .station
            .as_ref()
            .and_then(|s| s.merchant.as_ref())
            .map(|m| (m.session.clone(), m.mob));
        if let Some((s, mob)) = session.as_mut() {
            self.merchant_sync_in(s, *mob);
        }
        // The cartography table reads and makes the level's maps; a
        // server's (the client menu's ContainerLevelAccess.NULL) doesn't.
        let level_maps = (kind == Kind::Cartography).then(|| {
            let held: Vec<ItemStack> = before
                .iter()
                .flatten()
                .chain(self.inventory.iter().flatten())
                .chain(self.carried.iter())
                .cloned()
                .collect();
            self.level_maps(&held)
        });
        let next_map = if merchant_open { self.peek_map_id() } else { 0 };
        let tag = |t: &str| self.structure_tag(t);
        let maps = trading::GameMaps {
            terrain: self.end.locator(),
            structure_tag: &tag,
            made: Default::default(),
            next_id: std::cell::Cell::new(next_map),
        };
        let trade_env = crate::hc::merchant::TradeEnv {
            trades: &self.res.stations().trades,
            enchantments: &self.res.stations().enchantments,
            maps: Some(&maps),
        };
        let env = Env {
            items: self.res.items(),
            recipes: self.res.recipes(),
            brewing: &self.res.stations().brewing,
            banners: &self.res.stations().banners,
            station_recipes: &self.res.stations().recipes,
            enchantments: &self.res.stations().enchantments,
            names: &self.pack.names,
            trade: Some(&trade_env),
            maps: level_maps.as_ref().map(|m| m as &dyn crate::hc::menu::Maps),
        };
        menu.creative = creative;
        menu.selected = self.selected;
        menu.levels = self.experience.level;
        if anvil_open {
            // The rename box as AnvilScreen.onNameChanged gives it to the menu.
            menu.anvil.name = self.station.as_ref().map(|s| self.anvil_item_name(s));
        }
        for (i, stack) in before.iter().enumerate() {
            menu.set_slot(&env, i, stack.clone());
        }
        // The off hand is part of the inventory even when the menu has no slot for it.
        let offhand_mapped = game_of.contains(&Some(504));
        if !offhand_mapped {
            menu.set_player_item(40, self.offhand.clone());
        }
        menu.settle(&env);
        if kind == Kind::Loom
            && let Some((selected, patterns)) = station_menu
        {
            // The menu's state between clicks: what set_slot recomputed is replaced.
            menu.data[0] = selected;
            menu.loom_patterns = patterns;
            menu.set_slot(&env, 3, before[3].clone());
        }
        if kind == Kind::Stonecutter
            && let Some((selected, state)) = stone
        {
            // The same: the list and selection between clicks, then the result.
            menu.data[0] = selected;
            menu.stone = state;
            menu.set_slot(&env, 1, before[1].clone());
        }
        if kind == Kind::Enchanting
            && let Some(s) = self.station.as_ref()
        {
            // The offers between clicks (what copying the slots in flagged is not a change).
            menu.enchant_changed = false;
            for i in 0..3 {
                menu.data[i] = s.offers.costs[i];
                menu.data[7 + i] = s.offers.clues[i].as_ref().map_or(-1, |(_, l)| *l as i32);
            }
            menu.data[3] = self.level.xp_seed;
        }
        if let Some((disabled, triggered)) = crafter_data {
            for (i, d) in disabled.iter().enumerate() {
                menu.data[i] = i32::from(*d);
            }
            menu.data[9] = i32::from(triggered);
        }
        menu.carried = carried;
        menu.drops.clear();
        if kind == Kind::Merchant {
            // Attached after the slots were set: setting them is not a click.
            menu.merchant = session.map(|(s, _)| s);
        }
        // `FurnaceResultSlot.checkTakeAchievements` pays out on what left the output slot.
        let furnace_before = if matches!(kind, Kind::Furnace(_)) {
            menu.slot(2).cloned().map_or(0, |s| s.count)
        } else {
            0
        };
        let outcome = run(&mut menu, &env, &to_menu);
        let made = maps.made.take();
        match outcome {
            None => return false,
            Some(false) => return true,
            Some(true) => {}
        }
        // On a server the clicks only predict: the server drops, crafts and
        // makes maps itself.
        let predicting = false;
        if !predicting {
            self.register_exploration_maps(made);
        }
        if let Some(s) = menu.merchant.take() {
            self.merchant_sync_out(s);
        }
        let after: Vec<Option<ItemStack>> = (0..total).map(|i| menu.slot(i).cloned()).collect();
        // Only a furnace has a result slot to compare: an enchanting table has two slots.
        let furnace_after = if matches!(kind, Kind::Furnace(_)) {
            after[2].as_ref().map_or(0, |s| s.count)
        } else {
            0
        };
        let carried = menu.carried.take();
        let dropped = std::mem::take(&mut menu.dropped);
        let sounds = std::mem::take(&mut menu.sounds);
        // A bundle's sounds are the player's own (`Entity.playSound`): the
        // client plays them as it clicks, predicting or not.
        for sound in std::mem::take(&mut menu.player_sounds) {
            self.player_menu_sound(sound);
        }
        let crafted = std::mem::take(&mut menu.crafted);
        let brewed = std::mem::take(&mut menu.brewed);
        let equipped = std::mem::take(&mut menu.equipped);
        for (i, stack) in after.into_iter().enumerate() {
            if let Some(g) = game_of[i] {
                *self.slot_mut(g) = stack;
            }
        }
        self.carried = carried;
        if !offhand_mapped {
            self.offhand = menu.player_item(40).cloned();
        }
        if !predicting {
            for stack in dropped {
                self.player_drop(stack);
            }
            self.award_crafted(&crafted);
            for stack in &brewed {
                self.trigger_brewed_potion(stack);
            }
            if furnace_before > furnace_after
                && let (Some(pos), Some(p)) = (self.container, self.player.as_ref())
            {
                let at = [p.position.x, p.position.y, p.position.z];
                self.award_furnace_recipes(pos, at);
            }
        }
        if kind == Kind::Loom {
            if let Some(s) = self.station.as_mut() {
                s.loom.selected = usize::try_from(menu.data[0]).ok();
                s.loom.patterns = std::mem::take(&mut menu.loom_patterns);
            }
            // A server's loom plays its own sounds (the client's menu has no level).
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
            self.loom_screen_changed();
        } else if kind == Kind::Cartography {
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
        } else if kind == Kind::Stonecutter {
            if let Some(s) = self.station.as_mut() {
                s.selected = usize::try_from(menu.data[0]).ok();
                s.stone = std::mem::take(&mut menu.stone);
                s.stonecutter_changed();
            }
            for sound in sounds.into_iter().filter(|_| !predicting) {
                self.station_sound(sound);
            }
        }
        if let Some(amount) = menu.ground.take().filter(|_| !predicting)
            && let Some(pos) = self.station.as_ref().map(|s| s.pos)
        {
            self.grindstone_taken(pos, amount);
        }
        if kind == Kind::Anvil || kind == Kind::Smithing {
            self.combiner_session_end(&menu, predicting);
        }
        if kind == Kind::Enchanting && (menu.enchant_changed || before[0] != menu.slot(0).cloned())
        {
            // EnchantmentMenu.slotsChanged: new offers for the item.
            self.station_changed();
        }
        if kind == Kind::CrafterBlock
            && let Some(BlockEntity::Crafter(c)) = self
                .container
                .and_then(|k| self.world.block_entities.get_mut(&k))
        {
            for (i, d) in c.disabled.iter_mut().enumerate() {
                *d = menu.data[i] == 1;
            }
        }
        if matches!(kind, Kind::Furnace(_))
            && let Some(BlockEntity::Furnace(f)) = self
                .container
                .and_then(|k| self.world.block_entities.get_mut(&k))
        {
            // AbstractFurnaceBlockEntity.setItem: only a new input restarts cooking.
            f.input_clicked(self.res.recipes(), before[0].clone());
        }
        for (slot, stack) in equipped {
            self.player_equipped(slot, stack.as_ref());
        }
        self.apply_equipment();
        if let Some(maps) = level_maps {
            self.register_made_maps(maps);
        }
        true
    }
    /// `LevelRenderer.destroyBlockProgress`: a player's crack on a block (0..=9), or none
    /// (any other progress). Cracks not updated for 400 ticks are dropped.
    pub(crate) fn destroy_block_progress(&mut self, breaker: i32, pos: [i32; 3], progress: i32) {
        let now = self.world.game_time;
        self.destroying
            .retain(|_, c| now.saturating_sub(c.2) <= 400);
        if (0..10).contains(&progress) {
            self.destroying.insert(breaker, (pos, progress as u8, now));
        } else {
            self.destroying.remove(&breaker);
        }
    }
    fn sky_darken(&self) -> i32 {
        if self.dim == Dim::Nether {
            return 11;
        }
        if self.dim == Dim::End {
            let sky = self
                .res
                .end_environment()
                .sample(self.world.day_time, None, None);
            return (15.0 - 15.0 * sky.sky_light_level).round() as i32;
        }
        let (rain, thunder) = self.world.weather.levels(1.0);
        // Sampled once per (time, weather): every mob asks each tick.
        let key = (self.world.day_time, rain.to_bits(), thunder.to_bits());
        if let Some((k, darken)) = self.sky_darken_memo.get()
            && k == key
        {
            return darken;
        }
        let mut sky = self
            .res
            .environment()
            .sample(self.world.day_time, None, None);
        crate::hc::environment::apply_weather(&mut sky, rain, thunder);
        // Level.updateSkyBrightness: darkening from the sky light level.
        let darken = (15.0 - 15.0 * sky.sky_light_level).round() as i32;
        self.sky_darken_memo.set(Some((key, darken)));
        darken
    }
    fn mob_hit(&self) -> Option<(usize, f64)> {
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let reach: f64 = if self.creative { 5.0 } else { 3.0 };
        let mut best: Option<(usize, f64)> = None;
        for (i, m) in self.mobs.iter().enumerate() {
            // ProjectileUtil.getEntityHitResult: the mob the player rides
            // shares its root vehicle and is never the hit.
            if !m.alive() || Some(m.id) == self.riding {
                continue;
            }
            if let Some(t) = ray_box(eye, dir, reach, m.bounds())
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t));
            }
        }
        best
    }
    /// `Player.attack`'s damage against `target_kind` (with the attack
    /// strength and whether it is a critical hit): the weapon and effects
    /// scaled by the attack strength, a mace's smash, the critical hit, then
    /// enchantments.
    pub(super) fn melee_damage(&mut self, target_kind: &str) -> (f32, f32, bool, bool) {
        let hand_damage = 1.0 + self.attack_item_info().map_or(0.0, |i| i.attack_damage);
        // Riptide's spin replaces the attack damage attribute.
        let spin = self.player.as_ref().map_or(0.0, |p| p.auto_spin_attack_dmg);
        let damage = if spin > 0.0 {
            spin
        } else {
            hand_damage + self.effect_attack_bonus()
        };
        let scale = self.attack_strength_scale();
        let mut amount = damage * (0.2 + scale * scale * 0.8);
        // Item.getAttackDamageBonus (a mace's smash), before the critical hit.
        amount += self.smash_damage_bonus(target_kind);
        let player = self.player.as_ref();
        // Player.canCriticalAttack: falling, off the ground, not in water,
        // not blind (isMobilityRestricted), not riding, not sprinting.
        let falling = player.is_some_and(|p| {
            !p.ground.on_ground
                && p.fall_distance > 0.0
                && !p.fluids.was_touching_water
                && !p.sprinting
        });
        let restricted = self.potions.effects.get("blindness").is_some() || self.riding.is_some();
        let crit = scale > 0.9 && falling && !restricted;
        if crit {
            amount *= 1.5;
        }
        // Enchantment damage (sharpness, smite, bane) scaled by the attack strength.
        let magic_boost = self.enchant_damage_bonus(target_kind) * scale;
        amount += magic_boost;
        // A plugin's EntityDamageEvent changed the hit under way.
        (amount, scale, crit, magic_boost > 0.0)
    }
    /// `Player.getAttackStrengthScale(0.5)`.
    fn attack_strength_scale(&self) -> f32 {
        let delay = crate::hc::attack_strength::delay(self.main_hand_attack_speed());
        ((self.attack_ticks as f32 + 0.5) / delay).clamp(0.0, 1.0)
    }
    /// `Player.isSweepAttack`: a charged, grounded sword hit below the movement limit.
    pub(super) fn sweep_attack_position(&self, scale: f32, crit: bool) -> Option<[f64; 3]> {
        let p = self.player.as_ref()?;
        let sword = self.held().is_some_and(|s| {
            self.res
                .items()
                .item_tags
                .get("minecraft:swords")
                .is_some_and(|tag| tag.contains(&*s.item))
        });
        if !p.is_sweep_attack(scale, crit, sword) {
            return None;
        }
        let yaw = f64::from(self.yaw.to_radians());
        Some([
            p.position.x - yaw.sin(),
            p.position.y + f64::from(p.dimensions.height) * 0.5,
            p.position.z + yaw.cos(),
        ])
    }
    /// The visual and sound part of `Player.doSweepAttack` after a successful hit.
    pub(super) fn sweep_attack(&mut self, at: [f64; 3], from: Option<[f64; 3]>) {
        let sweep = crate::hc::particles::ParticleOptions::simple(
            crate::hc::particles::ParticleType::SweepAttack,
        );
        self.particles.add(&sweep, at, [0.0; 3]);
        // Player.sweepAttack's `sendParticles(SWEEP_ATTACK, ..., 0, dx, 0, dz, 0)`:
        // the offset is the one from the player to the particle.
        let origin = from.unwrap_or(at);
        let spread = [at[0] - origin[0], 0.0, at[2] - origin[2]];
        self.lan_send_particles(&sweep, at, spread, 0.0, 0);
        self.sounds
            .play("minecraft:entity.player.attack.sweep", from, 1.0, 1.0);
    }
    /// What `Player.doSweepAttack` deals before the target's enchantment
    /// bonus and the attack strength: `1 + sweeping_damage_ratio * baseDamage`.
    pub(super) fn sweep_base_damage(&self, scale: f32) -> f32 {
        // `baseDamage` of `Player.attack` before a critical hit (a sweep has
        // none) and the enchantments: the attack damage attribute with the
        // strength and weakness effects, scaled by the attack strength.
        let hand_damage = 1.0 + self.attack_item_info().map_or(0.0, |i| i.attack_damage);
        let base_damage = (hand_damage + self.effect_attack_bonus()) * (0.2 + scale * scale * 0.8);
        // The enchantment's attribute modifier on the attribute's 0 to 1 range.
        let ratio = self
            .res
            .stations()
            .enchantments
            .attribute_add(
                self.inventory[self.selected].as_ref(),
                "minecraft:sweeping_damage_ratio",
            )
            .clamp(0.0, 1.0) as f32;
        1.0 + ratio * base_damage
    }
    /// The damage and knockback part of `Player.doSweepAttack` after a sweep
    /// hit on the mob at `target`: every other living mob in its box grown by
    /// (1, 0.25, 1) and closer than 3 blocks (`distanceToSqr < 9`, in creative
    /// too), but the player's allies
    /// (`isAlliedTo`), takes `1 + sweeping_damage_ratio * baseDamage` with the
    /// weapon's enchantments, scaled by the attack strength, and is pushed 0.4
    /// along the player's yaw.
    pub(super) fn sweep_damage(&mut self, target: usize, scale: f32) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let from = [p.position.x, p.position.y, p.position.z];
        let sweep_damage = self.sweep_base_damage(scale);
        let hit_box = self.mobs[target]
            .bounds()
            .inflate(horizon_core::geometry::Vec3::new(1.0, 0.25, 1.0));
        let (uuid, allies) = (
            self.level.player_uuid.or(Some([0, 0, 0, 1])),
            self.player_allies(),
        );
        let nearby: Vec<usize> = self
            .mobs
            .iter()
            .enumerate()
            .filter(|(i, m)| {
                *i != target
                    && m.alive()
                    && !m.removed
                    && !allies.contains(&m.id)
                    && !mace::ally_of_player(m, uuid)
                    && m.bounds().intersects(hit_box)
                    && m.position.distance_squared(p.position) < 9.0
            })
            .map(|(i, _)| i)
            .collect();
        let (xd, zd) = self.extra_knockback_direction();
        for i in nearby {
            let kind = self.mobs[i].kind.id.clone();
            let kind = format!("minecraft:{}", kind.trim_start_matches("minecraft:"));
            let damage = (sweep_damage + self.enchant_damage_bonus(&kind)) * scale;
            let source = self.attack_source(from);
            if self.hurt_by_player(i, damage, source) {
                self.mobs[i].knockback(0.4, xd, zd);
                self.last_hurt_by_player
                    .insert(self.mobs[i].id, self.lan_acting);
            }
        }
        // `getEntitiesOfClass(LivingEntity.class)` takes the other players too.
        let mut sweep = self.player_sweep(sweep_damage, scale);
        sweep.hit_box = hit_box;
        self.sweep_other_players(sweep);
    }
    /// `Player.attack`'s sounds when a vehicle takes the hit (it always does,
    /// and can't be hit critically): knockback when sprinting at full
    /// strength, then strong or weak. Before `swing_arm` resets the charge.
    pub(super) fn vehicle_attack_sounds(&mut self) {
        let full = self.attack_strength_scale() > 0.9;
        let player = self.player.as_ref();
        let from = player.map(|p| [p.position.x, p.position.y, p.position.z]);
        if full && player.is_some_and(|p| p.sprinting) {
            self.sounds
                .play("minecraft:entity.player.attack.knockback", from, 1.0, 1.0);
        }
        let sound = if full {
            "minecraft:entity.player.attack.strong"
        } else {
            "minecraft:entity.player.attack.weak"
        };
        if let Some(at) = self.sweep_attack_position(self.attack_strength_scale(), false) {
            self.sweep_attack(at, from);
        } else {
            self.sounds.play(sound, from, 1.0, 1.0);
        }
    }
    /// Player.attack with the attack-strength charge, crits and knockback.
    fn attack(&mut self) -> bool {
        if self.spectator() {
            return false;
        }
        // A painting nearer than the targeted block and mob breaks instead.
        if let Some((i, d)) = self.painting_hit()
            && self.mob_hit().is_none_or(|m| d < m.1)
            && self.pick().is_none_or(|h| {
                let eye = self.eye_position().unwrap_or_default();
                (0..3)
                    .map(|a| (h.point[a] - eye[a]).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    > d
            })
        {
            self.break_painting(i, true);
            return true;
        }
        // A ghast's fireball or a wind charge in front: turned back.
        if self.attack_redirectable() {
            return true;
        }
        let hand_damage = 1.0
            + self
                .held()
                .and_then(|h| self.res.items().get(&h.item))
                .map_or(0.0, |i| i.attack_damage);
        if self.attack_frame()
            || self.attack_armor_stand()
            || self.attack_cushion()
            || self.attack_boat(hand_damage)
            || self.attack_cart(hand_damage)
            || self.attack_dragon()
            || self.attack_guest()
        {
            return true;
        }
        let Some((index, distance)) = self.mob_hit() else {
            return false;
        };
        if let Some(hit) = self.pick() {
            let eye = self.eye_position().unwrap_or_default();
            let d = ((hit.point[0] - eye[0]).powi(2)
                + (hit.point[1] - eye[1]).powi(2)
                + (hit.point[2] - eye[2]).powi(2))
            .sqrt();
            if d < distance {
                return false;
            }
        }
        self.attack_mob(index);
        // Minecraft.startAttack swings after the attack.
        self.swing_arm();
        true
    }
    /// `Player.attack` on a mob: the damage, knockback, sweep, particles and
    /// sounds.
    pub(super) fn attack_mob(&mut self, index: usize) {
        let target_kind = self.mobs[index].kind.id.clone();
        let target_kind = format!("minecraft:{}", target_kind.trim_start_matches("minecraft:"));
        let (amount, scale, crit, magic) = self.melee_damage(&target_kind);
        let sweep = self.sweep_attack_position(scale, crit);
        let (knockback, knockback_attack) = self.extra_knockback(&target_kind, scale);
        self.attack_ticks = 0;
        let player = self.player.as_ref();
        let from = player.map(|p| [p.position.x, p.position.y, p.position.z]);
        if knockback_attack {
            self.sounds
                .play("minecraft:entity.player.attack.knockback", from, 1.0, 1.0);
        }
        // LivingEntity.setLastHurtMob (tamed wolves join in).
        self.combat.last_hurt_mob = Some((self.mobs[index].id, self.world.game_time));
        let old_health = self.mobs[index].health;
        let source = match from {
            Some(f) => self.attack_source(f),
            None => crate::hc::mobs::Damage::of(crate::hc::mobs::DamageKind::Generic),
        };
        let landed = self.hurt_by_player(index, amount, source);
        if landed {
            let victim = self.mob_entity(&self.mobs[index]);
            self.player_hit(
                &victim,
                creatures::damage_type(source.kind, true),
                None,
                amount,
            );
        }
        if let Some(p) = self.player.as_mut() {
            if p.attack_log.len() >= 64 {
                p.attack_log.remove(0);
            }
            p.attack_log.push(crate::hc::health::AttackRecord {
                game_time: p.health.game_time(),
                target: target_kind.clone(),
                id: self.mobs[index].id,
                killed: landed && !self.mobs[index].alive(),
                amount,
                scale,
                crit,
                landed,
                dealt: old_health - self.mobs[index].health,
            });
        }
        if landed {
            if knockback > 0.0 {
                let (xd, zd) = self.extra_knockback_direction();
                self.mobs[index].knockback(f64::from(knockback), xd, zd);
                self.stop_sprint_after_extra_knockback();
            }
            let mob = &self.mobs[index];
            // Player.damageStatsAndHearts: the damage the hit actually did.
            let actual = old_health - mob.health;
            // Player.attack: crit emitter and damage indicator hearts.
            let (feet, width, height) = (
                [mob.position.x, mob.position.y, mob.position.z],
                mob.kind.width,
                mob.kind.height,
            );
            self.stats
                .custom("damage_dealt", (actual * 10.0).round() as i32);
            if crit {
                let (acting, target) = (self.lan_acting, self.mobs[index].id as i32);
                self.critical_hit(acting, target, feet, width, height, false);
            }
            // Player.attackVisualEffects: magicCrit when the enchantments added damage.
            if magic {
                let (acting, target) = (self.lan_acting, self.mobs[index].id as i32);
                self.critical_hit(acting, target, feet, width, height, true);
            }
            if actual > 2.0 {
                let indicator = crate::hc::particles::ParticleOptions::simple(
                    crate::hc::particles::ParticleType::DamageIndicator,
                );
                self.particles.send_particles(
                    &indicator,
                    [feet[0], feet[1] + height * 0.5, feet[2]],
                    [0.1, 0.0, 0.1],
                    0.2,
                    (actual * 0.5) as u32,
                );
                // The players a server serves are sent them too.
                self.lan_send_particles(
                    &indicator,
                    [feet[0], feet[1] + height * 0.5, feet[2]],
                    [0.1, 0.0, 0.1],
                    0.2,
                    (actual * 0.5) as u32,
                );
            }
            self.last_hurt_by_player
                .insert(self.mobs[index].id, self.lan_acting);
            let sound = if crit {
                "minecraft:entity.player.attack.crit"
            } else if scale > 0.9 {
                "minecraft:entity.player.attack.strong"
            } else {
                "minecraft:entity.player.attack.weak"
            };
            // Player.playServerSideSound: at the player.
            if let Some(at) = sweep {
                self.sweep_attack(at, from);
                self.sweep_damage(index, scale);
            } else {
                self.sounds.play(sound, from, 1.0, 1.0);
            }
            // The hurt or death sound the mob raised (getHurtSound / getDeathSound,
            // its voice pitch from its own random).
            let queued = std::mem::take(&mut self.mobs[index].outbox);
            let mut infested = false;
            for event in queued {
                match event {
                    crate::hc::mobs::MobEvent::Sound(s, at, volume, pitch) => {
                        self.sounds
                            .play(&format!("minecraft:{s}"), Some(at), volume, pitch);
                    }
                    crate::hc::mobs::MobEvent::Infested { .. } => infested = true,
                    other => {
                        // Left for the end of the tick.
                        self.mobs[index].outbox.push(other);
                    }
                }
            }
            if infested {
                let id = self.mobs[index].id;
                self.mob_infested(id);
            }
            self.weapon_hit(index);
            // Player.attack: causeFoodExhaustion(0.1F) only when the hit hurt
            // (inside `if (wasHurt)`), after damageStatsAndHearts.
            if let Some(p) = self.player.as_mut()
                && !self.creative
            {
                p.cause_food_exhaustion(0.1);
            }
        } else {
            self.sounds
                .play("minecraft:entity.player.attack.nodamage", from, 1.0, 1.0);
        }
    }
    /// ServerExplosion: rays from a 16^3 grid lose (resistance + 0.3) * 0.3 per step.
    /// A mob explosion (`ServerLevel.explode` with `ExplosionInteraction.MOB`:
    /// blocks are kept without mobGriefing, else the mob_explosion_drop_decay
    /// rule picks decay).
    fn explode(&mut self, center: [f64; 3], radius: f32) -> Result<()> {
        let interaction = if !self.world_data.rules.bool("mob_griefing") {
            explosion::BlockInteraction::Keep
        } else if self.world_data.rules.bool("mob_explosion_drop_decay") {
            explosion::BlockInteraction::DestroyWithDecay
        } else {
            explosion::BlockInteraction::Destroy
        };
        self.server_explode(center, radius, interaction, false)
    }
    /// Copies of a save batch carrying its live block entities and save data.
    fn with_entities<'a>(
        &self,
        chunks: impl IntoIterator<Item = &'a Arc<crate::hc::chunk::Chunk>>,
    ) -> Vec<Arc<crate::hc::chunk::Chunk>> {
        crate::hc::save::chunks_with_block_entities(
            chunks,
            &self.world.block_entities,
            &self.world.block_names,
        )
        .into_iter()
        .map(|mut c| {
            self.attach_save_data(&mut c);
            Arc::new(c)
        })
        .collect()
    }
    fn tick_mobs(&mut self) -> Result<()> {
        self.tick_creatures()?;
        self.position_rider();
        self.tick_stats();
        self.tick_scoreboard();
        self.load_bossbars();
        self.tick_functions();
        self.tick_worldborder();
        self.spawn_phantoms();
        self.tick_advancements();
        Ok(())
    }
    /// PortalShape: an obsidian frame around 2..=21 by 3..=21 empty blocks.
    fn portal_shape(&self, start: [i32; 3], x_axis: bool) -> Option<([i32; 3], i32, i32)> {
        let name = |p: [i32; 3]| {
            self.res
                .facts()
                .get(self.world.block(p[0], p[1], p[2]))
                .name
                .clone()
        };
        let empty = |p: [i32; 3]| {
            let n = name(p);
            matches!(
                &*n,
                "minecraft:air"
                    | "minecraft:cave_air"
                    | "minecraft:fire"
                    | "minecraft:soul_fire"
                    | "minecraft:nether_portal"
            )
        };
        let frame = |p: [i32; 3]| &*name(p) == "minecraft:obsidian";
        let d = if x_axis { [1, 0, 0] } else { [0, 0, 1] };
        let mut bottom = start;
        let mut n = 0;
        while empty([bottom[0], bottom[1] - 1, bottom[2]]) && n < 21 {
            bottom[1] -= 1;
            n += 1;
        }
        if !frame([bottom[0], bottom[1] - 1, bottom[2]]) {
            return None;
        }
        let mut left = bottom;
        n = 0;
        while empty([left[0] - d[0], left[1], left[2] - d[2]]) && n < 21 {
            left = [left[0] - d[0], left[1], left[2] - d[2]];
            n += 1;
        }
        if !frame([left[0] - d[0], left[1], left[2] - d[2]]) {
            return None;
        }
        let mut width = 0;
        while width <= 21 {
            let p = [left[0] + d[0] * width, left[1], left[2] + d[2] * width];
            if !empty(p) || !frame([p[0], p[1] - 1, p[2]]) {
                break;
            }
            width += 1;
        }
        if !(2..=21).contains(&width)
            || !frame([left[0] + d[0] * width, left[1], left[2] + d[2] * width])
        {
            return None;
        }
        let mut height = 0;
        'rows: while height <= 21 {
            let y = left[1] + height;
            if !frame([left[0] - d[0], y, left[2] - d[2]])
                || !frame([left[0] + d[0] * width, y, left[2] + d[2] * width])
            {
                break;
            }
            for i in 0..width {
                if !empty([left[0] + d[0] * i, y, left[2] + d[2] * i]) {
                    break 'rows;
                }
            }
            height += 1;
        }
        let top_ok =
            (0..width).all(|i| frame([left[0] + d[0] * i, left[1] + height, left[2] + d[2] * i]));
        ((3..=21).contains(&height) && top_ok).then_some((left, width, height))
    }
    /// BaseFireBlock.onPlace: lighting fire inside a frame creates a portal,
    /// only in the Overworld and the Nether (`inPortalDimension`).
    fn try_light_portal(&mut self, at: [i32; 3]) -> Result<bool> {
        if !matches!(self.dim, Dim::Overworld | Dim::Nether) {
            return Ok(false);
        }
        for x_axis in [true, false] {
            if let Some((origin, width, height)) = self.portal_shape(at, x_axis) {
                let d = if x_axis { [1, 0, 0] } else { [0, 0, 1] };
                let registry = &self.pack.assets.registry;
                let Some(default) = registry.default_state("minecraft:nether_portal") else {
                    return Ok(false);
                };
                let mut portal = registry.state(default).cloned().ok_or("portal state")?;
                portal
                    .properties
                    .insert("axis".into(), if x_axis { "x" } else { "z" }.into());
                let id = registry.resolve(&portal).unwrap_or(default);
                for h in 0..height {
                    for w in 0..width {
                        self.world.set(
                            origin[0] + d[0] * w,
                            origin[1] + h,
                            origin[2] + d[2] * w,
                            id,
                        )?;
                    }
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Let go of every entity of the level (saved first): the next world
    /// opened must not inherit them, nor save them as its own.
    fn clear_entities(&mut self) {
        self.mobs.clear();
        self.dropped.clear();
        self.orbs.clear();
        self.creatures.projectiles.clear();
        self.falling.clear();
        self.tnts.clear();
        self.boats.clear();
        self.carts.clear();
        self.armor_stands.clear();
        self.cushions.clear();
        self.item_frames.clear();
        self.paintings.clear();
        self.others.clear();
        self.dragon = None;
        self.crystals.clear();
        self.dragon_fireballs.clear();
        self.breath_clouds.clear();
        self.tridents.clear();
        self.rockets.clear();
        self.bolts.clear();
        self.particles.clear();
        self.riding = None;
        self.persist = Default::default();
    }
    /// Equipment attribute modifiers (LivingEntity.collectEquipmentChanges).
    fn apply_equipment(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        for (attribute, id) in std::mem::take(&mut self.applied_modifiers) {
            let _ = player.set_modifier(&attribute, &id, 0.0, "add_value", false);
        }
        let slots = ["head", "chest", "legs", "feet"];
        let mut equipped: Vec<(Option<&ItemStack>, &str)> = self
            .armor
            .iter()
            .zip(slots)
            .map(|(s, n)| (s.as_ref(), n))
            .collect();
        equipped.push((self.inventory[self.selected].as_ref(), "mainhand"));
        for (stack, slot) in equipped {
            let Some(info) = stack.and_then(|s| self.res.items().get(&s.item)) else {
                continue;
            };
            for (attribute, id, amount, op, group) in &info.modifiers {
                let matches = group == slot
                    || group == "any"
                    || (group == "armor" && slot != "mainhand")
                    || (group == "hand" && slot == "mainhand");
                // Main-hand attack modifiers are read by the attack code directly.
                if !matches || slot == "mainhand" {
                    continue;
                }
                if player
                    .set_modifier(attribute, id, *amount, op, true)
                    .is_ok()
                {
                    self.applied_modifiers.push((attribute.clone(), id.clone()));
                }
            }
        }
        self.apply_enchantment_attributes();
    }
    /// The armor hits among a tick's motion events (`Player.hurtArmor` from
    /// `getDamageAfterArmorAbsorb`), whatever the damage: a mob, an explosion,
    /// a cactus, lava, a campfire.
    fn wear_armor(&mut self, events: &[MotionEvent]) {
        for event in events {
            if let MotionEvent::Health(crate::hc::health::HealthEvent::ArmorHurt {
                source,
                amount,
            }) = event
            {
                self.hurt_armor(*source, *amount);
            }
        }
    }
    /// `Player.hurtArmor` -> `LivingEntity.doHurtEquipment`: each worn piece that
    /// wears when its wearer is hurt loses max(1, damage / 4) durability, except
    /// to a fire source when the piece resists fire (`ItemStack.canBeHurtBy`).
    fn hurt_armor(&mut self, source: horizon_world::damage_source::DamageTypeId, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        let amount = ((damage / 4.0) as u32).max(1);
        let name = self
            .motion
            .health
            .damage
            .definition(source)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        let fire = self
            .motion
            .health
            .damage
            .in_tag(source, "minecraft:is_fire");
        let mut broke = false;
        let losses: Vec<u32> = (0..4)
            .map(|i| match self.armor[i].clone() {
                Some(s)
                    if self
                        .res
                        .items()
                        .get(&s.item)
                        .is_some_and(|info| info.equip_damage_on_hurt)
                        && !(fire && self.res.items().resists_fire(&s, &name)) =>
                {
                    self.durability_loss(&s, amount)
                }
                _ => 0,
            })
            .collect();
        for (slot, amount) in self.armor.iter_mut().zip(losses) {
            if let Some(stack) = slot.as_mut()
                && let Some(info) = self.res.items().get(&stack.item)
                && info.max_damage > 0
            {
                stack.damage += amount;
                if stack.damage >= info.max_damage {
                    *slot = None;
                    broke = true;
                }
            }
        }
        if broke {
            if let Some(eye) = self.eye_position() {
                self.sounds.play(
                    "minecraft:entity.item.break",
                    Some(eye),
                    0.8,
                    0.8 + self.world.random.next_float() * 0.4,
                );
            }
            self.apply_equipment();
        }
    }
    /// Voxel traversal that also stops at fluid source blocks (bucket pick).
    fn pick_fluid(&self) -> Option<Hit> {
        let origin = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let mut t = 0.0;
        let mut last = origin.map(|v| v.floor() as i32);
        while t <= self.reach() {
            let p = [
                origin[0] + dir[0] * t,
                origin[1] + dir[1] * t,
                origin[2] + dir[2] * t,
            ];
            let b = p.map(|v| v.floor() as i32);
            if b != last {
                let state = self.world.block(b[0], b[1], b[2]);
                let f = self.res.facts().get(state);
                let face = [last[0] - b[0], last[1] - b[1], last[2] - b[2]];
                if f.fluid != Fluid::None
                    && f.fluid_level == 8
                    && !f.fluid_falling
                    && !f.waterlogged
                    && f.render_shape != crate::hc::blocks::RenderShape::Model
                {
                    return Some(Hit {
                        pos: b,
                        face,
                        point: p,
                    });
                }
                if !self.res.facts().is_air(state) && f.fluid == Fluid::None {
                    return Some(Hit {
                        pos: b,
                        face,
                        point: p,
                    });
                }
                last = b;
            }
            t += 0.05;
        }
        None
    }
    /// `CauldronInteractions` for the held item aimed at a cauldron.
    fn use_on_cauldron(&mut self, pos: [i32; 3]) -> Result<bool> {
        use crate::hc::cauldron::{self, Outcome};
        struct Hand<'g, 'a>(&'g mut crate::game::Sim<'a>);
        impl cauldron::Player for Hand<'_, '_> {
            fn creative(&self) -> bool {
                self.0.creative
            }
            fn hand(&mut self) -> &mut Option<ItemStack> {
                self.0.held_mut()
            }
            fn add(&mut self, stack: &mut ItemStack) -> bool {
                let before = stack.count;
                match self.0.give(stack.clone()) {
                    None => stack.count = 0,
                    Some(rest) => stack.count = rest.count,
                }
                stack.count < before
            }
            fn contains(&self, stack: &ItemStack) -> bool {
                self.0
                    .inventory
                    .iter()
                    .flatten()
                    .any(|have| have.same_item_same_components(stack))
            }
            fn drop_stack(&mut self, stack: ItemStack) {
                if let Some(eye) = self.0.eye_position() {
                    // ItemUtils.createFilledResult: `player.drop(stack, false)`.
                    self.0.throw_from_eyes(stack, eye, false);
                }
            }
        }
        let registry = self.pack.assets.registry;
        let id = self.world.block(pos[0], pos[1], pos[2]);
        let Some(state) = registry
            .state(id)
            .filter(|s| s.name.ends_with("cauldron"))
            .cloned()
        else {
            return Ok(false);
        };
        let Some(held) = self.held() else {
            return Ok(false);
        };
        let removes_dye = cauldron::removes_dye(self.res.items(), &held.item);
        let above = self.world.block(pos[0], pos[1] + 1, pos[2]);
        let under_water = registry
            .state(above)
            .is_some_and(|s| s.name == "minecraft:water");
        let result = cauldron::interact(&state, under_water, removes_dye, &mut Hand(self));
        if result.outcome == Outcome::Pass {
            return Ok(false);
        }
        if let Some(stat) = result.stat {
            self.stats.custom(stat, 1);
        }
        if let Some(block) = result.block
            && let Ok(new) = registry.resolve(&block)
        {
            self.level_set(pos, new)?;
        }
        if let Some(sound) = result.sound {
            self.sounds.play(
                sound,
                Some([
                    f64::from(pos[0]) + 0.5,
                    f64::from(pos[1]) + 0.5,
                    f64::from(pos[2]) + 0.5,
                ]),
                1.0,
                1.0,
            );
        }
        Ok(true)
    }
    /// What `BlockTransformer.transformBlock` would do with a held axe, hoe
    /// or shovel on the block at `pos`, without doing it: the new state, its
    /// sound, loot table, the durability it costs and its particle.
    pub(super) fn block_transform(
        &self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
    ) -> Option<(BlockStateId, String, Option<String>, u32, String)> {
        use crate::hc::block_transform::Around;
        struct Column<'w> {
            world: &'w World,
            registry: &'w horizon_world::registry::BlockRegistry,
            pos: [i32; 3],
        }
        impl Around for Column<'_> {
            fn state(&self, o: [i32; 3]) -> horizon_world::chunk::BlockState {
                let id =
                    self.world
                        .block(self.pos[0] + o[0], self.pos[1] + o[1], self.pos[2] + o[2]);
                self.registry.state(id).cloned().unwrap_or_else(|| {
                    horizon_world::chunk::BlockState {
                        name: "minecraft:air".into(),
                        properties: Default::default(),
                    }
                })
            }
        }
        let id = self
            .res
            .items()
            .get(&stack.item)
            .and_then(|i| i.block_transformer.clone())?;
        // playerHasBlockingItemUseIntent: a shield in the off hand is raised
        // instead, unless sneaking.
        let shield = self
            .offhand
            .as_ref()
            .and_then(|s| self.res.items().get(&s.item))
            .is_some_and(|i| i.blocks_attacks.is_some());
        if shield && !self.keys.contains(&KeyCode::ShiftLeft) {
            return None;
        }
        let face_name = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            _ => "east",
        };
        let registry = self.pack.assets.registry;
        let around = Column {
            world: &self.world,
            registry,
            pos,
        };
        let (_, t, state) =
            self.res
                .transformers()
                .apply(self.res.items(), registry, &id, face_name, &around)?;
        let new = registry.resolve(&state).ok()?;
        Some((
            new,
            t.sound.clone(),
            t.loot.clone(),
            t.damage,
            t.particle.clone(),
        ))
    }
    /// `BlockTransformer.TransformParticle.send`: its level event.
    pub(super) fn transform_particle(&mut self, particle: &str, pos: [i32; 3]) {
        match particle {
            "scrape" => self.level_event_at(3005, pos, 0),
            "wax_on" => self.level_event_at(3003, pos, 0),
            "wax_off" => self.level_event_at(3004, pos, 0),
            _ => {}
        }
    }
    /// `BlockTransformer.transformBlock` for a held axe, hoe or shovel.
    fn use_block_transformer(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
    ) -> Result<bool> {
        let Some((new, sound, loot, damage, particle)) = self.block_transform(stack, pos, face)
        else {
            return Ok(false);
        };
        // BlockTransformer.transformBlock: ITEM_USED_ON_BLOCK before the swap.
        self.trigger_block("minecraft:item_used_on_block", pos, stack);
        self.level_set(pos, new)?;
        // BlockTransformer.TransformParticle.send.
        self.transform_particle(&particle, pos);
        if loot.as_deref() == Some("minecraft:till/rooted_dirt") {
            self.pop_resource(
                [pos[0] + face[0], pos[1] + face[1], pos[2] + face[2]],
                ItemStack::new("minecraft:hanging_roots", 1),
            );
        }
        self.sounds.play(
            &sound,
            Some([
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ]),
            1.0,
            1.0,
        );
        // ItemStack.useOn: a use that acted counts as one of the item.
        self.stats.add("used", &stack.item, 1);
        self.damage_held(damage);
        self.post_block_game_event("minecraft:block_change", pos, self.sculk_actor(), Some(new));
        Ok(true)
    }
    fn set_state_named(&mut self, pos: [i32; 3], name: &str, keep: &[&str]) -> Result<bool> {
        let registry = &self.pack.assets.registry;
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let Some(default) = registry.default_state(name) else {
            return Ok(false);
        };
        let mut state = registry.state(default).cloned().ok_or("state")?;
        if let Some(previous) = registry.state(old) {
            for k in keep {
                if let Some(v) = previous.properties.get(*k) {
                    state.properties.insert((*k).to_owned(), v.clone());
                }
            }
        }
        let id = registry.resolve(&state).unwrap_or(default);
        self.level_set(pos, id)?;
        Ok(true)
    }
    /// `LayeredCauldronBlock.handleEntityOnFireInside` for a burning player
    /// in the cauldron at `pos` (`mayInteract`: not a spectator): the fill
    /// level drops (`lowerFillLevel`, a `block_change` with no source). A
    /// server's cauldron is the server's.
    pub(super) fn cauldron_extinguish(&mut self, pos: BlockPos) {
        let at = [pos.x, pos.y, pos.z];
        let registry = self.pack.assets.registry;
        let Some(old) = registry
            .state(self.world.block(at[0], at[1], at[2]))
            .filter(|s| {
                matches!(
                    s.name.as_str(),
                    "minecraft:water_cauldron" | "minecraft:powder_snow_cauldron"
                )
            })
            .cloned()
        else {
            return;
        };
        let Ok(new) = registry.resolve(&crate::hc::cauldron::extinguished(&old)) else {
            return;
        };
        if self.level_set(at, new).is_ok() {
            self.post_block_game_event("minecraft:block_change", at, None, Some(new));
        }
    }
    fn damage_held(&mut self, amount: u32) {
        if self.creative {
            return;
        }
        let amount = match self.held().cloned() {
            Some(held) => self.durability_loss(&held, amount),
            None => amount,
        };
        let max = self
            .held()
            .and_then(|s| self.res.items().get(&s.item))
            .map_or(0, |i| i.max_damage);
        let cell = self.held_mut();
        if let Some(stack) = cell.as_mut()
            && max > 0
        {
            stack.damage += amount;
            if stack.damage >= max {
                let broken = stack.item.clone();
                *cell = None;
                self.stats.add("broken", &broken, 1);
                if let Some(eye) = self.eye_position() {
                    self.sounds.play(
                        "minecraft:entity.item.break",
                        Some(eye),
                        0.8,
                        0.8 + self.world.random.next_float() * 0.4,
                    );
                }
            }
        }
    }
    /// `ItemStack.hurtWithoutBreaking`: Unbreaking applies, but the stack stops at
    /// one point of durability left instead of breaking.
    fn hurt_held_without_breaking(&mut self, amount: u32) {
        if self.creative {
            return;
        }
        let Some(held) = self.held().cloned() else {
            return;
        };
        let amount = self.durability_loss(&held, amount);
        let max = self.res.items().get(&held.item).map_or(0, |i| i.max_damage);
        if amount == 0 || max == 0 {
            return;
        }
        if let Some(stack) = self.held_mut().as_mut() {
            stack.damage = (stack.damage + amount).min(max - 1);
        }
    }
    /// Block tool use: distinguish success, failure and fall-through for hand feedback.
    fn use_tool(
        &mut self,
        stack: &ItemStack,
        allow_block_actions: bool,
    ) -> Result<crate::hc::first_person::UseResult> {
        use crate::hc::first_person::UseResult;
        let item = stack.item.trim_start_matches("minecraft:").to_owned();
        let at = |p: [i32; 3]| {
            [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + 0.5,
                f64::from(p[2]) + 0.5,
            ]
        };
        if allow_block_actions
            && let Some(hit) = self.pick()
            && self.use_on_cauldron(hit.pos)?
        {
            return Ok(UseResult::Success { swing: true });
        }
        if allow_block_actions
            && let Some(hit) = self.pick()
            && let Some(placed) = self.use_on_campfire(hit.pos)?
        {
            // SUCCESS_SERVER swings; a full campfire's CONSUME does not.
            return Ok(UseResult::Success { swing: placed });
        }
        // BucketItem has no useOn: a guest's UseItemOn leaves it to UseItem.
        let bucket = crate::hc::items::bucket_contents(&stack.item);
        if matches!(self.guest_use, Some(GuestUse::OnBlock(_)))
            && (item == "bucket" || bucket.is_some())
        {
            return Ok(UseResult::Pass);
        }
        if item == "bucket" {
            // getPlayerPOVHitResult(SOURCE_ONLY): powder snow's outline before
            // a fluid source behind it.
            if let Some(hit) = self.pick()
                && self.is_powder_snow(hit.pos)
            {
                let eye = self.eye_position().unwrap_or(hit.point);
                let d = |p: [f64; 3]| {
                    (p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)
                };
                if self
                    .pick_fluid()
                    .is_none_or(|fluid| d(fluid.point) >= d(hit.point))
                {
                    self.pick_up_powder_snow(hit.pos)?;
                    return Ok(UseResult::Success { swing: true });
                }
            }
            let Some(hit) = self.pick_fluid() else {
                return Ok(UseResult::Pass);
            };
            let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
            let f = self.res.facts().get(state).clone();
            if f.fluid == Fluid::None || f.fluid_level != 8 || f.fluid_falling {
                return Ok(UseResult::Pass);
            }
            let sound = match f.fluid {
                Fluid::Water => "minecraft:item.bucket.fill",
                _ => "minecraft:item.bucket.fill_lava",
            };
            let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
            // PlayerBucketFillEvent.
            let Some(filled) =
                self.with_level(|b, level, ticks| b.bucket_pickup(level, ticks, pos))
            else {
                return Ok(UseResult::Pass);
            };
            self.process_level_events()?;
            self.sounds.play(sound, Some(at(hit.pos)), 1.0, 1.0);
            self.trigger_item("minecraft:filled_bucket", &ItemStack::new(filled, 1));
            if !self.creative {
                self.consume_held();
                let filled = ItemStack {
                    item: Arc::from(filled),
                    count: 1,
                    damage: 0,
                    extra: None,
                };
                if self.held().is_none() {
                    *self.held_mut() = Some(filled);
                } else if let Some(rest) = self.give(filled)
                    && let Some(eye) = self.eye_position()
                {
                    // ItemUtils.createFilledResult: `player.drop(stack, false)`.
                    self.throw_from_eyes(rest, eye, false);
                }
            }
            return Ok(UseResult::Success { swing: true });
        }
        // BucketItem.use has its own ray, even when the crosshair hits an entity.
        if !allow_block_actions && bucket.is_none() {
            return Ok(UseResult::Pass);
        }
        let Some(hit) = self.pick() else {
            return Ok(UseResult::Pass);
        };
        let clicked = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        if let Some((content, sound, mob)) = bucket {
            if !self.may_build() {
                return Ok(UseResult::Fail);
            }
            let target = if self.res.facts().get(clicked).replaceable {
                hit.pos
            } else {
                [
                    hit.pos[0] + hit.face[0],
                    hit.pos[1] + hit.face[1],
                    hit.pos[2] + hit.face[2],
                ]
            };
            let pos = BlockPos::new(target[0], target[1], target[2]);
            // PlayerBucketEmptyEvent.
            let emptied = self
                .with_level(|b, level, ticks| b.bucket_empty(level, ticks, pos, content, sound));
            self.process_level_events()?;
            if !emptied {
                return Ok(UseResult::Fail);
            }
            if let Some(kind) = mob {
                if let Some(id) = self.spawn_mob_at(kind, (target[0], target[1], target[2]), true) {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                        crate::hc::entity_nbt::load_bucket_data(m, stack);
                    }
                    self.mob_placed(id)?;
                }
                self.post_block_game_event(
                    "minecraft:entity_place",
                    target,
                    self.sculk_actor(),
                    None,
                );
            }
            self.trigger_block("minecraft:placed_block", target, stack);
            if !self.creative {
                *self.held_mut() = Some(ItemStack::new("minecraft:bucket", 1));
            }
            return Ok(UseResult::Success { swing: true });
        }
        if self.use_block_transformer(stack, hit.pos, hit.face)? {
            return Ok(UseResult::Success { swing: true });
        }
        if item == "honeycomb" {
            let registry = self.pack.assets.registry;
            let old = registry.state(clicked).cloned();
            if let Some(waxed) = old.and_then(|old| self.res.transformers().waxed(registry, &old))
                && let Ok(state) = registry.resolve(&waxed)
            {
                // HoneycombItem.useOn: ITEM_USED_ON_BLOCK before the swap.
                self.trigger_block("minecraft:item_used_on_block", hit.pos, stack);
                self.level_set(hit.pos, state)?;
                self.level_event_at(3003, hit.pos, 0);
                self.sounds.play(
                    "minecraft:item.honeycomb.wax_on",
                    Some(at(hit.pos)),
                    1.0,
                    1.0,
                );
                // HoneycombItem.useOn's game event, and ItemStack.useOn's used statistic.
                self.post_block_game_event(
                    "minecraft:block_change",
                    hit.pos,
                    self.sculk_actor(),
                    Some(state),
                );
                self.stats.add("used", &stack.item, 1);
                if !self.creative {
                    self.consume_held();
                }
                return Ok(UseResult::Success { swing: true });
            }
        }
        if item == "flint_and_steel" || item == "fire_charge" {
            if !self.may_build() {
                return Ok(UseResult::Fail);
            }
            let name = self.res.facts().get(clicked).name.clone();
            if self.can_light(clicked, &name) {
                let registry = self.pack.assets.registry;
                let mut state = registry
                    .state(clicked)
                    .cloned()
                    .ok_or("missing lit block state")?;
                state.properties.insert("lit".into(), "true".into());
                self.level_set(hit.pos, registry.resolve(&state)?)?;
                self.sounds.play(
                    if item == "fire_charge" {
                        "minecraft:item.firecharge.use"
                    } else {
                        "minecraft:item.flintandsteel.use"
                    },
                    Some(at(hit.pos)),
                    1.0,
                    crate::hc::items::fire_starter_pitch(
                        item == "fire_charge",
                        &mut self.world.random,
                    ),
                );
                if item == "fire_charge" {
                    self.consume_held();
                } else {
                    self.damage_held(1);
                }
                return Ok(UseResult::Success { swing: true });
            }
            let target = [
                hit.pos[0] + hit.face[0],
                hit.pos[1] + hit.face[1],
                hit.pos[2] + hit.face[2],
            ];
            if self.can_place_used_fire(target) {
                if !self.try_light_portal(target)? {
                    self.place_used_fire(target)?;
                }
                let sound = crate::hc::items::fire_starter_sound(item == "fire_charge");
                let pitch = crate::hc::items::fire_starter_pitch(
                    item == "fire_charge",
                    &mut self.world.random,
                );
                self.sounds.play(sound, Some(at(target)), 1.0, pitch);
                if item == "fire_charge" {
                    self.consume_held();
                } else {
                    self.damage_held(1);
                }
                return Ok(UseResult::Success { swing: true });
            }
            return Ok(UseResult::Fail);
        }
        Ok(UseResult::Pass)
    }
    /// `ExperienceOrb.award`: orbs of the largest sizes; each draws the id
    /// `tryMergeToExisting` compares from the level random, then moves with
    /// its own random.
    fn spawn_orbs(&mut self, at: [f64; 3], mut value: u32) {
        while value > 0 {
            let v = ExperienceOrb::split(value);
            value -= v;
            let merge_id = self.world.random.next_int_bound(40).unwrap_or(0);
            self.award_orb(at, v, merge_id);
        }
    }
    /// `ExperienceOrb.tryMergeToExisting`: an orb of the same value within a
    /// 1 block box of `at`, whose id is a multiple of 40 from `merge_id`, takes
    /// the new one as one more in its count (and restarts its age); otherwise
    /// a fresh orb spawns.
    fn award_orb(&mut self, at: [f64; 3], value: u32, merge_id: i32) {
        let centre = Vec3::new(at[0], at[1], at[2]);
        let half = Vec3::new(0.5, 0.5, 0.5);
        let area = Aabb::new(centre.subtract(half), centre.add_vector(half));
        // A server's orbs are its own; a LAN or server guest only shows them.
        let target = true.then(|| {
            self.orbs
                .iter_mut()
                .find(|o| o.can_merge(merge_id as u32, value) && o.bounds().intersects(area))
        });
        if let Some(Some(orb)) = target {
            orb.count += 1;
            orb.age = 0;
        } else {
            self.add_orb(at, value);
        }
    }
    /// `new ExperienceOrb(level, pos, Vec3.ZERO, value)`: yaw and motion from
    /// the orb's own random.
    fn add_orb(&mut self, at: [f64; 3], value: u32) {
        let mut r = dispenser::entity_random();
        let _yaw = r.next_float();
        let p = Vec3::new(at[0], at[1], at[2]);
        self.next_entity += 1;
        self.orbs.push(ExperienceOrb {
            id: self.next_entity,
            position: p,
            previous: p,
            velocity: Vec3::new(
                (r.next_double() * 0.2 - 0.1) * 2.0,
                r.next_double() * 0.2 * 2.0,
                (r.next_double() * 0.2 - 0.1) * 2.0,
            ),
            value,
            age: 0,
            on_ground: false,
            count: 1,
            ticks: 0,
        });
    }
    fn can_place_used_fire(&mut self, target: [i32; 3]) -> bool {
        let pos = BlockPos::new(target[0], target[1], target[2]);
        let direction = match ((self.yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
            0 => Direction::South,
            1 => Direction::West,
            2 => Direction::North,
            _ => Direction::East,
        };
        self.with_level(|b, level, ticks| b.fire_can_be_placed_at(level, ticks, pos, direction))
    }
    fn place_used_fire(&mut self, target: [i32; 3]) -> Result<()> {
        let pos = BlockPos::new(target[0], target[1], target[2]);
        let state = self.with_level(|b, level, ticks| b.fire_state_for(level, ticks, pos));
        self.level_set(target, state)
    }
    /// DropExperienceBlock / RedStoneOreBlock / SculkBlock experience ranges.
    fn block_experience(&mut self, name: &str) -> u32 {
        let (min, max) = match name.trim_start_matches("minecraft:") {
            "coal_ore" | "deepslate_coal_ore" => (0, 2),
            "nether_gold_ore" => (0, 1),
            "lapis_ore" | "deepslate_lapis_ore" | "nether_quartz_ore" => (2, 5),
            "diamond_ore" | "deepslate_diamond_ore" | "emerald_ore" | "deepslate_emerald_ore" => {
                (3, 7)
            }
            "redstone_ore" | "deepslate_redstone_ore" => (1, 5),
            "sculk" => (1, 1),
            "sculk_catalyst" | "sculk_sensor" | "calibrated_sculk_sensor" | "sculk_shrieker" => {
                (5, 5)
            }
            // SpawnerBlock.spawnAfterBreak: 15 + nextInt(15) + nextInt(15).
            "spawner" => {
                let r = &mut self.world.random;
                let a = r.next_int_bound(15).unwrap_or(0);
                return (15 + a + r.next_int_bound(15).unwrap_or(0)) as u32;
            }
            _ => (0, 0),
        };
        if max == 0 {
            return 0;
        }
        (min + self.world.random.next_int_bound(max - min + 1).unwrap_or(0)) as u32
    }
    fn tick_experience(&mut self) {
        if self.xp_delay > 0 {
            self.xp_delay -= 1;
        }
        let spectator = self.spectator();
        let player = self
            .player
            .as_ref()
            .filter(|p| !p.health.dead_or_dying() && !spectator);
        // ExperienceOrb.followNearbyPlayer: each orb goes for the nearest
        // living player within 8 blocks, the host's own or a guest.
        let mut followed: Vec<([f64; 3], [f64; 3])> = player
            .map(|p| {
                let feet = [p.position.x, p.position.y, p.position.z];
                let eye = f64::from(p.dimensions.eye_height);
                (feet, [feet[0], feet[1] + eye / 2.0, feet[2]])
            })
            .into_iter()
            .collect();
        if let Some(host) = self.lan.as_ref() {
            followed.extend(
                host.hub
                    .guests
                    .iter()
                    .filter(|g| g.placed && g.loaded && !g.dead && g.health > 0.0)
                    .filter(|g| !g.spectator())
                    .map(|g| {
                        let p = g.position;
                        (p, [p[0], p[1] + g.eye_height() / 2.0, p[2]])
                    }),
            );
        }
        // Player.aiStep: the pickup area is the box inflated by (1, 0.5, 1).
        let bounds = player.map(|p| {
            let w = f64::from(p.dimensions.width) / 2.0;
            (
                [
                    p.position.x - w - 1.0,
                    p.position.y - 0.5,
                    p.position.z - w - 1.0,
                ],
                [
                    p.position.x + w + 1.0,
                    p.position.y + f64::from(p.dimensions.height) + 0.5,
                    p.position.z + w + 1.0,
                ],
            )
        });
        let mut orbs = std::mem::take(&mut self.orbs);
        // An orb has no random of its own here: lava draws from one made on
        // first use, apart from the level stream.
        let mut lava_random = None;
        let mut next_float = || {
            lava_random
                .get_or_insert_with(dispenser::entity_random)
                .next_float()
        };
        orbs.retain_mut(|o| {
            let distance = |feet: &[f64; 3]| -> f64 {
                (feet[0] - o.position.x).powi(2)
                    + (feet[1] - o.position.y).powi(2)
                    + (feet[2] - o.position.z).powi(2)
            };
            let target = followed
                .iter()
                .filter(|(feet, _)| distance(feet) < 64.0)
                .min_by(|a, b| distance(&a.0).total_cmp(&distance(&b.0)))
                .map(|(_, target)| *target);
            o.tick(
                &self.world,
                self.res.facts(),
                self.collision,
                target,
                &mut next_float,
            )
        });
        // ExperienceOrb.scanForMerges runs on the server only.
        if true {
            crate::hc::entities::merge_orbs(&mut orbs);
        }
        let mut gained = 0;
        let mut taken = Vec::new();
        if let Some((min, max)) = bounds {
            orbs.retain_mut(|o| {
                let touching = o.position.x + 0.25 > min[0]
                    && o.position.x - 0.25 < max[0]
                    && o.position.y + 0.5 > min[1]
                    && o.position.y < max[1]
                    && o.position.z + 0.25 > min[2]
                    && o.position.z - 0.25 < max[2];
                if touching && self.xp_delay == 0 {
                    // ExperienceOrb.playerTouch: one orb every 2 ticks; a
                    // merged orb gives its value once per count.
                    self.xp_delay = 2;
                    gained += o.value;
                    taken.push(o.clone());
                    o.count -= 1;
                    return o.count > 0;
                }
                true
            });
        }
        self.orbs = orbs;
        if gained > 0 {
            gained = self.mending(gained);
        }
        if gained > 0 {
            let at = self.eye_position();
            let pitch =
                (self.world.random.next_float() - self.world.random.next_float()) * 0.35 + 0.9;
            self.sounds
                .play("minecraft:entity.experience_orb.pickup", at, 0.1, pitch);
            self.give_orb_points(gained);
        }
    }
    /// `FurnaceResultSlot.checkTakeAchievements`: the player took from the output of the
    /// furnace at `pos`, which pops the experience of what it made at them
    /// (`awardUsedRecipesAndPopExperience`) and forgets it.
    pub(super) fn award_furnace_recipes(&mut self, pos: (i32, i32, i32), at: [f64; 3]) {
        let used = match self.world.block_entities.get_mut(&pos) {
            Some(BlockEntity::Furnace(f)) => {
                let used = f.recipe_experience(self.res.recipes());
                f.recipes_used.clear();
                used
            }
            _ => return,
        };
        for (count, experience) in used {
            let xp =
                crate::hc::containers::create_experience(count, experience, &mut self.world.random);
            self.spawn_orbs(at, xp);
        }
    }
    /// `Player.giveExperiencePoints`, and `giveExperienceLevels`' level-up
    /// sound every fifth level (at most every 100 ticks).
    pub(super) fn give_orb_points(&mut self, gained: u32) {
        let at = self.eye_position();
        // Player.giveExperienceLevels runs per level, so the first multiple of 5
        // crossed plays the sound (and starts the cooldown for the rest).
        if let Some(level) = self.experience.give(gained as i32)
            && self.ticks > self.last_level_up + 100
        {
            self.last_level_up = self.ticks;
            let volume = if level > 30 { 1.0 } else { level as f32 / 30.0 };
            self.sounds
                .play("minecraft:entity.player.levelup", at, volume * 0.75, 1.0);
        }
    }
    /// BowItem.releaseUsing: power from charge time, arrows at power * 3.
    fn release_bow(&mut self, ticks: u32) {
        let f = ticks as f32 / 20.0;
        let power = ((f * f + f * 2.0) / 3.0).min(1.0);
        if power < 0.1 {
            return;
        }
        let arrow_slot = self.projectile_slot("minecraft:bow");
        if arrow_slot.is_none() && !self.creative {
            return;
        }
        let Some(eye) = self.eye_position() else {
            return;
        };
        // BowItem.shootProjectile: shootFromRotation(power * 3, inaccuracy 1).
        let p = Vec3::new(eye[0], eye[1] - 0.1, eye[2]);
        let ammo = arrow_slot
            .and_then(|i| self.stack_in(i).cloned())
            .unwrap_or_else(|| ItemStack::new("minecraft:arrow", 1));
        let (power_bonus, infinity) = self.bow_enchantments(&ammo.item);
        let kind = crate::hc::projectiles::ProjectileKind::Arrow {
            base_damage: 2.0 + power_bonus,
            critical: power >= 1.0,
            effect: None,
        };
        let mut arrow =
            crate::hc::projectiles::Projectile::new(kind, p, Some(crate::hc::mobs::Who::Player));
        // ProjectileWeaponItem.useAmmo: creative and Infinity draw without
        // using the ammo, so the arrow is CREATIVE_ONLY; else setOwner(player)
        // lets it be picked up.
        if let Some(a) = arrow.arrow.as_mut() {
            a.set_item(&crate::hc::arrows::drawn_ammo(
                &ammo,
                self.creative || infinity,
            ));
            a.set_owner(Some(crate::hc::mobs::Who::Player));
        }
        // Flame: the arrow is spawned burning (EnchantmentHelper.onProjectileSpawned).
        if let Some(seconds) = self.bow_projectile_ignite()
            && let Some(a) = arrow.arrow.as_mut()
        {
            a.ignite_for_seconds(seconds);
        }
        let (velocity, on_ground) = self
            .player
            .as_ref()
            .map_or((Vec3::ZERO, true), |pl| (pl.velocity, pl.ground.on_ground));
        arrow.shoot_from_rotation(
            &mut self.world.random,
            self.pitch,
            self.yaw,
            0.0,
            power * 3.0,
            1.0,
            velocity,
            on_ground,
        );
        self.add_projectile(arrow);
        self.player_projectile_shot();
        let pitch = 1.0 / (self.world.random.next_float() * 0.4 + 1.2) + power * 0.5;
        self.sounds
            .play("minecraft:entity.arrow.shoot", Some(eye), 1.0, pitch);
        if !self.creative
            && !infinity
            && let Some(i) = arrow_slot
        {
            let cell = self.stack_in_mut(i);
            if let Some(s) = cell.as_mut() {
                s.count -= 1;
                if s.count == 0 {
                    *cell = None;
                }
            }
        }
        self.damage_held(1);
    }
    /// The streaming's radius and meshing options from the settings.
    fn apply_streaming_settings(&mut self) {
        self.streaming.radius = self.settings.render_distance;
        // Options.biomeBlendRadius, ambientOcclusion and cutoutLeaves:
        // LevelExtractor.allChanged when they change.
        let (s, st) = (&self.settings, &self.streaming);
        let remesh = (s.biome_blend, s.smooth_lighting, s.cutout_leaves)
            != (st.biome_blend, st.smooth_lighting, st.cutout_leaves);
        self.streaming.biome_blend = self.settings.biome_blend;
        self.streaming.smooth_lighting = self.settings.smooth_lighting;
        self.streaming.cutout_leaves = self.settings.cutout_leaves;
        if remesh {
            self.all_sections_changed();
        }
    }
    /// `LevelExtractor.allChanged`: every loaded section is rebuilt.
    fn all_sections_changed(&mut self) {
        for &(x, z) in self.world.chunks.keys() {
            for sy in 0..crate::hc::chunk::HEIGHT / 16 {
                self.world.dirty_sections.insert((x, sy, z));
            }
        }
    }
    /// Whether precipitation at a column is snow (Biome.coldEnoughToSnow:
    /// height adjusted temperature) or none (no precipitation).
    fn precipitation(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        self.res
            .biomes()
            .get(chunk.biome(x, y, z))
            .snows_at(x, y, z)
    }
    /// `Level.isRainingAt`: it rains, the sky light is 15, nothing that stops
    /// motion is above, and the biome's precipitation there is rain.
    fn raining_at(&self, p: BlockPos) -> bool {
        self.world.weather.levels(1.0).0 > 0.2
            && self.world.light(p.x, p.y, p.z) >> 4 == 15
            && self.rain_height(p.x, p.z) <= p.y
            && self.precipitation(p.x, p.y, p.z) == Some(false)
    }
    /// `getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)`: one above the
    /// highest block in `blocks_motion_in_heightmap` or holding a fluid.
    fn rain_height(&self, x: i32, z: i32) -> i32 {
        use horizon_world::{heightmap::HeightmapType, worldgen::terrain::TerrainBlockStates};
        let properties = &self.pack.assets.properties;
        self.world.motion_blocking(x, z, |s| {
            HeightmapType::MotionBlocking.is_opaque(properties.heightmap_state(s))
        })
    }
    fn tick_weather_effects(&mut self) {
        if self.flash > 0 {
            self.flash -= 1;
        }
        // The rain sounds come with the rain splashes (`tick_particles`).
    }
    /// A translated message for the player the game acts for: `overlay` is
    /// `sendOverlayMessage` (the action bar), else `sendSystemMessage` (chat).
    /// A guest gets a packet, so its own client translates it.
    fn tell_sleeper(&mut self, key: &str, fallback: &str, overlay: bool) {
        if self.lan_acting.is_some() {
            self.guest_packets
                .push(horizon_protocol::packets::play::Clientbound::SystemChat(
                    horizon_protocol::packets::play::SystemChat {
                        content: horizon_protocol::text::translatable(key, &[]),
                        overlay,
                    },
                ));
        } else if overlay {
            self.message = Some((self.translate(key, fallback), Instant::now()));
        } else {
            self.system_message(crate::hc::text::Text::translate(key, &[]));
        }
    }
    /// ServerPlayer.startSleepInBed: in reach, not obstructed, night or thunder,
    /// and (outside creative) no monsters within 8 blocks; the bed becomes the
    /// respawn point either way.
    fn try_sleep(&mut self, pos: [i32; 3]) {
        let Some((position, yaw)) = self.player.as_ref().map(|p| (p.position, p.yaw)) else {
            return;
        };
        let time = self.world.day_time.rem_euclid(24000);
        let night = (12542..23460).contains(&time) || self.world.weather.thundering;
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let props = self
            .pack
            .assets
            .registry
            .state(state)
            .map(|s| s.properties.clone())
            .unwrap_or_default();
        // The head and the foot: the block faces from the foot to the head.
        let d = facing_offset(props.get("facing").map_or("north", String::as_str));
        let (head, foot) = if props.get("part").map(String::as_str) == Some("foot") {
            ([pos[0] + d[0], pos[1], pos[2] + d[2]], pos)
        } else {
            (pos, [pos[0] - d[0], pos[1], pos[2] - d[2]])
        };
        // ServerPlayer.bedInRange / isReachableBedBlock: within 3 blocks
        // sideways and 2 vertically of the head or the foot.
        let reachable = |bed: [i32; 3]| {
            (position.x - f64::from(bed[0]) - 0.5).abs() <= 3.0
                && (position.y - f64::from(bed[1])).abs() <= 2.0
                && (position.z - f64::from(bed[2]) - 0.5).abs() <= 3.0
        };
        if !reachable(head) && !reachable(foot) {
            self.tell_sleeper(
                "block.minecraft.bed.too_far_away",
                "You may not rest now; the bed is too far away",
                true,
            );
            return;
        }
        // ServerPlayer.bedBlocked: Player.freeAt (not suffocating) for the
        // blocks above the head and the foot, as `in_wall` reads suffocating.
        let blocked = [head, foot].iter().any(|bed| {
            let f = self
                .res
                .facts()
                .get(self.world.block(bed[0], bed[1] + 1, bed[2]));
            f.full_collision && f.solid_render
        });
        if blocked {
            self.tell_sleeper(
                "block.minecraft.bed.obstructed",
                "This bed is obstructed",
                true,
            );
            return;
        }
        // PlayerBedEnterEvent: cancelled, the bed does nothing.
        // In reach (bedInRange), day or night: setRespawnPosition(
        // RespawnConfig(the head, the player's yaw), not forced). It replaces
        // an anchor or a spawnpoint; respawning checks the bed is still there
        // and finds where to stand up (findRespawnAndUseSpawnBlock).
        // Straw beds never set it (`straw_bed_rule`).
        if self.bed_sets_spawn(state) {
            let config = respawn_anchor::RespawnConfig::wrapped(self.dim, head, yaw, false);
            // ServerPlayer.setRespawnPosition: the chat message only when the
            // point moved (`isSamePosition`).
            let moved = self
                .respawn_config
                .is_none_or(|c| (c.dim, c.pos) != (config.dim, config.pos));
            self.respawn_config = Some(config);
            if moved {
                self.tell_sleeper("block.minecraft.set_spawn", "Respawn point set", false);
            }
            // The fallback becomes the world spawn: a guest's is none of its
            // own, so it follows `setworldspawn`.
            self.spawn = if self.lan_acting.is_some() {
                None
            } else {
                self.level
                    .world_spawn
                    .map(|[x, y, z]| {
                        Vec3::new(f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5)
                    })
                    .or(self.spawn)
            };
        }
        if !night {
            self.tell_sleeper(
                "block.minecraft.bed.no_sleep",
                "You can sleep only at night or during thunderstorms",
                true,
            );
            return;
        }
        // Creative players skip the monster check (`!this.isCreative()`).
        let monsters = !self.creative
            && self.mobs.iter().any(|m| {
                // `Monster.class` entities that `isPreventingPlayerRest`: a
                // zombified piglin only while it is angry.
                crate::hc::mobs::ai::is_monster(m.species())
                    && (m.species() != crate::hc::mobs::Species::ZombifiedPiglin
                        || (m.extra.anger > 0 && m.ai.target.is_some()))
                    && m.alive()
                    && (m.position.x - position.x).abs() < 8.0
                    && (m.position.y - position.y).abs() < 5.0
                    && (m.position.z - position.z).abs() < 8.0
            });
        if monsters {
            self.tell_sleeper(
                "block.minecraft.bed.not_safe",
                "You may not rest now; there are monsters nearby",
                true,
            );
            return;
        }
        // ServerPlayer.startSleepInBed: a percentage above 100 cannot skip the night.
        if self.world_data.rules.int("players_sleeping_percentage") > 100 {
            self.tell_sleeper(
                "sleep.not_possible",
                "No amount of rest can pass this night",
                true,
            );
        }
        self.stop_mining();
        self.left = false;
        self.right = false;
        self.using = None;
        self.sleeping = Some(head);
        self.sleep_timer = 0;
        self.stats.custom("sleep_in_bed", 1);
        self.trigger_player("minecraft:slept_in_bed");
        self.stats.reset("custom", "minecraft:time_since_rest");
    }
    fn wake_up(&mut self) {
        self.sleeping = None;
        self.sleep_timer = 100;
    }
    fn tick_sleep(&mut self) {
        self.sleep_timer = tick_sleep_timer(self.sleep_timer, self.sleeping.is_some());
        if let Some(head) = self.sleeping {
            if self.keys.contains(&KeyCode::ShiftLeft)
                || self
                    .player
                    .as_ref()
                    .is_none_or(|p| p.health.dead_or_dying())
            {
                self.wake_up();
                return;
            }
            if let Some(p) = self.player.as_mut() {
                p.position = Vec3::new(
                    f64::from(head[0]) + 0.5,
                    f64::from(head[1]) + 0.5625,
                    f64::from(head[2]) + 0.5,
                );
                p.velocity = Vec3::ZERO;
            }
        }
        // ServerLevel.tick: once enough of the players have slept 100 ticks,
        // skip to morning if `advance_time` is on, wake everyone, and
        // resetWeatherCycle if `advance_weather` is on and it is raining.
        if self.enough_slept() {
            if self.world_data.rules.bool("advance_time") {
                let t = self.world.day_time;
                self.world.day_time = t + 24000 - t.rem_euclid(24000);
            }
            // Level.isRaining: the rain level is above 0.2.
            if self.world_data.rules.bool("advance_weather")
                && self.world.weather.levels(1.0).0 > 0.2
            {
                self.world.weather.raining = false;
                self.world.weather.thundering = false;
                self.world.weather.rain_time = 0;
                self.world.weather.thunder_time = 0;
            }
            if self.sleeping.is_some() {
                self.wake_up();
            }
            self.wake_guests();
            // SleepStatus.removeAllSleepers: waking everyone is not announced.
            self.sleep_seen.1 = 0;
        }
    }
    /// Copy the live player and world state into the `Level` that gets saved.
    pub(super) fn sync_level(&mut self) {
        // Level.write saves the game rules (game_rules.dat) with the rest.
        self.level.game_rules = self.world_data.rules.by_name();
        self.level.selected_slot = self.selected as i32;
        let mode = self.mode_id();
        let level = &mut self.level;
        level.creative = self.creative;
        level.game_mode = mode;
        // PrimaryLevelData.setDifficulty: /difficulty and the options button change it.
        level.difficulty = crate::hc::world_options::DIFFICULTIES
            [usize::from(self.world_data.difficulty) & 3]
            .into();
        level.time = self.world.game_time;
        level.weather = self.world.weather;
        level.day_time = self.world.day_time;
        level.spawn = self.spawn.map(|s| [s.x, s.y, s.z]);
        level.respawn_elsewhere = self.respawn_config.map(|c| c.saved());
        level.rotation = [self.yaw, self.pitch];
        level.xp_level = self.experience.level;
        level.xp_progress = self.experience.progress;
        level.xp_total = self.experience.total;
        if let Some(p) = &self.player {
            level.position = Some([p.position.x, p.position.y, p.position.z]);
            level.health = p.health.health();
            level.absorption = p.health.absorption();
            let extra = level
                .player_extra
                .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
            if let horizon_world::nbt::Tag::Compound(map) = extra {
                map.insert(
                    "Fire".into(),
                    horizon_world::nbt::Tag::Short(
                        p.fire_ticks.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    ),
                );
                map.insert("Air".into(), horizon_world::nbt::Tag::Short(p.air as i16));
            }
            level.fall_distance = p.fall_distance;

            level.food = p.health.food.food;
            level.saturation = p.health.food.saturation;
            level.flying = p.flying;
        }
        level.inventory = self
            .inventory
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref()
                    .map(|s| (i as u8, s.item.to_string(), s.count, s.damage))
            })
            .collect();
        level.inventory_components = self
            .inventory
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i as u8, s.as_ref()?.extra.clone()?)))
            .collect();
        if let Some(s) = &self.offhand {
            level
                .inventory
                .push((150, s.item.to_string(), s.count, s.damage));
            if let Some(extra) = s.extra.clone() {
                level.inventory_components.insert(150, extra);
            }
        }
        // Vanilla stores armor in slots 100 (feet) to 103 (head).
        for (i, s) in self.armor.iter().enumerate() {
            if let Some(s) = s {
                level
                    .inventory
                    .push((103 - i as u8, s.item.to_string(), s.count, s.damage));
                if let Some(extra) = s.extra.clone() {
                    level.inventory_components.insert(103 - i as u8, extra);
                }
            }
        }
        // A server's main level is always the Overworld (MinecraftServer.overworld): the level
        // it happens to be in while saving is not the world's.
        // An open ender chest holds the live copy.
        if let Some(key) = self.ender_open
            && let Some(BlockEntity::Chest(items)) = self.world.block_entities.get(&key)
        {
            self.ender_items = items.clone();
        }
        level.ender_items = self.ender_items.clone();
        level.active_effects = self.potions.effects.saved();
    }
    fn save_game(&mut self) -> Result<()> {
        self.save_game_with(true)
    }
    /// The autosave doesn't `wait` for the chunks to be on disk: every
    /// generated chunk is saved, hundreds at a time, encoded and written on
    /// the region writer meanwhile.
    fn save_game_with(&mut self, wait: bool) -> Result<()> {
        self.save_level(wait)?;
        self.save_parked_levels(wait)?;
        self.save_game_metadata()
    }
    fn save_game_metadata(&mut self) -> Result<()> {
        self.save_maps();
        self.save_random_sequences();
        self.save_fight();
        self.save_wandering_trader();
        self.save_portal_arrivals();
        self.save_stats();
        self.save_advancements();
        self.save_scoreboard();
        self.save_bossbars();
        self.save_schedule();
        self.save_warden_tracker();
        self.sync_level();
        self.store_owner();
        self.level.write(&self.world_root)?;
        self.save_world_state();
        self.message = Some(("Saved the game".into(), Instant::now()));
        Ok(())
    }
    /// Save the level in place: its changed chunks, its entities and its raids.
    fn save_level(&mut self, wait: bool) -> Result<()> {
        self.save.save_forced_chunks(&self.streaming.forced)?;
        self.streaming.save(&mut self.world, wait);
        let keys: Vec<_> = self.world.modified.drain().collect();
        let chunks = self.with_entities(keys.iter().filter_map(|k| self.world.chunks.get(k)));
        self.save
            .save_later(&chunks, self.pack.assets.registry, self.res.biomes())?;
        // Prepare entities while the chunk writer runs, then use one barrier.
        self.save_entities(false)?;
        self.save_raids();
        if wait {
            self.save.flush()?;
        }
        Ok(())
    }
    /// The world's chunks as a server streams them (`ChunkMap`): the area around its players and
    /// forced chunks loads and generates, what falls out unloads with its entities and saves.
    /// Returns the section meshes the window draws and the chunks that unloaded.
    fn stream_world(
        &mut self,
    ) -> (
        Vec<((i32, i32, i32), ())>,
        Vec<std::sync::Arc<crate::hc::chunk::Chunk>>,
    ) {
        if let Some(p) = &self.player {
            self.streaming.center = (
                (p.position.x.floor() as i32) >> 4,
                (p.position.z.floor() as i32) >> 4,
            );
        }
        self.tick_pregen();
        let areas = self.streaming.extra.len();
        let waiting = std::mem::take(&mut self.spawn_wait);
        self.streaming
            .extra
            .extend(waiting.into_iter().map(|chunk| (chunk, 0)));
        let (uploads, unloaded) =
            crate::hc::spans::time("streaming", || self.streaming.update(&mut self.world));
        self.streaming.extra.truncate(areas);
        let dirty: Vec<_> = unloaded
            .iter()
            .filter(|c| self.world.modified.remove(&(c.x, c.z)))
            .cloned()
            .collect();
        let modified = crate::hc::spans::time("with_entities", || self.with_entities(&dirty));
        crate::hc::spans::time("unload_entities", || self.unload_entities(&unloaded));
        if !unloaded.is_empty() {
            let gone: crate::hc::fast_hash::FxHashSet<(i32, i32)> =
                unloaded.iter().map(|c| (c.x, c.z)).collect();
            self.world
                .block_entities
                .retain(|p, _| !gone.contains(&(p.0 >> 4, p.2 >> 4)));
            self.world
                .block_names
                .retain(|p, _| !gone.contains(&(p.0 >> 4, p.2 >> 4)));
        }
        if !modified.is_empty()
            && let Err(e) = crate::hc::spans::time("save_chunks", || {
                self.save
                    .save_later(&modified, self.pack.assets.registry, self.res.biomes())
            })
        {
            log::warn!("Horizon: failed to save unloaded chunks: {e}");
        }
        (uploads, unloaded)
    }
}

impl crate::game::Sim<'_> {}

/// The synchronous launch loads both phases; an ordinary launch returns the
/// core bundle while the rest worker keeps loading.
/// The server's resources: the prepared vanilla data, without the textures,
/// models and sounds only a window uses.
struct Loaded {
    resources: Arc<Resources>,
    pack: PackResources,
}

fn load_resources(root: &std::path::Path, settings: &Settings, started: Instant) -> Result<Loaded> {
    let threads = settings.worker_threads();
    let assets = Arc::new(
        Assets::load(root)
            .map_err(|e| format!("{e}\nPrepare resources with horizon-prepare --prepare"))?,
    );
    log::info!("load assets: {:.2}s", started.elapsed().as_secs_f32());
    let behaviour_json: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(
        std::fs::File::open(root.join("block-behaviours.json"))?,
    ))?;
    let registry = assets.registry;
    let (facts, gameplay_projection) =
        BlockFacts::load_with_projection(root, registry, &behaviour_json)?;
    let biomes = Arc::new(Biomes::load(root)?);
    let environment = Environment::load(root)?;
    let light = Arc::new(LightTables::load(root, assets.registry, &facts)?);
    let pack = PackResources::load(root, assets.clone(), light);
    crate::hc::lang::set(crate::hc::lang::Language::load(
        &[&root.to_path_buf()],
        &settings.language,
    ));
    let collision = collisions::load(root, assets.registry)?;
    let motion = MotionResources::load_with_facts(root, &assets, &facts)?;
    let resources = Arc::new(Resources {
        core: WorldCore {
            facts,
            biomes,
            environment,
            collision,
            motion,
            threads,
        },
        rest: std::sync::OnceLock::new(),
    });
    let rest = load_world_rest(
        root,
        &resources,
        &assets,
        &behaviour_json,
        &gameplay_projection,
        started,
    )?;
    resources
        .rest
        .set(rest)
        .map_err(|_| "world resources already loaded")?;
    Ok(Loaded { resources, pack })
}

fn load_world_rest(
    root: &std::path::Path,
    resources: &Resources,
    assets: &Assets,
    behaviour_json: &serde_json::Value,
    gameplay_projection: &[crate::hc::blocks::GameplayProjection],
    started: Instant,
) -> Result<WorldRest> {
    let facts = resources.facts();
    let biomes = resources.biomes();
    let threads = resources.threads();
    let registry_owner = assets;
    let mob_data = crate::hc::mobs::MobData::load(root, facts);
    crate::hc::mobs::trial_spawner::load_registry(root);

    let items = Items::load(root, assets.registry, facts)?;
    log::info!("load items: {:.2}s", started.elapsed().as_secs_f32());
    let recipes = Recipes::load(root, &items)?;
    let transformers = crate::hc::block_transform::Transformers::load(root, assets.registry)?;
    log::info!(
        "Horizon: resources loaded in {:.1}s; {} block states, {} items, {} biomes; {} worker threads",
        started.elapsed().as_secs_f32(),
        facts.states.len(),
        items.items.len(),
        biomes.list.len(),
        threads
    );
    let stations = crate::hc::stations::StationData::load(root, &items);
    let _animate =
        crate::hc::animate_tick::AnimateTables::load(root, registry_owner.registry, biomes)?;
    Ok(WorldRest {
        stations,
        items,
        recipes,
        transformers,
        providers: Providers::load(root),
        nether_environment: Environment::load_dimension(root, "the_nether", false)?,
        end_environment: Environment::load_dimension(root, "the_end", false)?,
        behaviours: Behaviours::load_with_gameplay(
            registry_owner.registry,
            root,
            behaviour_json,
            gameplay_projection,
        )?,
        mob_types: MobTypes::load(root)?
            .types
            .into_iter()
            .map(|(k, v)| (k, Arc::new(v)))
            .collect(),
        mob_data,
    })
}

/// The models a server's entities keep (frames, boats, carts, the dragon's parts).
struct GameCpuResources {
    painting_variants: paintings::PaintingVariants,
    boat_models: boats::BoatModels,
}
impl GameCpuResources {
    fn load(root: &std::path::Path) -> Result<Self> {
        let document = crate::hc::mobs::EntityModelDocument::load(root).ok();
        let _model = |key| document.as_ref().and_then(|document| document.model(key));
        Ok(Self {
            painting_variants: paintings::PaintingVariants::load(root),
            boat_models: boats::BoatModels::load(document.as_ref()),
        })
    }
}

/// `Main.main` for the server: the arguments, server.properties and the
/// EULA, the resources, then `DedicatedServer.initServer` and its loop (or a
/// component fixture a test asked for).
pub fn run_with(args: Vec<String>) -> Result<()> {
    let Some(mut options) = Options::read(args)? else {
        return Ok(());
    };
    let fixture = if let Ok(spec) = std::env::var("HORIZON_DISPENSE_FIXTURE") {
        Some(fixtures::ComponentHarness::Dispenser(spec))
    } else if let Ok(spec) = std::env::var("HORIZON_SCULK_FIXTURE") {
        Some(fixtures::ComponentHarness::Sculk(spec))
    } else {
        None
    };
    if fixture.is_none() && !dedicated::preflight(&mut options)? {
        return Ok(());
    }
    // Before any worker starts: on Linux they go behind the server thread.
    horizon_threads::put_helpers_behind();
    let started = Instant::now();
    Behaviours::set_cache_dir(std::env::current_dir()?.join(".horizon-cache"));
    let mut settings = Settings::default();
    if let Some(d) = options.distance_override {
        settings.render_distance = d;
    }
    let loaded = load_resources(&options.resources.clone(), &settings, started)?;
    let mut game = Sim::new(&loaded.resources, loaded.pack, options, settings, None)?;
    if let Some(fixture) = fixture {
        return game.run_component_harness(fixture);
    }
    if let Some(path) = game.options.control.clone() {
        let server = crate::hc::control::Server::start(&path)
            .map_err(|e| format!("control socket {}: {e}", path.display()))?;
        log::info!("Horizon: control socket {}", server.path().display());
        game.control = Some(server);
    }
    game.run_dedicated()
}

// Player.tick: cap the sleeping fade at 100, then fade out over 10 waking ticks.
fn tick_sleep_timer(timer: u32, sleeping: bool) -> u32 {
    if sleeping {
        (timer + 1).min(100)
    } else if (1..109).contains(&timer) {
        timer + 1
    } else {
        0
    }
}

fn reset_world_progress(adv: &mut advancements::AdvState, stats: &mut stats::Stats) {
    *adv = Default::default();
    *stats = Default::default();
}

impl crate::game::Sim<'_> {}

impl crate::game::Sim<'_> {}

impl<'a> Sim<'a> {
    /// The server's game: its state, before a world opens (`run_dedicated` loads it).
    pub(crate) fn new(
        res: &'a Arc<Resources>,
        pack: PackResources,
        options: Options,
        settings: Settings,
        prepared: Option<GameCpuResources>,
    ) -> Result<Self> {
        let root = options.resources.clone();
        let registry = pack.assets.registry;
        // Until a world is opened the streamer is idle.
        let mut streaming = Streaming::new(
            Arc::new(TestTerrain::new(registry, res.biomes())?),
            pack.context.clone(),
            1,
        );
        streaming.paused = true;
        let level = Level::default();
        let save = Arc::new(SaveDir::new(
            std::env::current_dir()?
                .join("saves")
                .join(".horizon-title"),
        ));
        let prepared = prepared
            .map(Ok)
            .unwrap_or_else(|| GameCpuResources::load(&root))?;
        let world = World::new(res.facts().air, 0);
        let creative = false;
        let inventory = vec![None; 36];
        let _now = Instant::now();
        // The server draws no particles: it only hears of them (and sends them on).
        let particles =
            crate::hc::particles::ParticleEngine::new(&root, &HashMap::new(), &HashSet::new());
        Ok(Sim {
            be_anims: {
                let mut anims = crate::hc::block_entity_anim::BlockEntityAnims::default();
                anims.classify(registry);
                anims
            },
            res: res.clone(),
            collision: res.collision(),
            motion: res.motion(),
            pack,
            world,
            streaming,
            options,
            player: None,
            spawn: None,
            clock: FixedStep::default(),
            previous_position: Vec3::ZERO,
            eye: 1.62,
            previous_eye: 1.62,
            yaw: level.rotation[0],
            pitch: level.rotation[1],
            inventory,
            selected: 0,
            carried: None,
            craft_grid: vec![None; 9],
            craft_size: 2,
            craft_table: None,
            inv_drag: None,
            last_inv_click: None,
            container: None,
            station: None,
            potions: Default::default(),
            scratch_slot: None,
            mining: None,
            destroying: HashMap::new(),
            destroy_delay: 0,
            use_delay: 0,
            creative,
            mode: if creative {
                crate::hc::command::args::GameMode::Creative
            } else {
                crate::hc::command::args::GameMode::Survival
            },
            keys: HashSet::new(),
            left: false,
            right: false,
            screen: Screen::Playing,
            ticks: 0,
            error: None,
            message: None,
            save,
            level,
            session_lock: None,
            sounds: crate::hc::sound_out::SoundOut::new(unique_seed()),
            hands_used: Vec::new(),
            pointer_grab: None,
            dropped: Vec::new(),
            next_entity: 0,
            particles,
            sign_edit: None,
            book: None,
            lectern_pulses: Vec::new(),
            spawn_origin: None,
            spawn_spiral: Default::default(),
            player_spawn: None,
            spawn_wait: Vec::new(),
            mobs: Vec::new(),
            creatures: Default::default(),
            sculk: Default::default(),
            raiding: Default::default(),
            attack_ticks: 100,
            attack_item: None,
            last_hurt_by_player: HashMap::new(),
            died: HashSet::new(),
            using: None,
            armor: [None, None, None, None],
            offhand: None,
            hand: Hand::Main,
            shield_cooldown: 0,
            cooldowns: Default::default(),
            command_source: None,
            autosaving: false,
            applied_modifiers: Vec::new(),
            experience: Experience::default(),
            orbs: Vec::new(),
            xp_delay: 0,
            last_level_up: 0,
            swing: Default::default(),
            swings_started: 0,
            clicks: Default::default(),
            attack_clicks: 0,
            hand_strength: Default::default(),
            settings,
            flash: 0,
            lightning_seen: [0; 2],
            sleeping: None,
            sleep_timer: 0,
            sleep_seen: (0, 0),
            dim: Dim::Overworld,
            world_root: PathBuf::new(),
            world_flat: false,
            travelling: None,
            server_trips: Vec::new(),
            level_teleports: Vec::new(),
            levels: Default::default(),
            level_pass: Default::default(),
            portals: Default::default(),
            respawn_config: None,
            respawn_travel: None,
            end_portal_travel: false,
            end: Default::default(),
            block_ticks: Ticks::new(0),
            tick_chunks: Default::default(),
            tickets: Default::default(),
            random_tick_sections: Default::default(),
            level_events: Vec::new(),
            falling: Vec::new(),
            paintings: Vec::new(),
            painting_variants: prepared.painting_variants,
            item_frames: Vec::new(),
            armor_stands: Vec::new(),
            cushions: Vec::new(),
            others: Vec::new(),
            boats: Vec::new(),
            boat_models: prepared.boat_models,
            carts: Vec::new(),
            jukebox_sounds: HashSet::new(),
            bolts: Vec::new(),
            maps: maps::Maps::default(),
            fight: dragon::Fight::default(),
            dragon: None,
            crystals: Vec::new(),
            dragon_fireballs: Vec::new(),
            breath_clouds: Vec::new(),
            knot_ids: Default::default(),
            riding: None,
            ender_items: vec![None; 27],
            ender_open: None,
            pending: Default::default(),
            spill: Default::default(),
            mount_menu: Default::default(),
            hook: None,
            remote_hooks: Vec::new(),
            tridents: Vec::new(),
            rockets: Vec::new(),
            ominous_spawners: Vec::new(),
            creaking: Default::default(),
            spawners: Default::default(),
            spawn_scratch: Default::default(),
            tnts: Vec::new(),
            hoppers: HashMap::new(),
            item_columns: None,
            agent: server_control::Agent::default(),
            control: None,
            sky_darken_memo: Default::default(),
            persist: Default::default(),
            lan: None,
            plugins: None,
            plugin_actions: Vec::new(),
            plugin_seen: Vec::new(),
            plugin_resend: Vec::new(),
            plugin_resync: Vec::new(),
            plugin_last_moves: HashMap::new(),
            previous_mode: None,
            forced_hit: None,
            guest_use: None,
            lan_events: Vec::new(),
            lan_songs: Vec::new(),
            lan_cushion_breaks: Vec::new(),
            lan_block_events: Vec::new(),
            lan_entity_events: Vec::new(),
            lan_critical_hits: Vec::new(),
            lan_kinetic_hits: Vec::new(),
            stabbed: HashMap::new(),
            known_movement: Vec3::ZERO,
            kinetic_feedback: HashMap::new(),
            lan_global_events: Vec::new(),
            lid_openers: HashMap::new(),
            lan_acting: None,
            guest_packets: Vec::new(),
            guest_cooldowns: Vec::new(),
            guest_entity_events: Vec::new(),
            lan_acting_name: None,
            lan_acting_uuid: None,
            chat_capture: None,
            admin_feedback: Vec::new(),
            console_broadcast: None,
            cmds: Default::default(),
            ui: Default::default(),
            world_data: Default::default(),
            combat: Default::default(),
            guest_combat: Default::default(),
            mount: Default::default(),
            stats: Default::default(),
            score: Default::default(),
            fns: Default::default(),
            bosses: Default::default(),
            exec: Default::default(),
            rand_sequences: Default::default(),
            brushing: Default::default(),
            border: Default::default(),
            adv: Default::default(),
            restore_player_state: true,
            fangs: Default::default(),
        })
    }
}

/// What a server's levels take from the settings a window's game reads from
/// `options.txt`: the server's view and simulation distances and its worker
/// pool (server.properties' `view-distance`, `simulation-distance`).
#[derive(Clone)]
pub struct Settings {
    pub render_distance: i32,
    pub simulation_distance: i32,
    pub biome_blend: i32,
    pub smooth_lighting: bool,
    pub cutout_leaves: bool,
    pub particles: u8,
    pub main_hand_left: bool,
    pub language: String,
    /// Worker threads; 0 picks vanilla's count (every core but one).
    pub workers: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            render_distance: 10,
            simulation_distance: 10,
            biome_blend: 2,
            smooth_lighting: true,
            cutout_leaves: true,
            particles: 0,
            main_hand_left: false,
            language: "en_us".into(),
            workers: 0,
        }
    }
}
impl Settings {
    /// `Util.maxAllowedExecutorThreads`: every core but one.
    pub fn worker_threads(&self) -> usize {
        if self.workers > 0 {
            return self.workers;
        }
        std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).max(1))
    }
}

/// `RandomSource.createNewThreadLocalInstance`'s seed (`SoundManager`'s per-sound seeds).
fn unique_seed() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static UNIQUIFIER: AtomicI64 = AtomicI64::new(8_682_522_807_148_012);
    let mut current = UNIQUIFIER.load(Ordering::Relaxed);
    let next = loop {
        let next = current.wrapping_mul(1_181_783_497_276_652_981);
        match UNIQUIFIER.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break next,
            Err(v) => current = v,
        }
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    next ^ nanos
}

impl crate::game::Sim<'_> {
    pub(crate) fn spawn_player(&mut self) -> Result<()> {
        if self.player.is_some() {
            return Ok(());
        }
        let saved = self.level.position.map(|p| Vec3::new(p[0], p[1], p[2]));
        if self.spawn.is_none() {
            self.spawn = self
                .level
                .spawn
                .map(|s| Vec3::new(s[0] + 0.5, s[1], s[2] + 0.5));
        }
        let feet = if let Some(p) = saved {
            let (cx, cz) = ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
            self.streaming.center = (cx, cz);
            if !(-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz))) {
                return Ok(());
            }
            self.level.position = None;
            p
        } else if self.spawn.is_none() || self.player_spawn.is_some() {
            // A new world: its spawn, then the first player around it.
            if self.spawn.is_none() {
                let Some([x, y, z]) = self.find_world_spawn()? else {
                    return Ok(());
                };
                self.spawn = Some(Vec3::new(
                    f64::from(x) + 0.5,
                    f64::from(y),
                    f64::from(z) + 0.5,
                ));
                self.level.spawn = Some([f64::from(x), f64::from(y), f64::from(z)]);
                // The world's spawn from the start, not only once saved: a bed set
                // before the first save must not become it.
                self.level.world_spawn.get_or_insert([x, y, z]);
                // The "Bonus Chest" create-world option, placed once with the spawn.
                if crate::hc::save::WorldGen::read(&self.world_root).is_some_and(|g| g.bonus_chest)
                {
                    self.place_bonus_chest([x, y, z])?;
                }
            }
            let s = self
                .level
                .spawn
                .unwrap_or_default()
                .map(|v| v.floor() as i32);
            let Some(feet) = self.find_player_spawn(s)? else {
                return Ok(());
            };
            feet
        } else {
            let s = self.spawn.unwrap_or_default();
            let (cx, cz) = ((s.x.floor() as i32) >> 4, (s.z.floor() as i32) >> 4);
            self.streaming.center = (cx, cz);
            if !(-1..=1).all(|dz| (-1..=1).all(|dx| self.streaming.is_lit(cx + dx, cz + dz))) {
                return Ok(());
            }
            let column = [s.x.floor() as i32, s.y.floor() as i32, s.z.floor() as i32];
            if self.level.world_spawn == Some(column) {
                // TeleportTransition.missingRespawnBlock: PlayerSpawnFinder
                // around the world spawn, not its exact column; its candidates'
                // chunks load first.
                let Some(feet) = self.find_player_spawn(column)? else {
                    return Ok(());
                };
                feet
            } else {
                // A spawnpoint: the stored column, lifted onto its current surface.
                let y = self
                    .land_column(column[0], column[2])
                    .map_or(s.y, f64::from);
                Vec3::new(s.x, y.max(s.y), s.z)
            }
        };
        let mut player = PlayerMotion::new(self.motion, feet)?;
        player.yaw = self.yaw;
        player.load_saved_state_once(&mut self.restore_player_state, &self.level)?;
        let spectator = self.mode == crate::hc::command::args::GameMode::Spectator;
        player.may_fly = self.creative || spectator;
        player.invulnerable = self.creative || spectator;
        player.spectator = spectator;
        if spectator {
            player.flying = true;
        } else if !player.may_fly {
            player.flying = false;
        }
        player.respiration = self.armor[0]
            .as_ref()
            .map_or(0, |s| s.enchantment_level("minecraft:respiration") as i32);
        player.pitch = self.pitch;
        player.glider = self.glider_worn();
        // A saved death already dropped inventory/XP and counted the death.
        if player.health.dead_or_dying() {
            self.screen = Screen::Dead;
        }
        self.previous_position = feet;
        self.player = Some(player);
        self.apply_equipment();
        self.eye = 1.62;
        self.previous_eye = 1.62;
        self.clock.reset();
        Ok(())
    }
}

impl crate::game::Sim<'_> {
    /// Load authoritative world state synchronously, without client entry or
    /// integrated-server selection. Server startup must use this entry point.
    fn load_world(&mut self, request: world_loading::WorldRequest) -> Result<()> {
        let prepared = request.prepare(&self.res, &self.pack, &self.options, &self.settings)?;
        self.finish_open_world(prepared)
    }

    fn finish_open_world(&mut self, prepared: world_loading::PreparedWorld) -> Result<()> {
        let world_loading::PreparedWorld {
            save_root,
            level,
            world,
            streaming,
            save,
            session_lock,
            rand_sequences,
            locator,
        } = prepared;
        let dim = Dim::from_id(&level.dimension);
        crate::hc::crash::set_detail("World", format!("{} ({})", level.name, save_root.display()));
        self.session_lock = Some(session_lock);
        self.end.set_locator(locator);
        self.share_spawn_conditions();
        self.score = Default::default();
        self.fns = Default::default();
        self.bosses = Default::default();
        reset_world_progress(&mut self.adv, &mut self.stats);
        self.prefetch_advancement_tree();
        self.restore_player_state = true;
        self.rand_sequences = rand_sequences;
        self.border = Default::default();
        self.set_mode_id(level.mode());
        let mut inventory = vec![None; 36];
        self.armor = [None, None, None, None];
        self.offhand = None;
        self.ender_items = if level.ender_items.len() == 27 {
            level.ender_items.clone()
        } else {
            vec![None; 27]
        };
        for (slot, id, count, damage) in &level.inventory {
            let stack = ItemStack {
                item: Arc::from(id.as_str()),
                count: *count,
                damage: *damage,
                extra: level.inventory_components.get(slot).cloned(),
            };
            if (100..104).contains(slot) {
                self.armor[usize::from(103 - slot)] = Some(stack);
            } else if *slot == 150 {
                self.offhand = Some(stack);
            } else if let Some(cell) = inventory.get_mut(usize::from(*slot)) {
                *cell = Some(stack);
            }
        }
        if self.creative && level.inventory.is_empty() && level.position.is_none() {
            for (i, name) in [
                "minecraft:grass_block",
                "minecraft:stone",
                "minecraft:oak_planks",
                "minecraft:glass",
                "minecraft:torch",
                "minecraft:oak_log",
                "minecraft:chest",
                "minecraft:furnace",
                "minecraft:diamond_pickaxe",
            ]
            .iter()
            .enumerate()
            {
                inventory[i] = Some(ItemStack {
                    item: Arc::from(*name),
                    count: 1,
                    damage: 0,
                    extra: None,
                });
            }
        }
        let (yaw, pitch) = (level.rotation[0], level.rotation[1]);
        self.yaw = yaw;
        self.pitch = pitch;
        self.selected = level.selected_slot.clamp(0, 8) as usize;
        self.streaming = streaming;
        self.world = world;
        self.inventory = inventory;
        self.save = save;
        self.level = level;
        // ServerRecipeBook.loadUntrusted: recipes the manager does not have are dropped.
        let index = &self.res.stations().recipe_index;
        let book = &mut self.level.recipe_book;
        book.recipes.retain(|r| index.contains(r));
        book.to_be_displayed.retain(|r| index.contains(r));
        self.dim = dim;
        self.world_root = save_root.clone();
        self.world_flat = crate::hc::save::WorldGen::read(&save_root).is_some_and(|w| w.flat);
        self.block_ticks = Ticks::new(self.level.seed as i32);
        self.tick_chunks.clear();
        self.clear_entities();
        self.travelling = None;
        self.player = None;
        self.sleeping = None;
        self.sleep_timer = 0;
        self.spawn = None;
        self.respawn_travel = None;
        self.respawn_config = self
            .level
            .respawn_elsewhere
            .as_ref()
            .map(respawn_anchor::RespawnConfig::from_saved);
        self.spawn_origin = None;
        self.spawn_spiral = Default::default();
        self.player_spawn = None;
        self.creatures.reset_trader();
        self.creatures.cat_tick = 0;
        self.portals = Default::default();
        self.experience = Experience {
            level: self.level.xp_level,
            progress: self.level.xp_progress,
            total: self.level.xp_total,
        };
        self.restore_effects();
        self.mining = None;
        self.container = None;
        let kinds = std::mem::take(&mut self.be_anims.kinds);
        self.be_anims = crate::hc::block_entity_anim::BlockEntityAnims {
            kinds,
            ..Default::default()
        };
        self.carried = None;
        self.using = None;
        self.screen = Screen::Playing;
        self.apply_streaming_settings();
        self.streaming.sky_light = dim != Dim::Nether;
        self.streaming.nether_shading = dim == Dim::Nether;
        self.load_world_state();
        Ok(())
    }
}
