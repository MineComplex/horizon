//! Hosting the open world on the local network ("Open to LAN"): starts the
//! server threads and the play-state hub, feeds it the world's entities each
//! tick and turns what the guests do into gameplay. A world that is not open
//! costs one `Option` check per tick.
use super::Game;
use horizon_client::{
    Result,
    lan::{
        self,
        
        hub::Hub,
        playerdata},
    mobs::Mob,
    text::Text};
use horizon_protocol::{
    entity_data::{Value, serializer}};
use std::collections::HashMap;



/// The settings of the open world (WorldOptionsScreen's multiplayer section).
#[derive(Clone, Copy, Debug)]
pub struct LanSettings {
    pub port: u16,
    pub guest_commands: bool,
    pub force_game_mode: bool,
}

pub(super) struct LanHost {
    pub hub: Hub,
    pub settings: LanSettings,
    pub host_entity: i32,
    /// The host's swings the guests were told of (`Game::swings_started`).
    pub host_swings: u32,
    /// The host's crack the guests were told of: the block and its stage.
    pub host_crack: Option<([i32; 3], i32)>,
    pub host_uuid: u128,
    /// The host's profile name (the account's, or the offline name).
    pub host_name: String,
    /// The guests' models in the host's world.
    pub models: HashMap<u32, Mob>,
    host_mode: i32,
    pub play: HashMap<u32, super::lan_actions::GuestPlay>,
    /// Blocks broken this tick (their sound is a level event for the guests).
    pub broken: Vec<[f64; 3]>,
    /// Hurt time, death and swing time (while swinging) of the mobs guests see.
    mob_state: HashMap<super::Dim, HashMap<u32, (i32, bool, Option<i32>)>>,
    /// The guest each mob sees as its player, when not the host (game/lan_mobs.rs).
    pub mob_players: HashMap<u32, u32>,
    /// An integrated server's owner (game/integrated.rs).
    pub owner: Option<Owner>,
    /// The players' locator bars, one waypoint manager per level by
    /// dimension id (game/lan_waypoints.rs), and the guests who are dead,
    /// whose respawn replaces their waypoint.
    pub waypoints: std::collections::HashMap<String, lan::waypoints::Waypoints>,
    pub waypoint_dead: std::collections::HashSet<u128>,
    /// The world border as the guests were last told of it (lan/border.rs).
    pub border_sent: Option<lan::border::Sent>,
    /// Guests' commands waiting for the hub to be back in `self.lan`, where
    /// a command can reach every player (game/lan_actions.rs).
    pub commands: Vec<super::lan_actions::GuestCommand>,
    /// The entities the guests track, described again every tick
    /// (`lan_entities`) over last tick's states.
    states: lan::entity::States,
}

impl LanHost {
    /// `PlayerList.remove`: `player.awardStat(Stats.LEAVE_GAME)` before the
    /// player is saved. Once per guest, however often they are saved.
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
            .guests
            .iter()
            .filter(|g| g.placed)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            self.award_leave_game(conn);
        }
    }
}

