//! Hosting the open world on the local network ("Open to LAN"): starts the
//! server threads and the play-state hub, feeds it the world's entities each
//! tick and turns what the players do into gameplay. A world that is not open
//! costs one `Option` check per tick.
use super::shield::OFFHAND_SLOT;
use crate::hc::{
    Result,
    items::ItemStack,
    mobs::{Mob, Species, Who, leash::Holder},
    server_net::{
        self, Listener, NetConfig,
        chunks::ChunkEncoder,
        data::SyncData,
        entity::{EntityState, States},
        hub::{HostEvent, Hub, HubConfig, TickCtx},
        playerdata,
    },
    text::Text,
};
use horizon_protocol::{
    entity_data::{Value, serializer},
    packets::play::{self, Clientbound as C},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// The registries and tags are the same for every world: load them once.
static SYNC: Mutex<Option<std::result::Result<Arc<SyncData>, String>>> = Mutex::new(None);

/// The registries and tags a server sends, loaded from `root` the first time.
pub(super) fn sync_data(root: &std::path::Path) -> std::result::Result<Arc<SyncData>, String> {
    let mut sync = SYNC.lock().unwrap_or_else(|e| e.into_inner());
    sync.get_or_insert_with(|| SyncData::load(root).map(Arc::new))
        .clone()
}

/// The settings of the open world (WorldOptionsScreen's multiplayer section).
#[derive(Clone, Copy, Debug)]
pub struct NetSettings {
    pub port: u16,
    pub player_commands: bool,
    pub force_game_mode: bool,
}

pub(super) struct Network {
    pub hub: Hub,
    pub settings: NetSettings,
    /// The players' models in the game's world.
    pub models: HashMap<u32, Mob>,
    pub play: HashMap<u32, super::net_actions::PlayerPlay>,
    /// Blocks broken this tick (their sound is a level event for the players).
    pub broken: Vec<[f64; 3]>,
    /// Hurt time, death and swing time (while swinging) of the mobs players see.
    mob_state: HashMap<super::Dim, HashMap<u32, (i32, bool, Option<i32>)>>,
    /// The player each mob sees as its player, when not the game (game/net_mobs.rs).
    pub mob_players: HashMap<u32, u32>,
    /// The players' locator bars, one waypoint manager per level by
    /// dimension id (game/net_waypoints.rs), and the players who are dead,
    /// whose respawn replaces their waypoint.
    pub waypoints: std::collections::HashMap<String, server_net::waypoints::Waypoints>,
    pub waypoint_dead: std::collections::HashSet<u128>,
    /// The world border as the players were last told of it (server_net/border.rs).
    pub border_sent: Option<server_net::border::Sent>,
    /// Players' commands waiting for the hub to be back in `self.network`, where
    /// a command can reach every player (game/net_actions.rs).
    pub commands: Vec<super::net_actions::QueuedCommand>,
    /// The entities the players track, described again every tick
    /// (`net_entities`) over last tick's states.
    states: server_net::entity::States,
}

impl Network {
    /// `PlayerList.remove`: `player.awardStat(Stats.LEAVE_GAME)` before the
    /// player is saved. Once per player, however often they are saved.
    pub(super) fn award_leave_game(&mut self, conn: u32) {
        if let Some(play) = self.play.get_mut(&conn)
            && !play.left_game
        {
            play.left_game = true;
            play.stats.custom("leave_game", 1);
        }
    }

    /// `PlayerList.removeAll`: every player who joined leaves with the world.
    pub(super) fn award_leave_game_to_all(&mut self) {
        let conns: Vec<u32> = self
            .hub
            .players
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            self.award_leave_game(conn);
        }
    }
}

/// A stable UUID for an entity that has none.
pub(super) fn entity_uuid(id: u32) -> u128 {
    0x4856_5a4e_0000_0000_0000_0000_0000_0000u128 | u128::from(id)
}

/// The SynchedEntityData index of the baby flag: AgeableMob.DATA_BABY_ID and
/// Zombie.DATA_BABY_ID are both the first entry after Mob's (16); Piglin's
/// comes after AbstractPiglin's zombification flag (17).
pub(super) fn baby_index(kind: &str) -> Option<u8> {
    match kind {
        "minecraft:piglin" => Some(17),
        "minecraft:cow"
        | "minecraft:mooshroom"
        | "minecraft:pig"
        | "minecraft:sheep"
        | "minecraft:chicken"
        | "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:skeleton_horse"
        | "minecraft:zombie_horse"
        | "minecraft:llama"
        | "minecraft:trader_llama"
        | "minecraft:cat"
        | "minecraft:wolf"
        | "minecraft:fox"
        | "minecraft:rabbit"
        | "minecraft:panda"
        | "minecraft:polar_bear"
        | "minecraft:bee"
        | "minecraft:frog"
        | "minecraft:sniffer"
        | "minecraft:armadillo"
        | "minecraft:villager"
        | "minecraft:zombie"
        | "minecraft:husk"
        | "minecraft:drowned"
        | "minecraft:zombie_villager"
        | "minecraft:zombified_piglin"
        | "minecraft:hoglin"
        | "minecraft:zoglin"
        | "minecraft:goat"
        | "minecraft:ocelot"
        | "minecraft:turtle"
        | "minecraft:strider"
        | "minecraft:camel"
        | "minecraft:axolotl"
        | "minecraft:dolphin"
        | "minecraft:squid"
        | "minecraft:glow_squid" => Some(16),
        _ => None,
    }
}

/// The variant and look entries of `mob_meta`'s mobs (registry holders, the
/// fish's, the bee's, the copper golem's weathering...), in a match of their
/// own: `mob_meta`'s arms hold these mobs' other entries.
fn mob_variant_meta(
    m: &Mob,
    hub: &Hub,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    let (kind, e) = (m.kind.id.as_str(), &m.extra);
    let id = |registry: &str, name: &str| hub.registry_id(registry, &format!("minecraft:{name}"));
    match kind {
        // Axolotl.DATA_VARIANT (after AgeableMob's two).
        "minecraft:axolotl" => {
            if e.variant != 0 {
                meta.push((18, serializer::INT, Value::Int(i32::from(e.variant))));
            }
        }
        // Frog.DATA_VARIANT_ID, Pig's, Cow's, Chicken's and ZombieNautilus's
        // (after the entries of their parents): registry holders.
        "minecraft:frog"
        | "minecraft:pig"
        | "minecraft:cow"
        | "minecraft:chicken"
        | "minecraft:zombie_nautilus" => {
            let (index, serializer, registry) = match kind {
                "minecraft:frog" => (18, serializer::FROG_VARIANT, "minecraft:frog_variant"),
                "minecraft:pig" => (19, serializer::PIG_VARIANT, "minecraft:pig_variant"),
                "minecraft:cow" => (18, serializer::COW_VARIANT, "minecraft:cow_variant"),
                "minecraft:chicken" => {
                    (18, serializer::CHICKEN_VARIANT, "minecraft:chicken_variant")
                }
                _ => (
                    21,
                    serializer::ZOMBIE_NAUTILUS_VARIANT,
                    "minecraft:zombie_nautilus_variant",
                ),
            };
            // Each registry's default (`define(DATA_VARIANT_ID, ... TEMPERATE)`)
            // stays out of the pairing data, as other default values do.
            let name = crate::hc::mobs::finalize::variant_name(m.species(), e.variant);
            let name = name.trim_start_matches("minecraft:");
            if let Some(v) = id(registry, name)
                && name != "temperate"
            {
                meta.push((index, serializer, Value::Id(v)));
            }
            if let Some(v) = id(registry, "temperate") {
                defaults.push((index, serializer, Value::Id(v)));
            }
        }
        // Rabbit.DATA_TYPE_ID (after AgeableMob's two) and
        // Parrot.DATA_VARIANT_ID (after TamableAnimal's two and its flags and
        // owner). The fox's type goes with its flags (#2538).
        "minecraft:rabbit" | "minecraft:parrot" => {
            let index = if kind == "minecraft:parrot" { 20 } else { 18 };
            if e.variant != 0 {
                meta.push((index, serializer::INT, Value::Int(i32::from(e.variant))));
            }
        }
        // Bee.DATA_FLAGS_ID (rolling 2, stung 4, nectar 8) and
        // DATA_ANGER_END_TIME: the texture shows the nectar and the anger.
        "minecraft:bee" => {
            let bee = crate::hc::mobs::bee::get(m);
            let flags = bee.map_or(0, |b| b.flags);
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
            let anger_end = bee.map_or(-1, |b| b.anger_end);
            if anger_end != -1 {
                meta.push((19, serializer::LONG, Value::Long(anger_end)));
            }
            defaults.push((19, serializer::LONG, Value::Long(-1)));
        }
        // AbstractFish.FROM_BUCKET takes 16: Salmon.DATA_TYPE (small 0,
        // medium 1, large 2; medium is the data's default, so always sent),
        // Pufferfish.PUFF_STATE and TropicalFish.DATA_ID_TYPE_VARIANT: the
        // pattern's packed id (size, then index << 8), base colour << 16 and
        // pattern colour << 24.
        "minecraft:salmon" => {
            meta.push((17, serializer::INT, Value::Int(i32::from(e.variant.min(2)))));
        }
        "minecraft:pufferfish" => {
            if e.size > 0 {
                meta.push((17, serializer::INT, Value::Int(e.size)));
            }
            defaults.push((17, serializer::INT, Value::Int(0)));
        }
        "minecraft:tropical_fish" => {
            let pattern = e.variant.min(11);
            let packed = i32::from(pattern >= 6)
                | i32::from(pattern % 6) << 8
                | i32::from(e.variant2 & 15) << 16
                | i32::from(e.variant3 & 15) << 24;
            if packed != 0 {
                meta.push((17, serializer::INT, Value::Int(packed)));
            }
        }
        // CopperGolem.DATA_WEATHER_STATE: unaffected 0 to oxidized 3, and it
        // goes back down when the golem is scraped.
        "minecraft:copper_golem" => {
            if e.variant != 0 {
                meta.push((
                    16,
                    serializer::WEATHERING_COPPER_STATE,
                    Value::Id(i32::from(e.variant.min(3))),
                ));
            }
            defaults.push((16, serializer::WEATHERING_COPPER_STATE, Value::Id(0)));
        }
        // Ghast.DATA_IS_CHARGING: the shooting texture.
        "minecraft:ghast" => {
            if e.flag {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
        }
        // WitherBoss.DATA_ID_INV (after the three head targets): the
        // invulnerable ticks, the blinking texture and the growing scale.
        "minecraft:wither" => {
            if e.counter > 0 {
                meta.push((19, serializer::INT, Value::Int(e.counter)));
            }
            defaults.push((19, serializer::INT, Value::Int(0)));
        }
        _ => {}
    }
}

/// A mob's synched data as ServerEntity sends it (only the values that differ
/// from the defaults the client constructs), with vanilla's entry indices:
/// Entity 0..=7, LivingEntity 8..=14, Mob 15, then the subclasses.
/// `defaults` gets the defaults of the entries that can go back to theirs.
fn mob_meta(
    m: &Mob,
    hub: &Hub,
    player_entity: Option<i32>,
    game_time: i64,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    let kind = m.kind.id.as_str();
    // Entity.DATA_SHARED_FLAGS_ID: on fire 0x01, swimming 0x10 (a drowned
    // under water, `Drowned.setSwimming`), invisible 0x20 (the effect's flag)
    // and glowing 0x40 (LivingEntity.updateGlowingStatus).
    let shared = i8::from(m.fire_ticks > 0)
        | i8::from(m.species() == Species::Drowned && m.extra.flag) << 4
        | i8::from(m.invisible || crate::hc::mobs::trader::is_invisible(m)) << 5
        | i8::from(m.is_currently_glowing()) << 6;
    if shared != 0 {
        meta.push((0, serializer::BYTE, Value::Byte(shared)));
    }
    // Entity.DATA_POSE: the pose the species keeps in its own fields (a
    // pose back at standing goes out by the entity's own default).
    let pose = crate::hc::mobs::sync::pose_id(m);
    if pose != 0 {
        meta.push((6, serializer::POSE, Value::Id(i32::from(pose))));
    }
    // LivingEntity.DATA_SLEEPING_POS_ID: the bed a sleeping villager lies in.
    if crate::hc::mobs::villager::sleeping(m)
        && let Some(bed) = m.extra.bed
    {
        meta.push((14, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(Some(bed))));
    }
    if let Some(name) = &m.custom_name {
        meta.push((
            2,
            serializer::OPTIONAL_COMPONENT,
            Value::OptText(Some(horizon_protocol::text::literal(name))),
        ));
        if m.name_visible {
            meta.push((3, serializer::BOOLEAN, Value::Bool(true)));
        }
    }
    // LivingEntity.DATA_LIVING_ENTITY_FLAGS: using an item 1, in the off
    // hand 2 (a skeleton draws its bow, a witch raises her potion).
    let using = i8::from(m.extra.using_item > 0) | i8::from(m.extra.using_off_hand) << 1;
    if using != 0 {
        meta.push((8, serializer::BYTE, Value::Byte(using)));
    }
    defaults.push((8, serializer::BYTE, Value::Byte(0)));
    // LivingEntity.DATA_HEALTH_ID: defined as 1, then set to the max health.
    let health = m.health.max(0.0);
    if health != 1.0 {
        meta.push((9, serializer::FLOAT, Value::Float(health)));
    }
    defaults.push((9, serializer::FLOAT, Value::Float(1.0)));
    // LivingEntity.DATA_EFFECT_PARTICLES and DATA_EFFECT_AMBIENCE_ID: the
    // swirls of its effects (the ones a mob has are visible, not ambient).
    meta.extend(
        hub.effect_meta(
            (m.extra.effects.iter())
                .filter(|e| !m.extra.hidden_effects.contains(&e.0))
                .map(|e| (e.0, false)),
        ),
    );
    defaults.push((10, serializer::PARTICLES, Value::Particles(Vec::new())));
    defaults.push((11, serializer::BOOLEAN, Value::Bool(false)));
    // Mob.DATA_MOB_FLAGS_ID: no AI 1, left-handed 2, aggressive 4 (what
    // draws a vindicator's axe, a skeleton's drawn bow, a piglin's raised
    // weapon).
    let flags =
        i8::from(m.no_ai) | i8::from(m.extra.left_handed) << 1 | i8::from(m.ai.aggressive) << 2;
    if flags != 0 {
        meta.push((15, serializer::BYTE, Value::Byte(flags)));
    }
    defaults.push((15, serializer::BYTE, Value::Byte(0)));
    // AgeableMob.DATA_BABY_ID (and Zombie's, Piglin's).
    if let Some(index) = baby_index(kind) {
        if m.is_baby() {
            meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
    }
    let e = &m.extra;
    let id = |registry: &str, name: &str| hub.registry_id(registry, &format!("minecraft:{name}"));
    mob_variant_meta(m, hub, meta, defaults);
    match kind {
        "minecraft:guardian" | "minecraft:elder_guardian" => {
            // Guardian.DATA_ID_MOVING and DATA_ID_ATTACK_TARGET.
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            let target = if e.flag {
                m.ai.target.and_then(|who| match who {
                    Who::Player => player_entity,
                    Who::Mob(id) => Some(id as i32),
                })
            } else {
                None
            };
            if let Some(target) = target {
                meta.push((17, serializer::INT, Value::Int(target)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((17, serializer::INT, Value::Int(0)));
        }
        // Bat.DATA_ID_FLAGS: resting in bit 0, flying by default.
        "minecraft:bat" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // SnowGolem.DATA_PUMPKIN_ID: 16 while the pumpkin is on (the default).
        "minecraft:snow_golem" => {
            if e.sheared {
                meta.push((16, serializer::BYTE, Value::Byte(0)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(16)));
        }
        // MushroomCow.DATA_TYPE: 0 red, 1 brown.
        "minecraft:mooshroom" => {
            if e.variant == 1 {
                meta.push((17, serializer::INT, Value::Int(1)));
            }
            defaults.push((17, serializer::INT, Value::Int(0)));
        }
        // AbstractPiglin.DATA_IMMUNE_TO_ZOMBIFICATION (16: with the dimension it
        // makes it shake) and Piglin.DATA_IS_DANCING (after the baby flag and
        // the crossbow charge).
        "minecraft:piglin" => {
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            if e.scratch[0] > 0 {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Sheep.DATA_WOOL_ID: the colour in the low nibble, 16 when sheared.
        "minecraft:sheep" => {
            let value = (e.color & 15) | if e.sheared { 16 } else { 0 };
            if value != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(value as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
        }
        // Fox.DATA_TYPE_ID and DATA_FLAGS_ID (after AgeableMob's two; sleeping 32, ...).
        "minecraft:fox" => {
            if e.variant != 0 {
                meta.push((18, serializer::INT, Value::Int(i32::from(e.variant))));
            }
            defaults.push((18, serializer::INT, Value::Int(0)));
            let flags = (e.ints[0] & 0xff) as u8 as i8;
            if flags != 0 {
                meta.push((19, serializer::BYTE, Value::Byte(flags)));
            }
            defaults.push((19, serializer::BYTE, Value::Byte(0)));
        }
        "minecraft:parrot" => {
            // TamableAnimal.DATA_FLAGS_ID: sitting 1, tame 4.
            let flags = i8::from(e.sitting) | if e.tame { 4 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
        }
        "minecraft:cat" | "minecraft:wolf" => {
            // TamableAnimal.DATA_FLAGS_ID: sitting 1, tame 4.
            let flags = i8::from(e.sitting) | if e.tame { 4 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
            let (index, registry, name) = if kind == "minecraft:cat" {
                let v = crate::hc::mobs::tame::CAT_VARIANTS[(e.variant as usize).min(10)];
                (20, "minecraft:cat_variant", v)
            } else {
                let v = crate::hc::mobs::tame::WOLF_VARIANTS[(e.variant as usize).min(8)].0;
                (23, "minecraft:wolf_variant", v)
            };
            let serializer = if index == 20 {
                serializer::CAT_VARIANT
            } else {
                serializer::WOLF_VARIANT
            };
            if let Some(v) = id(registry, name) {
                meta.push((index, serializer, Value::Id(v)));
            }
            // Cat.IS_LYING and RELAX_STATE_ONE.
            if kind == "minecraft:cat" {
                for (index, on) in [(21, e.flag), (22, e.relax_one)] {
                    if on {
                        meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
                    }
                    defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
                }
            }
            // Wolf.DATA_COLLAR_COLOR and Cat's, red by default.
            let collar = if kind == "minecraft:cat" { 23 } else { 21 };
            let red = i32::from(crate::hc::mobs::tame::RED);
            if i32::from(e.color & 15) != red {
                meta.push((collar, serializer::INT, Value::Int(i32::from(e.color & 15))));
            }
            defaults.push((collar, serializer::INT, Value::Int(red)));
            // Wolf.DATA_ANGER_END_TIME: the game time the anger ends, -1 when calm.
            if kind == "minecraft:wolf" {
                if e.anger > 0 {
                    let end = game_time + i64::from(e.anger);
                    meta.push((22, serializer::LONG, Value::Long(end)));
                }
                defaults.push((22, serializer::LONG, Value::Long(-1)));
            }
        }
        "minecraft:horse"
        | "minecraft:donkey"
        | "minecraft:mule"
        | "minecraft:llama"
        | "minecraft:trader_llama"
        | "minecraft:skeleton_horse"
        | "minecraft:zombie_horse"
        | "minecraft:camel" => {
            // AbstractHorse.DATA_ID_FLAGS: tame 2, eating 16 (grazing), standing
            // 32 (rearing).
            let grazes = crate::hc::mobs::equine::grazes(m.species());
            let flags = if e.tame { 2 } else { 0 }
                | if grazes && e.flag { 16 } else { 0 }
                | if grazes && e.counter > 0 { 32 } else { 0 };
            if flags != 0 {
                meta.push((18, serializer::BYTE, Value::Byte(flags)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(0)));
            if kind == "minecraft:horse" {
                let v = i32::from(e.variant) | i32::from(e.variant2) << 8;
                if v != 0 {
                    meta.push((19, serializer::INT, Value::Int(v)));
                }
            } else if kind.ends_with("llama") {
                if e.chested {
                    meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
                }
                meta.push((
                    20,
                    serializer::INT,
                    Value::Int(i32::from(e.variant2.max(1))),
                ));
                if e.variant != 0 {
                    meta.push((21, serializer::INT, Value::Int(i32::from(e.variant))));
                }
            } else if (kind == "minecraft:donkey" || kind == "minecraft:mule") && e.chested {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // HappyGhast.STAYS_STILL (after AgeableMob's two and IS_LEASH_HOLDER),
        // set while `serverStillTimeout > 0` (`syncStayStillFlag`).
        "minecraft:happy_ghast" => {
            if crate::hc::mobs::happy_ghast::on_still_timeout(m) {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Villager.DATA_VILLAGER_DATA and ZombieVillager's (after Zombie's three).
        "minecraft:villager" | "minecraft:zombie_villager" => {
            use crate::hc::mobs::render::{PROFESSIONS, VILLAGER_TYPES};
            if kind == "minecraft:villager" {
                unhappy_counter(m, meta, defaults);
            } else {
                // ZombieVillager.DATA_CONVERTING_ID: the cure's shaking.
                if crate::hc::mobs::zombie_villager::is_converting(m) {
                    meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
                }
                defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
            }
            let villager_type = VILLAGER_TYPES[e.variant as usize % VILLAGER_TYPES.len()];
            let profession = PROFESSIONS[e.variant2 as usize % PROFESSIONS.len()];
            if let (Some(t), Some(p)) = (
                id("minecraft:villager_type", villager_type),
                id("minecraft:villager_profession", profession),
            ) {
                let index = if kind == "minecraft:villager" { 19 } else { 20 };
                let level = i32::from(e.variant3.max(1));
                meta.push((
                    index,
                    serializer::VILLAGER_DATA,
                    Value::Ids(vec![t, p, level]),
                ));
            }
        }
        // AbstractCubeMob.ID_SIZE.
        "minecraft:slime" | "minecraft:magma_cube" => {
            if e.size > 1 {
                meta.push((18, serializer::INT, Value::Int(e.size)));
            }
        }
        // Creeper.DATA_SWELL_DIR, DATA_IS_POWERED and DATA_IS_IGNITED: the
        // player swells the creeper from them. The direction always goes in,
        // as only changed entries are sent and it drops back to -1.
        "minecraft:creeper" => {
            meta.push((16, serializer::INT, Value::Int(e.counter)));
            if e.charged {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // Pillager.IS_CHARGING_CROSSBOW: the charge pose and the held
        // item's loading animation.
        "minecraft:pillager" => {
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Panda: UNHAPPY_COUNTER, SNEEZE_COUNTER and EAT_COUNTER (after
        // AgeableMob's two), MAIN_GENE_ID, HIDDEN_GENE_ID and DATA_ID_FLAGS;
        // the client animates the states from them.
        "minecraft:panda" => {
            let (flags, counters) = crate::hc::mobs::panda::get(m)
                .map_or((0, [0; 3]), |s| (s.flags, [s.unhappy, s.sneeze, s.eat]));
            for (index, value) in (18..).zip(counters) {
                if value != 0 {
                    meta.push((index, serializer::INT, Value::Int(value)));
                }
                defaults.push((index, serializer::INT, Value::Int(0)));
            }
            for (index, gene) in [(21, e.variant), (22, e.variant2)] {
                if gene != 0 {
                    meta.push((index, serializer::BYTE, Value::Byte(gene as i8)));
                }
                defaults.push((index, serializer::BYTE, Value::Byte(0)));
            }
            if flags != 0 {
                meta.push((23, serializer::BYTE, Value::Byte(flags as i8)));
            }
            defaults.push((23, serializer::BYTE, Value::Byte(0)));
        }
        // PolarBear.DATA_STANDING_ID (after AgeableMob's two): the bear
        // rears up; the client animates it.
        "minecraft:polar_bear" => {
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // SpellcasterIllager.DATA_SPELL_CASTING_ID: the casting pose.
        "minecraft:evoker" | "minecraft:illusioner" => {
            if e.variant2 != 0 {
                meta.push((17, serializer::BYTE, Value::Byte(e.variant2 as i8)));
            }
            defaults.push((17, serializer::BYTE, Value::Byte(0)));
        }
        // Warden.CLIENT_ANGER_LEVEL: the heartbeat's pace.
        "minecraft:warden" => {
            let anger = e.warden.as_ref().map_or(0, |w| w.client_anger);
            if anger != 0 {
                meta.push((16, serializer::INT, Value::Int(anger)));
            }
            defaults.push((16, serializer::INT, Value::Int(0)));
        }
        // Sniffer.DATA_STATE (after AgeableMob's two).
        "minecraft:sniffer" => {
            if e.variant != 0 {
                meta.push((
                    18,
                    serializer::SNIFFER_STATE,
                    Value::Id(i32::from(e.variant)),
                ));
            }
            defaults.push((18, serializer::SNIFFER_STATE, Value::Id(0)));
        }
        // Shulker.DATA_ATTACH_FACE_ID, DATA_PEEK_ID and DATA_COLOR_ID (16
        // for none).
        "minecraft:shulker" => {
            if e.variant3 != 0 {
                meta.push((16, serializer::DIRECTION, Value::Id(i32::from(e.variant3))));
            }
            defaults.push((16, serializer::DIRECTION, Value::Id(0)));
            if e.counter != 0 {
                meta.push((17, serializer::BYTE, Value::Byte(e.counter as i8)));
            }
            defaults.push((17, serializer::BYTE, Value::Byte(0)));
            if e.color != 16 {
                meta.push((18, serializer::BYTE, Value::Byte(e.color as i8)));
            }
            defaults.push((18, serializer::BYTE, Value::Byte(16)));
        }
        // Vex.DATA_FLAGS_ID: charging in bit 0 (the arms and the glowing
        // texture).
        "minecraft:vex" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // Creaking.CAN_MOVE, IS_ACTIVE and IS_TEARING_DOWN.
        "minecraft:creaking" => {
            if e.flag2 {
                meta.push((16, serializer::BOOLEAN, Value::Bool(false)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(true)));
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
            if crate::hc::mobs::creaking::tearing_down(m) {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Allay.DATA_DANCING and DATA_CAN_DUPLICATE (the held item is its
        // main hand equipment).
        "minecraft:allay" => {
            if e.flag {
                meta.push((16, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((16, serializer::BOOLEAN, Value::Bool(false)));
            if e.egg_time > 0 {
                meta.push((17, serializer::BOOLEAN, Value::Bool(false)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(true)));
        }
        // Axolotl.DATA_PLAYING_DEAD (after DATA_VARIANT).
        "minecraft:axolotl" => {
            if crate::hc::mobs::axolotl::is_playing_dead(m) {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // CopperGolem.COPPER_GOLEM_STATE (after DATA_WEATHER_STATE).
        "minecraft:copper_golem" => {
            if e.variant3 != 0 {
                meta.push((
                    17,
                    serializer::COPPER_GOLEM_STATE,
                    Value::Id(i32::from(e.variant3)),
                ));
            }
            defaults.push((17, serializer::COPPER_GOLEM_STATE, Value::Id(0)));
        }
        // Armadillo.ARMADILLO_STATE (after AgeableMob's two).
        "minecraft:armadillo" => {
            let state = crate::hc::mobs::armadillo::state(m);
            if state != 0 {
                meta.push((18, serializer::ARMADILLO_STATE, Value::Id(i32::from(state))));
            }
            defaults.push((18, serializer::ARMADILLO_STATE, Value::Id(0)));
        }
        // Frog.DATA_TONGUE_TARGET_ID (after the variant): the entity id plus
        // one, 0 for none.
        "minecraft:frog" => {
            if let Some(target) = e.frog.tongue_target {
                let id = i32::try_from(target).map_or(0, |t| t + 1);
                meta.push((19, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(id)));
            }
            defaults.push((19, serializer::OPTIONAL_UNSIGNED_INT, Value::Id(0)));
        }
        // Goat.DATA_HAS_LEFT_HORN and DATA_HAS_RIGHT_HORN (after
        // DATA_IS_SCREAMING_GOAT): both true by default.
        "minecraft:goat" => {
            for (index, lost) in [(19, e.lost_horns[0]), (20, e.lost_horns[1])] {
                if lost {
                    meta.push((index, serializer::BOOLEAN, Value::Bool(false)));
                }
                defaults.push((index, serializer::BOOLEAN, Value::Bool(true)));
            }
        }
        // Enderman.DATA_CARRY_STATE and DATA_CREEPY (angry: the open mouth).
        "minecraft:enderman" => {
            if let Some(state) = e.carried {
                meta.push((
                    16,
                    serializer::OPTIONAL_BLOCK_STATE,
                    Value::Id(state.0 as i32),
                ));
            }
            if e.flag {
                meta.push((17, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((17, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((16, serializer::OPTIONAL_BLOCK_STATE, Value::Id(0)));
        }
        // Zombie.DATA_DROWNED_CONVERSION_ID (after DATA_BABY_ID and
        // DATA_SPECIAL_TYPE_ID): the shaking of the conversion under water.
        "minecraft:zombie" | "minecraft:husk" => {
            if e.counter2 > 0 {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
        }
        // AbstractVillager.DATA_UNHAPPY_COUNTER (after AgeableMob's two).
        "minecraft:wandering_trader" => unhappy_counter(m, meta, defaults),
        // Hoglin.DATA_IMMUNE_TO_ZOMBIFICATION (after AgeableMob's two) and
        // AbstractPiglin's, the first entry of its subclasses (the piglin's is
        // in its own arm): with the dimension it makes them shake.
        "minecraft:hoglin" | "minecraft:piglin_brute" => {
            let index = if kind == "minecraft:hoglin" { 18 } else { 16 };
            if e.flag2 {
                meta.push((index, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((index, serializer::BOOLEAN, Value::Bool(false)));
        }
        // Blaze.DATA_FLAGS_ID: charged 1, which makes it burn.
        "minecraft:blaze" => {
            if e.flag {
                meta.push((16, serializer::BYTE, Value::Byte(1)));
            }
            defaults.push((16, serializer::BYTE, Value::Byte(0)));
        }
        // Turtle.HAS_EGG and LAYING_EGG (after AgeableMob's two).
        "minecraft:turtle" => {
            if e.flag {
                meta.push((18, serializer::BOOLEAN, Value::Bool(true)));
            }
            if e.flag2 {
                meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
            }
            defaults.push((18, serializer::BOOLEAN, Value::Bool(false)));
            defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        }
        // WitherBoss.DATA_TARGET_B and DATA_TARGET_C (after DATA_TARGET_A, the
        // main target): the entities the side heads look at, by id.
        "minecraft:wither" => {
            for (k, index) in [(0, 17), (1, 18)] {
                let target = match e.ints[k] {
                    0 => None,
                    -1 => player_entity,
                    id => Some(id),
                };
                if let Some(target) = target {
                    meta.push((index, serializer::INT, Value::Int(target)));
                }
                defaults.push((index, serializer::INT, Value::Int(0)));
            }
        }
        _ => {}
    }
    // Camel.DASH and LAST_POSE_CHANGE_TICK (after AbstractHorse's flags).
    if crate::hc::mobs::camel::is_camel(m.species()) {
        if e.flag2 {
            meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
        let tick = crate::hc::mobs::camel::synced_pose_tick(m, game_time);
        if tick != 0 {
            meta.push((20, serializer::LONG, Value::Long(tick)));
        }
        defaults.push((20, serializer::LONG, Value::Long(0)));
    }
}

/// AbstractVillager.DATA_UNHAPPY_COUNTER (after AgeableMob's two): the head
/// shake of a villager saying no counts it down from 40.
fn unhappy_counter(
    m: &Mob,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    if m.extra.ints[0] > 0 {
        meta.push((18, serializer::INT, Value::Int(m.extra.ints[0])));
    }
    defaults.push((18, serializer::INT, Value::Int(0)));
}

/// What a client shakes a mob by: Entity.DATA_TICKS_FROZEN (7), whose full
/// count is `isFullyFrozen`, and Strider.DATA_SUFFOCATING (19, after
/// AgeableMob's two and DATA_BOOST_TIME).
fn mob_freeze_meta(
    m: &Mob,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
) {
    if m.ticks_frozen > 0 {
        meta.push((7, serializer::INT, Value::Int(m.ticks_frozen)));
    }
    defaults.push((7, serializer::INT, Value::Int(0)));
    if m.kind.id == "minecraft:strider" {
        if m.extra.flag {
            meta.push((19, serializer::BOOLEAN, Value::Bool(true)));
        }
        defaults.push((19, serializer::BOOLEAN, Value::Bool(false)));
    }
}

/// The equipment a mob shows (ClientboundSetEquipmentPacket slots: main hand,
/// off hand, feet, legs, chest, head, body, saddle).
fn mob_equipment(
    m: &Mob,
    hub: &mut Hub,
    out: &mut Vec<(u8, Option<horizon_protocol::item::Stack>)>,
) {
    let e = &m.extra;
    // A horse's armour, a llama's carpet, a harness: Horizon keeps the body
    // slot's item (6) in `head`.
    let body = crate::hc::mobs::drops::head_is_body(m.species());
    let slots = [
        (0u8, &e.main_hand),
        (1, &e.off_hand),
        (2, &e.feet),
        (3, &e.legs),
        (4, &e.chest),
        (if body { 6 } else { 5 }, &e.head),
    ];
    for (slot, item) in slots {
        if let Some(item) = item {
            let stack = ItemStack {
                item: item.clone(),
                count: 1,
                // A wolf's armor shows its cracks.
                damage: if slot == 6 && m.species() == crate::hc::mobs::Species::Wolf {
                    e.body_damage.max(0) as u32
                } else {
                    0
                },
                extra: if slot == 6 && m.species() == crate::hc::mobs::Species::Wolf {
                    e.body_extra.clone()
                } else {
                    None
                },
            };
            out.push((slot, hub.stack(&Some(stack))));
        }
    }
    if e.saddled {
        let stack = e.saddle_stack.clone().unwrap_or(ItemStack {
            item: Arc::from("minecraft:saddle"),
            count: 1,
            damage: 0,
            extra: None,
        });
        out.push((7, hub.stack(&Some(stack))));
    }
}

impl crate::game::Sim<'_> {
    /// `horizon-ctl network status`.
    pub(super) fn net_status(&self) -> serde_json::Value {
        use serde_json::json;
        match &self.network {
            Some(h) => json!({
                "open": true,
                "port": h.settings.port,
                // The server's own world: its finished chunks and how many it
                // has generated or loaded so far.
                "world": {
                    "chunks": self.world.chunks.len(),
                    "generated": self.streaming.generated_total,
                },
                "players": h.hub.players.iter().map(|g| json!({
                    "name": g.name,
                    "position": g.position,
                    "placed": g.placed,
                    "loaded": g.loaded,
                    "game_mode": g.game_mode,
                    // ItemCooldowns: the ticks the shield an axe disabled stays down.
                    "shield_cooldown": h.play.get(&g.conn).map_or(0, |p| p.shield_cooldown),
                    // The item the server has this player using: its slot and ticks.
                    "using": h.play.get(&g.conn).and_then(|p| p.using())
                        .map(|(slot, ticks)| json!({ "slot": slot, "ticks": ticks })),
                    "sneaking": g.sneaking,
                    // The server's Elytra glide (shared flag 7).
                    "fall_flying": g.fall_flying,
                    "sleeping": g.bed,
                    "health": g.health,
                    // The server's body: its air supply and golden hearts.
                    "air": g.motion.as_ref().map(|m| m.air),
                    "absorption": g.motion.as_ref().map(|m| m.health.absorption()),
                    // FoodData: food level, saturation and exhaustion.
                    "food": [
                        f64::from(g.food),
                        f64::from(g.saturation),
                        g.motion.as_ref().map_or(0.0, |m| f64::from(m.health.food.exhaustion)),
                    ],
                    "chunks": g.sent_chunks.len(),
                    "tracked": g.tracker.len(),
                    // Whether the game draws this player lying in a bed.
                    "model_lying": h.models.get(&g.conn).map(|m| m.extra.ints[1] != 0),
                    // The armour the game draws on this player, head to feet.
                    "model_armor": h.models.get(&g.conn).map(|m| {
                        [&m.extra.head, &m.extra.chest, &m.extra.legs, &m.extra.feet]
                            .map(|s| s.as_deref())
                    }),
                    // The game draws this player's arm swinging.
                    "model_swinging": h.models.get(&g.conn).map(|m| m.ai.swinging),
                    // For how many ticks the game draws this player using an item.
                    "model_using": h.models.get(&g.conn).map(|m| m.extra.using_item),
                })).collect::<Vec<_>>(),
            }),
            _ => json!({ "open": false }),
        }
    }
    /// Start the network side and the play-state hub.
    pub(super) fn start_network(
        &mut self,
        settings: NetSettings,
        net_config: NetConfig,
        config: HubConfig,
    ) -> Result<()> {
        let sync = sync_data(&self.options.resources)?;
        let biome_names: Vec<String> = self
            .res
            .biomes()
            .list
            .iter()
            .map(|b| b.name.clone())
            .collect();
        let registry: Vec<String> = sync
            .dynamic
            .registries
            .get("minecraft:worldgen/biome")
            .map(|v| v.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default();
        let mut encoder = ChunkEncoder::new(
            self.res
                .facts()
                .states
                .iter()
                .map(|s| (s.name.clone(), s.fluid_level > 0 || s.waterlogged)),
            &sync.builtin,
            &biome_names,
            &registry,
        );
        encoder.tables = Some(
            self.save
                .tables
                .get_or_init(|| Arc::new(crate::hc::save::SaveTables::new(&self.pack.assets)))
                .clone(),
        );
        let network = Listener::start_with(sync.clone(), net_config)?;
        let mut hub = Hub::new(network, sync, encoder, config);
        hub.flat_world = self.world_flat;
        let port = hub.network.port;
        self.world.block_log = Some(Vec::new());
        self.world.light_log = Some(Default::default());
        self.network = Some(Network {
            hub,
            settings: NetSettings { port, ..settings },
            models: HashMap::new(),
            play: HashMap::new(),
            broken: Vec::new(),
            mob_state: HashMap::new(),
            mob_players: HashMap::new(),
            waypoints: Default::default(),
            waypoint_dead: Default::default(),
            border_sent: None,
            commands: Vec::new(),
            states: Default::default(),
        });
        self.sounds.capture = Some(Vec::new());
        Ok(())
    }
    /// The server stops: the players are saved and sent away.
    pub(super) fn net_close(&mut self, announce: bool) {
        let Some(mut net) = self.network.take() else {
            return;
        };
        net.award_leave_game_to_all();
        for g in &net.hub.players {
            self.save_player(&net, g);
        }
        net.hub.disconnect_all();
        // Give the writer threads a moment to flush the disconnect packets.
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.world.block_log = None;
        self.world.light_log = None;
        self.sounds.capture = None;
        self.net_events.clear();
        self.net_songs.clear();
        self.net_critical_hits.clear();
        self.net_cushion_breaks.clear();
        self.streaming.extra.clear();
        if announce {
            self.system_message(Text::translate(
                "menu.multiplayerOptions.publish.stopped",
                &[],
            ));
        }
    }
    /// Save a player when they leave or the server stops, in their file.
    pub(super) fn save_player(
        &mut self,
        net: &Network,
        player: &crate::hc::server_net::player::ServerPlayer,
    ) {
        playerdata::save(player, &self.world_root);
        self.save_player_records(net, player);
    }
    /// The file name of a player's advancements and statistics.
    fn records_uuid(&self, _net: &Network, uuid: u128) -> Option<[i32; 4]> {
        Some(playerdata::uuid_ints(uuid))
    }
    /// `PlayerAdvancements.save` and `ServerStatsCounter.save` of a network player.
    pub(super) fn save_player_records(
        &self,
        net: &Network,
        player: &crate::hc::server_net::player::ServerPlayer,
    ) {
        if !player.placed || self.adv.tree.list.is_empty() {
            return;
        }
        let Some(uuid) = self.records_uuid(net, player.uuid) else {
            return;
        };
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let json = player
            .advancements
            .to_json(&self.adv.tree, horizon_world::DATA_VERSION);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&json) {
            let _ = std::fs::write(path, text);
        }
        if let Some(play) = net.play.get(&player.conn) {
            play.stats
                .write(&super::stats::player_json(&self.world_root, "stats", uuid));
        }
    }
    /// `PlayerAdvancements.load` and `ServerStatsCounter.parseLocal` of a
    /// network player who joined; every statistic goes out on the first
    /// request (`PlayerList.placeNewPlayer`'s `markAllDirty`).
    fn load_player_records(&mut self, net: &mut Network, conn: u32) {
        let Some(uuid) = net
            .hub
            .player(conn)
            .and_then(|g| self.records_uuid(net, g.uuid))
        else {
            return;
        };
        let tree = self.advancement_tree();
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let saved = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        if let Some(g) = net.hub.player_mut(conn)
            && let Some(json) = saved
        {
            let mut progress = crate::hc::advancements::Progress::from_json(&json);
            // PlayerAdvancements.applyFrom: advancements this server lacks are dropped.
            progress
                .criteria
                .retain(|id, _| tree.by_id.contains_key(id));
            g.advancements = progress;
            g.advancements_loaded = true;
        }
        let mut stats =
            super::stats::Stats::read(&super::stats::player_json(&self.world_root, "stats", uuid));
        stats.mark_all_dirty();
        net.play.entry(conn).or_default().stats = stats;
    }
    /// The world's default game mode (`defaultgamemode`).
    pub(super) fn set_default_game_mode(&mut self, mode: i32) {
        self.level.game_mode = mode;
        self.level.creative = mode == 1;
        if let Some(net) = self.network.as_mut() {
            net.hub.config.default_game_mode = mode;
        }
    }
    /// The entities players track: mobs, items and the rest (game/net_entities.rs).
    /// `mob_players` is the player each mob sees as its player (game/net_mobs.rs).
    fn net_entities(&self, mob_players: &HashMap<u32, u32>, hub: &mut Hub, out: &mut States) {
        out.clear();
        for m in self.mobs.iter().filter(|m| !m.removed) {
            // The lead's holder: the player the mob sees, another mob or a
            // fence knot (game/net_entities.rs).
            let player_entity = mob_players
                .get(&m.id)
                .and_then(|conn| hub.player(*conn))
                .map(|g| g.entity_id);
            let leash = m.extra.leash.and_then(|h| match h {
                Holder::Player => player_entity,
                Holder::Mob(id) => Some(id as i32),
                Holder::Knot(p) => self.knot_ids.get(&p).map(|&id| id as i32),
            });
            let e = out.add(m.id as i32, entity_uuid(m.id), m.kind.id.as_str());
            e.position = [m.position.x, m.position.y, m.position.z];
            (e.yaw, e.pitch, e.head_yaw) = (m.yaw, m.head_pitch, m.head_yaw);
            e.on_ground = m.on_ground;
            e.velocity = [m.velocity.x, m.velocity.y, m.velocity.z];
            e.needs_sync = m.needs_sync;
            mob_meta(
                m,
                hub,
                player_entity,
                self.world.game_time,
                &mut e.meta,
                &mut e.defaults,
            );
            // HappyGhast.aiStep: requiresPrecisePosition while on a still timeout.
            e.precise = m.species() == crate::hc::mobs::Species::HappyGhast
                && crate::hc::mobs::happy_ghast::on_still_timeout(m);
            mob_freeze_meta(m, &mut e.meta, &mut e.defaults);
            mob_equipment(m, hub, &mut e.equipment);
            self.mob_attributes(m, &mut e.attributes);
            e.leash = leash;
        }
        for d in &self.dropped {
            let stack = hub.stack(&Some(d.stack.clone()));
            let e = out.add(d.id as i32, entity_uuid(d.id), "minecraft:item");
            e.position = [d.position.x, d.position.y, d.position.z];
            e.on_ground = d.on_ground;
            e.velocity = [d.velocity.x, d.velocity.y, d.velocity.z];
            // Entity.DATA_SHARED_FLAGS_ID: glowing 0x40 (the Glowing tag).
            if self.entity_glowing(d.id) {
                e.meta.push((0, serializer::BYTE, Value::Byte(0x40)));
            }
            e.meta.push((8, serializer::ITEM_STACK, Value::Item(stack)));
        }
        for o in &self.orbs {
            let e = out.add(o.id as i32, entity_uuid(o.id), "minecraft:experience_orb");
            e.position = [o.position.x, o.position.y, o.position.z];
            e.on_ground = o.on_ground;
            e.velocity = [o.velocity.x, o.velocity.y, o.velocity.z];
            // ExperienceOrb.DATA_VALUE.
            e.meta
                .push((8, serializer::INT, Value::Int(o.value as i32)));
        }
        self.net_object_states(hub, out);
    }
    /// Run the hub for this tick and apply what the players did. While the
    /// single player game is `paused` only the connections run
    /// (`IntegratedServer.tickPaused`): `ServerGamePacketListenerImpl.tick`
    /// skips `tickPlayer`, so no player's air, hunger or damage moves on.
    pub(super) fn tick_net(&mut self, paused: bool) {
        let Some(mut net) = self.network.take() else {
            return;
        };
        // What the tick sends a player goes to its connection at the end.
        let holding = server_net::Holding::start();
        // The game's own commands' (and command blocks') players moving
        // to another level, and their broadcastToAdmins lines.
        if !self.level_teleports.is_empty() {
            self.run_level_teleports(&mut net);
        }
        if !self.admin_feedback.is_empty() {
            self.send_admin_feedback(&mut net, None);
        }
        // An animal whose rider left the game (or the level) stops taking
        // its input.
        for m in &mut self.mobs {
            if m.rider.is_some()
                && self.riding != Some(m.id)
                && !net.hub.players.iter().any(|g| g.vehicle == Some(m.id))
            {
                m.rider = None;
            }
        }
        if net.hub.config.dedicated {
            let has_players = !net.hub.players.is_empty();
            let mut states = std::mem::take(&mut net.states);
            if has_players {
                self.net_assign_ids();
                let players: Vec<(u32, i32)> = net
                    .hub
                    .players
                    .iter()
                    .filter_map(|g| Some((g.vehicle?, g.entity_id)))
                    .collect();
                self.net_riding(&players, &mut net.hub.riding);
                self.net_entities(&net.mob_players, &mut net.hub, &mut states);
                self.net_hook_states(&net.play, &net.hub, &mut states);
            } else {
                states.clear();
            }
            // ServerEntity.sendChanges clears needsSync once it has looked.
            for m in &mut self.mobs {
                m.needs_sync = false;
            }
            for p in &mut self.creatures.projectiles {
                p.needs_sync = false;
            }
            // `runBlockEvents`' broadcast precedes the tick's block changes
            // (`ChunkHolder.broadcastChanges` in `tick_level` below): the
            // player re-runs a piston's `triggerEvent` while its level still
            // has the blocks as they were before the move.
            for (pos, b0, b1, state) in std::mem::take(&mut self.net_block_events) {
                net.hub.block_event(pos, b0, b1, state);
            }
            let entities = states.as_slice();
            // An entity that left the level since the last tick (a snowball
            // that hit) is dropped by the tracker below: its event goes out
            // first, as `Level.broadcastEntityEvent` precedes the discard.
            let (gone, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.net_entity_events)
                .into_iter()
                .partition(|(id, ..)| !entities.iter().any(|e| e.id == *id));
            self.net_entity_events = later;
            for (id, event, at) in gone {
                net.hub.entity_event(id, event, at);
            }
            let spawn = self.spawn.map_or([0.5, 80.0, 0.5], |s| [s.x, s.y, s.z]);
            let events = {
                let clock_rate = self.network_clock_rate();
                let mut ctx = TickCtx {
                    world: &mut self.world,
                    entities,
                    next_entity: &mut self.next_entity,
                    spawn,
                    difficulty: i32::from(self.world_data.difficulty),
                    difficulty_locked: self.level.difficulty_locked,
                    clock_rate,
                    tick_rate: self.clock.rate(),
                    tick_frozen: self.clock.frozen(),
                    tick_steps: self.clock.step_state(),
                    collision: Some(self.collision),
                    border: &self.border,
                };
                net.hub.movement_checks = (
                    self.world_data.rules.bool("player_movement_check"),
                    self.world_data.rules.bool("elytra_movement_check"),
                );
                net.hub
                    .set_immediate_respawn(self.world_data.rules.bool("immediate_respawn"));
                // A server with several levels runs a pass for each
                // (`tick_server`): the network is read once, before the first.
                let pass = self.level_pass;
                let mut events = if pass.first {
                    net.hub.poll(&mut ctx)
                } else {
                    Vec::new()
                };
                events.extend(net.hub.tick_level(&mut ctx));
                if pass.last {
                    net.hub.end_tick();
                }
                events
            };
            for event in events {
                match event {
                    event @ (HostEvent::Chat { .. } | HostEvent::Warn(_) | HostEvent::Error(_)) => {
                        self.net_log_event(event);
                    }
                    HostEvent::PlayerChat { .. } => {}
                    HostEvent::ChatPending { message, .. } => {
                        // AsyncPlayerChatEvent first when a plugin listens.
                        let events = if self.plugins.is_some() {
                            self.plugin_chat(&mut net, *message)
                        } else {
                            net.hub.relay_chat(*message)
                        };
                        for e in events {
                            self.net_log_event(e);
                        }
                    }
                    HostEvent::Joined(conn) => {
                        // Before the join saves the player: whether they had played here.
                        let first_join = self.plugin_first_join(&net, conn);
                        self.net_player_joined(&mut net, conn, entities, spawn);
                        self.plugin_joined(&mut net, conn, first_join);
                        if let Some(g) = net.hub.player(conn) {
                            // PlayerList.placeNewPlayer, then the broadcast join message.
                            super::dedicated::player_joined(g.uuid, &g.name);
                            let [x, y, z] = g.position;
                            super::dedicated::log(
                                "INFO",
                                &format!(
                                    "{}[/{}] logged in with entity id {} at ({}, {}, {})",
                                    g.name,
                                    g.address,
                                    g.entity_id,
                                    super::dedicated::java_double(x),
                                    super::dedicated::java_double(y),
                                    super::dedicated::java_double(z)
                                ),
                            );
                            super::dedicated::log(
                                "INFO",
                                &format!("System chat: {} joined the game", g.name),
                            );
                        } else if self.player.is_some()
                            && let Some(g) = net.hub.player(conn)
                            && g.placed
                        {
                            // The game's chat hears of the newcomer as the players do.
                            let who = self.player_text(&g.name, g.uuid);
                            self.system_message(
                                Text::translate("multiplayer.player.joined", &[who])
                                    .with_color(crate::hc::text::YELLOW),
                            );
                        }
                    }
                    HostEvent::Left(mut player) => {
                        // The container they had open loses an opener.
                        if let Some(pos) = player.menu.as_ref().and_then(|m| m.pos)
                            && !player.spectator()
                        {
                            // In the level they were in.
                            let back = self.dim;
                            let theirs = super::Dim::from_id(&player.dimension);
                            if self.enter_level(Some(&mut net), theirs).is_ok() {
                                self.container_openers_changed(pos, false);
                            }
                            if let Err(e) = self.enter_level(Some(&mut net), back) {
                                log::error!("could not reopen {}: {e}", back.id());
                            }
                        }
                        self.net_crafting_left(&mut net, &mut player);
                        if player.placed {
                            net.award_leave_game(player.conn);
                        }
                        self.save_player(&net, &player);
                        self.plugin_left(&mut net, &player);
                        net.models.remove(&player.conn);
                        net.play.remove(&player.conn);
                        self.player_combat.remove(&player.conn);
                        if player.placed {
                            super::dedicated::log(
                                "INFO",
                                &format!("{} lost connection: {}", player.name, player.left_reason),
                            );
                            super::dedicated::log(
                                "INFO",
                                &format!("System chat: {} left the game", player.name),
                            );
                        } else if self.player.is_some() && player.placed {
                            // ServerGamePacketListenerImpl.removePlayerFromWorld.
                            let who = self.player_text(&player.name, player.uuid);
                            self.system_message(
                                Text::translate("multiplayer.player.left", &[who])
                                    .with_color(crate::hc::text::YELLOW),
                            );
                        }
                    }
                    HostEvent::Packet(conn, packet) => {
                        self.net_player_packet(&mut net, conn, *packet);
                        if !net.commands.is_empty() {
                            self.network = Some(net);
                            self.run_player_commands();
                            net = self
                                .network
                                .take()
                                .expect("the hub is back after the commands");
                        }
                    }
                }
            }
            net.states = states;
        }
        self.net_flush_effects(&mut net);
        if !paused {
            self.tick_players(&mut net);
        }
        self.net_living_flags(&mut net);
        self.tick_server_travel(&mut net);
        self.net_swings(&mut net);
        self.net_update_streaming(&net);
        self.net_update_models(&mut net);
        self.net_update_waypoints(&mut net);
        self.net_sync_scoreboard(&mut net);
        self.net_sync_border(&mut net);
        drop(holding);
        server_net::flush_all(net.hub.players.iter().map(|g| &g.sender));
        self.network = Some(net);
        // A blast from a player's attack, and a mace's wind burst.
        self.flush_net_explosions();
        let _ = self.flush_wind_bursts();
        // A sweep attack's reach to the other players from a player's attack.
        self.flush_sweeps();
        // A reeled-in hook pulling a player.
        self.flush_hook_pulls();
        // Particles a player's turn sent (sweep attack, damage indicator, a hook's splashes).
        self.flush_net_particles();
    }
    fn net_player_joined(
        &mut self,
        net: &mut Network,
        conn: u32,
        entities: &[EntityState],
        spawn: [f64; 3],
    ) {
        let default_mode = net.hub.config.default_game_mode;
        let force = net.hub.config.force_game_mode;
        let root = self.world_root.clone();
        let in_place = net.hub.config.dimension.clone();
        let Some(g) = net.hub.player_mut(conn) else {
            return;
        };
        let known = playerdata::load(g, &root);
        if !known || force {
            g.game_mode = default_mode;
        }
        if !known {
            // ServerPlayer.adjustSpawnLocation: a player new to the world
            // stands where PlayerSpawnFinder puts them, not all on one spot
            // (it reads the loaded level, which holds the spawn when it is
            // the overworld).
            let feet = if self.dim == super::Dim::Overworld {
                self.server_player_spawn(spawn)
            } else {
                spawn
            };
            g.position = feet;
            g.previous_position = feet;
            g.health = 20.0;
            g.food = 20;
            g.dimension = super::Dim::Overworld.id().to_owned();
        }
        // A server places each player in their own level
        // (`PlayerList.placeNewPlayer`); the in-frame game has one.
        let back = self.dim;
        let level = super::Dim::from_id(&g.dimension);
        let elsewhere = level != back;
        if !elsewhere {
            g.dimension = in_place;
        }
        self.load_player_records(net, conn);
        let mut theirs = States::default();
        let entities = if elsewhere {
            if let Err(e) = self.enter_level(Some(net), level) {
                log::error!("could not open {}: {e}", level.id());
                return;
            }
            self.net_entities(&net.mob_players, &mut net.hub, &mut theirs);
            self.net_hook_states(&net.play, &net.hub, &mut theirs);
            theirs.as_slice()
        } else {
            entities
        };
        let clock_rate = self.network_clock_rate();
        let mut ctx = TickCtx {
            world: &mut self.world,
            entities,
            next_entity: &mut self.next_entity,
            spawn,
            difficulty: i32::from(self.world_data.difficulty),
            difficulty_locked: self.level.difficulty_locked,
            clock_rate,
            tick_rate: self.clock.rate(),
            tick_frozen: self.clock.frozen(),
            tick_steps: self.clock.step_state(),
            collision: Some(self.collision),
            border: &self.border,
        };
        net.hub.place_new_player(conn, &mut ctx);
        if elsewhere && let Err(e) = self.enter_level(Some(net), back) {
            log::error!("could not reopen {}: {e}", back.id());
        }
    }
    /// Level events the game queued, for the players (`process_level_events`).
    pub(super) fn net_record_events(&mut self, events: &[horizon_world::level::LevelEvent]) {
        if self.sounds.capture.is_none() {
            return;
        }
        for e in events {
            match e {
                horizon_world::level::LevelEvent::LevelEvent { id, pos, data } => {
                    // The acting player does not predict the egg's teleport
                    // particles, bone meal's growth, anvil result sounds, or a
                    // finished brushing (these levelEvent calls name no
                    // excluded player), nor the digger's own crack and hit
                    // sound (2019, 2020).
                    let except = if matches!(*id, 2015 | 1505 | 1029 | 1030 | 2019 | 2020 | 3008) {
                        None
                    } else {
                        self.net_acting
                    };
                    self.net_events
                        .push((*id, [pos.x, pos.y, pos.z], *data, except));
                }
                // `ServerLevel.runBlockEvents` broadcasts a handled block
                // event (`ClientboundBlockEventPacket`): the player re-runs
                // the block's `triggerEvent` — a piston's `moveBlocks` is
                // what animates the move there. `state` is the block the
                // event named, read before it ran (the packet's `block`).
                horizon_world::level::LevelEvent::BlockEntityEvent { pos, state, a, b } => {
                    self.net_block_events
                        .push(([pos.x, pos.y, pos.z], *a, *b, state.0));
                }
                _ => {}
            }
        }
    }
    /// `Level.blockEvent` for the players (`ClientboundBlockEventPacket`).
    pub(super) fn net_block_event(&mut self, pos: [i32; 3], b0: i32, b1: i32) {
        if self.sounds.capture.is_some() {
            let state = self.world.block(pos[0], pos[1], pos[2]).0;
            self.net_block_events.push((pos, b0, b1, state));
        }
    }
    /// `Level.broadcastEntityEvent` for the players.
    pub(super) fn net_entity_event(&mut self, id: i32, event: i8, at: [f64; 3]) {
        if self.sounds.capture.is_some() {
            self.net_entity_events.push((id, event, at));
        }
    }
    /// `ServerPlayer.crit` (or `magicCrit` when `magic`): this window's
    /// emitter, and an animation for the attacker's client and observers once
    /// the hub is back in place.
    pub(super) fn critical_hit(
        &mut self,
        attacker: Option<u32>,
        id: i32,
        feet: [f64; 3],
        width: f64,
        height: f64,
        magic: bool,
    ) {
        use crate::hc::particles::{ParticleOptions, ParticleType};
        let particle = if magic {
            ParticleType::EnchantedHit
        } else {
            ParticleType::Crit
        };
        self.particles
            .tracking_emitter(ParticleOptions::simple(particle), feet, width, height);
        if self.sounds.capture.is_some() {
            self.net_critical_hits.push((attacker, id, magic));
        }
    }
    /// Send the tick's level events, sounds and mob animations.
    fn net_flush_effects(&mut self, net: &mut Network) {
        let critical_hits = std::mem::take(&mut self.net_critical_hits);
        let kinetic_hits = std::mem::take(&mut self.net_kinetic_hits);
        for (state, at) in std::mem::take(&mut self.net_cushion_breaks) {
            if let Some(id) = net
                .hub
                .registry_id("minecraft:particle_type", "minecraft:block")
            {
                net.hub.broadcast_near(
                    at,
                    32.0,
                    &C::LevelParticles(play::LevelParticles {
                        particle: horizon_protocol::entity_data::Particle {
                            id,
                            data: Some(horizon_protocol::entity_data::ParticleData::State(
                                state.0 as i32,
                            )),
                        },
                        override_limiter: false,
                        always_show: false,
                        pos: at,
                        dist: [0.25, 0.0625, 0.25],
                        max_speed: [0.05; 3],
                        count: 10,
                        randomization: 0,
                    }),
                    None,
                );
            }
        }
        for (kind, pos, data, acting) in std::mem::take(&mut self.net_events) {
            if kind == 2001 {
                net.broken
                    .push([f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2])]);
            }
            net.hub.level_event(kind, pos, data, acting);
        }
        // Level events 1010 and 1011: the players' parrots dance or stop.
        for (pos, song) in std::mem::take(&mut self.net_songs) {
            match song {
                Some(name) => {
                    let id = net.hub.registry_id("minecraft:jukebox_song", &name);
                    net.hub.level_event(1010, pos, id.unwrap_or(0), None);
                }
                None => net.hub.level_event(1011, pos, 0, None),
            }
        }
        for (pos, b0, b1, state) in std::mem::take(&mut self.net_block_events) {
            net.hub.block_event(pos, b0, b1, state);
        }
        for (id, event, at) in std::mem::take(&mut self.net_entity_events) {
            net.hub.entity_event(id, event, at);
        }
        for (kind, pos, data) in std::mem::take(&mut self.net_global_events) {
            net.hub.global_level_event(kind, pos, data);
        }
        self.net_broadcast_effects(net);
        if net.hub.players.is_empty() {
            net.mob_state.clear();
            return;
        }
        // Hurt and death animations of mobs.
        let mut seen = HashMap::new();
        for m in &self.mobs {
            let dead = !m.alive();
            let old = net
                .mob_state
                .get(&self.dim)
                .and_then(|s| s.get(&m.id))
                .copied();
            let at = [m.position.x, m.position.y, m.position.z];
            // LivingEntity.hurtServer: a hurt that restarted the hurt time.
            if m.hurt_time > old.map_or(0, |o| o.0)
                && !dead
                && let Some((source, _)) = m.last_damage
            {
                use crate::hc::mobs::{DamageKind as K, Who};
                let player = source.attacker == Some(Who::Player);
                let kind = super::creatures::damage_type(source.kind, player);
                // The player a mob sees is the player acting.
                let cause = source.attacker.and_then(|who| match who {
                    Who::Player => net
                        .mob_players
                        .get(&m.id)
                        .and_then(|conn| net.hub.player(*conn))
                        .map(|g| g.entity_id),
                    Who::Mob(id) => Some(id as i32),
                });
                // A projectile is the direct entity of its hit: not tracked here.
                let melee = matches!(
                    source.kind,
                    K::PlayerAttack
                        | K::MaceSmash
                        | K::Spear
                        | K::MobAttack
                        | K::Sting
                        | K::SonicBoom
                );
                let direct = cause.filter(|_| melee);
                net.hub.damage_event(m.id as i32, kind, cause, direct, at);
            }
            if dead && !old.is_some_and(|o| o.1) {
                net.hub.entity_event(m.id as i32, 3, at);
            }
            // LivingEntity.swing on a server (a melee hit, a thrown potion):
            // ClientboundSwingAnimationPacket for the players around. A swing
            // started since the last flush, or restarted (its time went back).
            let swing = m.ai.swinging.then_some(m.ai.swing_time);
            if let Some(time) = swing
                && !dead
                && old.and_then(|o| o.2).is_none_or(|before| time < before)
            {
                net.hub.swing(m.id as i32, 0, at, None);
            }
            seen.insert(m.id, (m.hurt_time, dead, swing));
        }
        net.mob_state.insert(self.dim, seen);
        // Level.broadcastEntityEvent(player, 2) of a spear's stab.
        for attacker in kinetic_hits {
            let hit = attacker
                .and_then(|conn| net.hub.player(conn))
                .map(|g| (g.entity_id, g.position));
            if let Some((id, at)) = hit {
                net.hub.entity_event(id, 2, at);
            }
        }
        for (attacker, target, magic) in critical_hits {
            let Some(attacker) =
                attacker.and_then(|conn| net.hub.player(conn).map(|g| g.entity_id))
            else {
                continue;
            };
            net.hub.critical_hit(attacker, target, magic);
        }
    }
    /// Keep the chunks around every player loaded.
    fn net_update_streaming(&mut self, net: &Network) {
        // DistanceManager.PlayerTicketTracker: every chunk within the server's
        // view distance (Chebyshev) of a player holds a PLAYER_LOADING ticket
        // at the entity-ticking level (31), whatever the player asked for;
        // levels rise by one a chunk, so FULL (33) reaches two chunks past
        // it. An extra area finishes its chunks out to its radius + 1. That
        // covers what the player tracks (`ChunkTrackingView`, within its view
        // distance + 1) and those chunks' lit neighbours.
        let radius = net.hub.config.view_distance + 1;
        self.streaming.extra = net
            .hub
            .players
            .iter()
            .filter(|g| net.hub.here(g))
            .map(|g| (g.chunk(), radius))
            .collect();
    }
}

impl crate::game::Sim<'_> {
    /// A hub's chat and log lines, in the server's log
    /// (MinecraftServer.logChatMessage, `[Not Secure]` for unsigned or
    /// expired messages).
    pub(super) fn net_log_event(&mut self, event: HostEvent) {
        match event {
            HostEvent::Chat { line, secure } => {
                let tag = if secure { "" } else { "[Not Secure] " };
                super::dedicated::log("INFO", &format!("{tag}{line}"));
            }
            HostEvent::Warn(line) => super::dedicated::log("WARN", &line),
            HostEvent::Error(line) => super::dedicated::log("ERROR", &line),
            _ => {}
        }
    }
}

/// `LivingEntity.DATA_LIVING_ENTITY_FLAGS` for an item in use: 1, and 2 when
/// it is the off hand's.
pub(super) fn living_flags(using: Option<(usize, u32)>) -> i8 {
    using.map_or(0, |(slot, _)| if slot == OFFHAND_SLOT { 3 } else { 1 })
}