/// The player who owns an integrated server's world: a guest whose state is
/// the world's own player (`players/data/<singleplayer_uuid>.dat`), as in
/// single player.
pub(super) struct Owner {
    pub uuid: u128,
    /// The world's player as loaded, until the owner joins.
    pub saved: Option<Box<horizon_client::save::Level>>,
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
            let name = horizon_client::mobs::finalize::variant_name(m.species(), e.variant);
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
            let bee = horizon_client::mobs::bee::get(m);
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





pub const HELP: &[&str] = &[
    "lan open [PORT] [commands] [force]  the test server's port; commands lets every player run commands",
    "lan status                     the test server's port and players",
];

impl Game<'_> {
    /// The guests' models for the entity pass.
    pub(super) fn guest_model_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        let Some(host) = &self.lan else {
            return;
        };
        let atlas = &self.front.gui.atlas;
        let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
        for (conn, m) in &host.models {
            let entity_start = out.len();
            let light = self.world.light(
                m.position.x.floor() as i32,
                (m.position.y + 1.62).floor() as i32,
                m.position.z.floor() as i32,
            );
            let packed = (u32::from(light >> 4) * 16) | ((u32::from(light & 15) * 16) << 8);
            horizon_client::mobs::render::render_with(
                m,
                alpha as f32,
                camera,
                &lookup,
                packed,
                self.res.mob_data(),
                &host.hub.guest(*conn).map_or_else(
                    || self.worn_armor_layers(m),
                    |g| self.stack_armor_layers(&g.armor),
                ),
                out,
            );
            self.player_elytra_geometry(
                m,
                m.extra.chest.as_deref(),
                camera,
                alpha as f32,
                packed,
                out,
            );
            self.spin_attack_geometry(m, camera, alpha as f32, packed, out);
            self.held_item_geometry(m, camera, alpha, packed, true, out);
            // LivingEntity.isCurrentlyGlowing: the guest's Glowing effect.
            let guest = host.hub.guest(*conn);
            if m.glowing || guest.is_some_and(|g| g.effects.get("glowing").is_some()) {
                let team = guest.and_then(|g| self.score.board.players_team(&g.name));
                self.mark_outline(&out[entity_start..], super::glowing::team_color(team));
            } else {
                self.cull_entity_vertices(out, entity_start);
            }
        }
    }
}
impl crate::game::Sim<'_> {
    /// `horizon-ctl lan ...`: a test's client asks its server
    /// (`--server-control`), which is always open, on its own port.
    pub(super) fn lan_control(&mut self, words: &[&str]) -> Result<Option<serde_json::Value>> {
        if words[0] != "lan" {
            return Ok(None);
        }
        match self.ask_server(&words.join(" ")) {
            Some(reply) => reply.map(Some),
            None => {
                Err("lan: this client has no server control endpoint (--server-control)".into())
            }
        }
    }
    /// IntegratedServer.unpublishServer: the guests are sent away.
    pub(super) fn lan_close(&mut self, announce: bool) {
        let Some(mut host) = self.lan.take() else {
            return;
        };
        host.award_leave_game_to_all();
        for g in &host.hub.guests {
            self.save_guest(&host, g);
        }
        host.hub.disconnect_all();
        // Give the writer threads a moment to flush the disconnect packets.
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.world.block_log = None;
        self.world.light_log = None;
        self.sounds.capture = None;
        self.lan_events.clear();
        self.lan_songs.clear();
        self.lan_critical_hits.clear();
        self.lan_cushion_breaks.clear();
        self.streaming.extra.clear();
        if announce {
            self.system_message(Text::translate(
                "menu.multiplayerOptions.publish.stopped",
                &[],
            ));
        }
    }
    /// Save a guest when they leave or the world closes: an integrated
    /// server's owner with the world (`save_game`), anyone else in their file.
    pub(super) fn save_guest(&mut self, host: &LanHost, guest: &horizon_client::lan::guest::Guest) {
        if host.owner.as_ref().is_some_and(|o| o.uuid == guest.uuid) {
            if guest.placed {
                playerdata::store(guest, &mut self.level);
            }
        } else {
            playerdata::save(guest, &self.world_root);
        }
        self.save_guest_records(host, guest);
    }
    /// The file name of a player's advancements and statistics: an
    /// integrated server's owner keeps single player's.
    fn records_uuid(&self, host: &LanHost, uuid: u128) -> Option<[i32; 4]> {
        if host.owner.as_ref().is_some_and(|o| o.uuid == uuid) {
            self.level.player_uuid
        } else {
            Some(playerdata::uuid_ints(uuid))
        }
    }
    /// `PlayerAdvancements.save` and `ServerStatsCounter.save` of a network player.
    pub(super) fn save_guest_records(
        &self,
        host: &LanHost,
        guest: &horizon_client::lan::guest::Guest,
    ) {
        if !guest.placed || self.adv.tree.list.is_empty() {
            return;
        }
        let Some(uuid) = self.records_uuid(host, guest.uuid) else {
            return;
        };
        let path = super::stats::player_json(&self.world_root, "advancements", uuid);
        let json = guest
            .advancements
            .to_json(&self.adv.tree, horizon_world::DATA_VERSION);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&json) {
            let _ = std::fs::write(path, text);
        }
        if let Some(play) = host.play.get(&guest.conn) {
            play.stats
                .write(&super::stats::player_json(&self.world_root, "stats", uuid));
        }
    }
    /// The integrated server's owner into the world's player, for `Level::write`.
    pub(super) fn store_owner(&mut self) {
        let Some(host) = &self.lan else {
            return;
        };
        let Some(uuid) = host.owner.as_ref().map(|o| o.uuid) else {
            return;
        };
        if let Some(g) = host.hub.guests.iter().find(|g| g.uuid == uuid && g.placed) {
            playerdata::store(g, &mut self.level);
            self.save_guest_records(host, g);
        }
    }
    /// The world's default game mode (`defaultgamemode`).
    pub(super) fn set_default_game_mode(&mut self, mode: i32) {
        self.level.game_mode = mode;
        self.level.creative = mode == 1;
        if let Some(host) = self.lan.as_mut() {
            host.hub.config.default_game_mode = mode;
        }
    }
    /// Level events the game queued, for the guests (`process_level_events`).
    pub(super) fn lan_record_events(&mut self, events: &[horizon_world::level::LevelEvent]) {
        if self.sounds.capture.is_none() {
            return;
        }
        for e in events {
            match e {
                horizon_world::level::LevelEvent::LevelEvent { id, pos, data } => {
                    // The acting guest does not predict the egg's teleport
                    // particles, bone meal's growth, anvil result sounds, or a
                    // finished brushing (these levelEvent calls name no
                    // excluded player), nor the digger's own crack and hit
                    // sound (2019, 2020).
                    let except = if matches!(*id, 2015 | 1505 | 1029 | 1030 | 2019 | 2020 | 3008) {
                        None
                    } else {
                        self.lan_acting
                    };
                    self.lan_events
                        .push((*id, [pos.x, pos.y, pos.z], *data, except));
                }
                // `ServerLevel.runBlockEvents` broadcasts a handled block
                // event (`ClientboundBlockEventPacket`): the guest re-runs
                // the block's `triggerEvent` — a piston's `moveBlocks` is
                // what animates the move there. `state` is the block the
                // event named, read before it ran (the packet's `block`).
                horizon_world::level::LevelEvent::BlockEntityEvent { pos, state, a, b } => {
                    self.lan_block_events
                        .push(([pos.x, pos.y, pos.z], *a, *b, state.0));
                }
                _ => {}
            }
        }
    }
    /// `Level.blockEvent` for the guests (`ClientboundBlockEventPacket`).
    pub(super) fn lan_block_event(&mut self, pos: [i32; 3], b0: i32, b1: i32) {
        if self.sounds.capture.is_some() {
            let state = self.world.block(pos[0], pos[1], pos[2]).0;
            self.lan_block_events.push((pos, b0, b1, state));
        }
    }
    /// `Level.broadcastEntityEvent` for the guests.
    pub(super) fn lan_entity_event(&mut self, id: i32, event: i8, at: [f64; 3]) {
        if self.sounds.capture.is_some() {
            self.lan_entity_events.push((id, event, at));
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
        use horizon_client::particles::{ParticleOptions, ParticleType};
        let particle = if magic {
            ParticleType::EnchantedHit
        } else {
            ParticleType::Crit
        };
        self.particles
            .tracking_emitter(ParticleOptions::simple(particle), feet, width, height);
        if self.sounds.capture.is_some() {
            self.lan_critical_hits.push((attacker, id, magic));
        }
    }
    /// The bed at `head` as a sleeping model lies in it (`ints[1]`,
    /// `villager::sleep_pose`): its `facing`.
    pub(super) fn bed_facing(&self, head: [i32; 3]) -> i32 {
        let state = self.world.block(head[0], head[1], head[2]);
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing"))
            .map(String::as_str);
        horizon_client::mobs::villager::bed_facing_code(facing)
    }
    /// A chat line the host typed, for the guests.
    pub(super) fn lan_host_chat(&mut self, message: &str) {
        if let Some(host) = &self.lan {
            host.hub.chat(&host.host_name, message);
        }
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
}

