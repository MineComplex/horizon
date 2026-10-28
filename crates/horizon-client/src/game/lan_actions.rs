//! What guests do in a hosted world. The host's gameplay code is written for
//! the one local player, so a guest's action runs through it with the guest's
//! state swapped in for the duration of the call (position, inventory, game
//! mode, experience, mining and item use): digging, placing, using items and
//! blocks, attacking and the commands all behave exactly as they do for the
//! host, and every world change they cause reaches all players through the
//! block log, the level events and the entity tracker.
use super::{ Game, Mining, Screen, lan::LanHost, respawn_anchor::RespawnConfig};
use horizon_client::{
    containers::BlockEntity,
    entities::Experience,
    
    items::ItemStack,
    lan::{
        guest::SPECTATOR,
        menu::Slots},
    motion::PlayerMotion};
use horizon_core::geometry::Vec3;
use horizon_protocol::{
    packets::play::{self, Clientbound as C},
    text::literal};
use winit::keyboard::KeyCode;

/// Per-guest state of the gameplay code that lives outside `Guest`.
#[derive(Default)]
pub(super) struct GuestPlay {
    /// The sign this guest may write on (`SignBlockEntity.playerWhoMayEdit`).
    pub(super) sign_editing: Option<[i32; 3]>,
    mining: Option<Mining>,
    destroy_delay: u32,
    using: Option<(usize, u32)>,
    use_delay: u32,
    /// The shield's ItemCooldowns entry: an axe disabled it for this many ticks.
    pub(super) shield_cooldown: u32,
    /// `ServerGamePacketListenerImpl.dropSpamThrottler` (`TickThrottler(20, 1480)`): 20 for each
    /// item a creative player drops from the creative screen, one less each tick.
    creative_drops: u32,
    /// The guest's `Player.getCooldowns()` (`Game::cooldowns` for its turn).
    pub(super) cooldowns: horizon_client::cooldowns::Cooldowns,
    attack_ticks: u32,
    /// The main hand's item at the guest's last tick (`Game::attack_item`).
    attack_item: Option<std::sync::Arc<str>>,
    swing: horizon_client::first_person::SwingState,
    /// The block being dug and the face the client aimed at.
    digging: Option<([i32; 3], [i32; 3])>,
    /// The server tick the digging started (ServerPlayerGameMode.destroyProgressStart).
    dig_start: u64,
    /// A block the client stopped digging before the server's time was up: it breaks once
    /// its progress since `dig_start` is complete (ServerPlayerGameMode.hasDelayedDestroy).
    delayed: Option<([i32; 3], u64)>,
    last_progress: i32,
    /// Swings the guest's actions started this tick, for the players around
    /// (`LivingEntity.swing` sends ClientboundSwingAnimationPacket).
    swings: u32,
    /// The guest's riding jump charge (`LocalPlayer.jumpRidingTicks`).
    mount: super::mounts::MountState,
    /// The trading screen the guest has open.
    pub(super) merchant: Option<super::lan_trading::GuestMerchant>,
    /// The inventory menu and the crafting table the guest crafts in.
    pub(super) crafting: super::lan_crafting::GuestCrafting,
    /// The workstation menu the guest has open (game/lan_stations.rs).
    pub(super) station: Option<super::lan_stations::GuestStation>,
    /// The mount inventory the guest has open (game/lan_mount_menu.rs).
    pub(super) mount_window: Option<super::lan_mount_menu::GuestMount>,
    /// The lectern the guest reads (game/lan_stations.rs).
    pub(super) lectern: Option<super::lan_stations::GuestLectern>,
    /// What the guest's body was given for its effects and enchantments.
    potions: super::potions::BodyPotions,
    /// `ServerPlayer.getStats` and the inventory `inventory_changed` last saw.
    pub(super) stats: super::stats::Stats,
    /// `Stats.LEAVE_GAME` is counted: a guest is saved more than once on the
    /// way out, the statistic is awarded once (`LanHost::award_leave_game`).
    pub(super) left_game: bool,
    last_inventory: u64,
    /// `ServerPlayer.levitationStartPos` and `levitationStartTime`.
    levitation: Option<([f64; 3], u64)>,
    /// `Entity.portalProcess` and `portalCooldown` (game/server_travel.rs).
    pub(super) portal: horizon_client::nether_portal::PortalState,
    /// `ServerPlayer.wonGame`: the End poem is showing, the player waits for
    /// its client's PERFORM_RESPAWN (`respawn_requested`).
    pub(super) won_game: bool,
    pub(super) respawn_requested: bool,
    /// `Player.takeXpDelay`: ticks until the next orb can be taken.
    xp_delay: u32,
    /// What last killed the guest outright (`/kill`): the death message's key.
    death_cause: Option<String>,
    /// The equipment modifiers on the guest's attributes (`apply_equipment`)
    /// and the armour and main hand they were put on for: the hub's player
    /// is a `ServerPlayer` too, whose worn armour counts
    /// (`LivingEntity.collectEquipmentChanges`).
    modifiers: Vec<(String, String)>,
    equipped: Option<Vec<Option<ItemStack>>>,
    /// The guest's cast fishing hook (`Player.fishing`).
    pub(super) hook: Option<super::fishing::Hook>,
    sculk: super::sculk::PlayerEvents,
}

/// A guest's command waiting for the hub (`LanHost::commands`).
pub(super) struct GuestCommand {
    pub(super) conn: u32,
    pub(super) command: String,
    pub(super) permission: u8,
    /// An integrated server's: no level teleports (yet).
    pub(super) integrated: bool,
    /// A mod's quiet command (game/lan_quiet.rs): its output suppressed, as
    /// `CommandSourceStack.withSuppressedOutput` does.
    pub(super) quiet: bool,
}

/// The game's own state while a guest's is swapped in (`enter_guest`), and
/// the guest's records that live outside `Guest`.
pub(super) struct GuestSwap {
    conn: u32,
    i: usize,
    play: GuestPlay,
    player: Option<PlayerMotion>,
    /// The guest's eye height and the game's.
    eye: (f32, f32),
    yaw: f32,
    pitch: f32,
    selected: usize,
    mode: horizon_client::command::args::GameMode,
    creative: bool,
    xp: Experience,
    host_keys: std::collections::HashSet<KeyCode>,
    screen: Screen,
    host_sign: Option<super::sign_edit::SignEditor>,
    host_book: Option<super::books::BookScreen>,
    host_command_block: Option<super::command_blocks::CommandBlockScreen>,
    host_mount_menu: super::mount_menu::MountMenu,
    craft_size: usize,
    craft_table: Option<[i32; 3]>,
    craft_grid: Vec<Option<ItemStack>>,
    container: Option<(i32, i32, i32)>,
    mining: Option<Mining>,
    destroy_delay: u32,
    using: Option<(usize, u32)>,
    use_delay: u32,
    pub(super) shield_cooldown: u32,
    /// The guest's item cooldowns (`Game::cooldowns` for its turn).
    cooldowns: horizon_client::cooldowns::Cooldowns,
    attack_ticks: u32,
    attack_item: Option<std::sync::Arc<str>>,
    swing: horizon_client::first_person::SwingState,
    left: bool,
    right: bool,
    riding: Option<u32>,
    host_station: Option<super::stations::Station>,
    sleeping: Option<[i32; 3]>,
    sleep_timer: u32,
    spawn: Option<Vec3>,
    respawn: (
        Option<super::respawn_anchor::RespawnConfig>,
        Option<[f64; 3]>,
    ),
    message: Option<(String, std::time::Instant)>,
    host_ender: Option<(i32, i32, i32)>,
    adv_loaded: bool,
    last_inventory: u64,
    levitation: Option<([f64; 3], u64)>,
    death_cause: Option<String>,
    /// The guest's own damage record, in place while they act.
    combat: super::combat::CombatTracker,
    /// The guest whose turn this one nests in, if any.
    acting: (Option<u32>, Option<String>, Option<u128>),
    /// The packets queued for that guest.
    packets: Vec<play::Clientbound>,
    /// The entity events queued for that guest.
    entity_events: Vec<i8>,
    /// The host's count of swings started, kept while the guest acts.
    swings: u32,
    /// How many projectiles there were: those added since are the guest's.
    projectiles: usize,
    /// The host's equipment modifiers.
    modifiers: Vec<(String, String)>,
}

impl GuestPlay {
}



impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Run `f` as the guest `conn`: the host's player state is swapped for theirs.
    pub(super) fn as_guest<R>(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let swap = self.enter_guest(host, conn)?;
        let result = f(self);
        self.leave_guest(host, swap);
        Some(result)
    }
    /// The first half of `as_guest`: the guest's state in place of the
    /// game's, which the returned swap keeps until `leave_guest`. Between the
    /// two the host may go back into `self.lan`, so that what the guest does
    /// can reach the other players (their commands, `for_player`), and a
    /// guest's turn can nest in another's: the acting guest and the packets
    /// queued for them are kept and restored too.
    pub(super) fn enter_guest(&mut self, host: &mut LanHost, conn: u32) -> Option<GuestSwap> {
        // The guest whose turn it is is in place already (their record holds
        // the game's state until their turn ends).
        if self.lan_acting == Some(conn) {
            return None;
        }
        let i = host.hub.guests.iter().position(|g| g.conn == conn)?;
        let fresh = host.hub.guests[i].motion.is_none();
        if fresh {
            let p = Vec3::new(
                host.hub.guests[i].position[0],
                host.hub.guests[i].position[1],
                host.hub.guests[i].position[2],
            );
            host.hub.guests[i].motion = PlayerMotion::new(self.motion, p).ok().map(Box::new);
            // The first body after joining burns, breathes and falls on as saved.
            if let Some(saved) = host.hub.guests[i].loaded_body.take()
                && let Some(m) = host.hub.guests[i].motion.as_mut()
            {
                saved.apply(m);
            }
            // The saved golden hearts go on the first body of a player who
            // has been here before.
            let saved = host.hub.guests[i].saved_absorption.take();
            if let (Some(m), Some(amount)) = (host.hub.guests[i].motion.as_mut(), saved) {
                let _ = m.internal_set_absorption(amount);
            }
            let kept = host.hub.guests[i].logs_kept.take();
            if let (Some(m), Some((hurts, attacks))) = (host.hub.guests[i].motion.as_mut(), kept) {
                m.hurt_log = hurts;
                m.attack_log = attacks;
            }
        }
        let mut play = host.play.remove(&conn).unwrap_or_default();
        if fresh {
            // A new body (joined, respawned, changed level): nothing on it yet,
            // its effects' modifiers and armor protection included.
            play.equipped = None;
            play.potions = Default::default();
            play.sculk = Default::default();
        }
        let g = &mut host.hub.guests[i];
        // The guest's body.
        let Some(mut motion) = g.motion.take() else {
            host.play.insert(conn, play);
            return None;
        };
        self.sculk.swap_player(&mut play.sculk);
        // The guest's own CombatTracker: damage to them is not the host's.
        // A new body starts with no record of the last one.
        let guest_combat = if fresh {
            Default::default()
        } else {
            self.guest_combat.remove(&conn).unwrap_or_default()
        };
        let host_combat = std::mem::replace(&mut self.combat, guest_combat);
        self.potions.swap_player(&mut g.effects, &mut play.potions);
        motion.position = Vec3::new(g.position[0], g.position[1], g.position[2]);
        motion.ground.on_ground = g.on_ground;
        motion.yaw = g.yaw;
        motion.pitch = g.pitch;
        motion.sprinting = g.sprinting;
        motion.may_fly = g.creative() || g.game_mode == SPECTATOR;
        motion.flying = g.flying;
        // FireworkRocketItem.use boosts only a gliding player.
        motion.fall_flying = g.fall_flying;
        motion.invulnerable = motion.may_fly;
        let _ = motion.set_health(g.health, false);
        motion.health.food.food = g.food;
        motion.health.food.saturation = g.saturation;
        let mut player = Some(*motion);
        std::mem::swap(&mut self.player, &mut player);
        let eye = (g.eye_height() as f32, self.eye);
        self.eye = eye.0;
        let (yaw, pitch) = (self.yaw, self.pitch);
        self.yaw = g.yaw;
        self.pitch = g.pitch;
        std::mem::swap(&mut self.inventory, &mut g.inventory);
        std::mem::swap(&mut self.armor, &mut g.armor);
        std::mem::swap(&mut self.offhand, &mut g.offhand);
        std::mem::swap(&mut self.carried, &mut g.carried);
        let selected = std::mem::replace(&mut self.selected, g.selected);
        let (mode, creative) = (self.mode, self.creative);
        self.set_mode_raw(g.game_mode);
        let xp = std::mem::replace(
            &mut self.experience,
            Experience {
                level: g.xp_level,
                progress: g.xp_progress,
                total: g.xp_total,
            },
        );
        // The host's held keys stay theirs: the guest holds only sneak, and a
        // screen the guest opens (`keys.clear()`) leaves the host walking.
        let host_keys = std::mem::take(&mut self.keys);
        if g.sneaking {
            self.keys.insert(KeyCode::ShiftLeft);
        }
        // The window's screens are the host's: a sign, book, command block or
        // mount menu the guest's action opens is not drawn over the host's
        // world (#687). The guest's client opens its own.
        let screen = std::mem::replace(&mut self.screen, Screen::Playing);
        let host_sign = self.sign_edit.take();
        let host_book = self.book.take();
        let host_command_block = self.command_block_screen.take();
        let host_mount_menu = std::mem::take(&mut self.mount_menu);
        // The host's crafting grid stays theirs; a crafting table the guest
        // opens shows as the 3x3 size.
        let craft_size = std::mem::replace(&mut self.craft_size, 2);
        let craft_table = self.craft_table.take();
        let craft_grid = std::mem::replace(&mut self.craft_grid, vec![None; 9]);
        let container = self.container.take();
        let mining = std::mem::replace(&mut self.mining, play.mining.take());
        let destroy_delay = std::mem::replace(&mut self.destroy_delay, play.destroy_delay);
        let using = std::mem::replace(&mut self.using, play.using.take());
        let use_delay = std::mem::replace(&mut self.use_delay, play.use_delay);
        let shield_cooldown = std::mem::replace(&mut self.shield_cooldown, play.shield_cooldown);
        let cooldowns = std::mem::replace(&mut self.cooldowns, std::mem::take(&mut play.cooldowns));
        let attack_ticks = std::mem::replace(&mut self.attack_ticks, play.attack_ticks);
        let attack_item = std::mem::replace(&mut self.attack_item, play.attack_item.take());
        let swing = std::mem::replace(&mut self.swing, play.swing);
        std::mem::swap(&mut self.mount, &mut play.mount);
        let (left, right) = (self.left, self.right);
        self.left = play.digging.is_some();
        self.right = play.using.is_some() || self.using.is_some();
        // The guest's vehicle (Entity.vehicle) for its turn.
        let riding = std::mem::replace(&mut self.riding, g.vehicle);
        let host_station = self.station.take();
        // The guest's bed, sleep counter and respawn point; the host's
        // respawn point, anchor and action bar stay theirs.
        let sleeping = std::mem::replace(&mut self.sleeping, g.bed);
        let sleep_timer = std::mem::replace(&mut self.sleep_timer, g.sleep_timer);
        let spawn = std::mem::replace(
            &mut self.spawn,
            g.spawn.map(|s| Vec3::new(s[0], s[1], s[2])),
        );
        let respawn = (self.respawn_config.take(), self.level.spawn);
        self.respawn_config = g.respawn_elsewhere.as_ref().map(RespawnConfig::from_saved);
        let message = self.message.take();
        // The guest's own ender inventory; one the host has open goes back
        // into the host's while the guest acts.
        let host_ender = self.ender_open;
        self.close_ender_chest();
        std::mem::swap(&mut self.ender_items, &mut g.ender_items);
        // The guest's advancements, recipe book and statistics.
        self.advancement_tree();
        std::mem::swap(&mut self.adv.progress, &mut g.advancements);
        std::mem::swap(&mut self.adv.changed, &mut g.advancements_changed);
        std::mem::swap(&mut self.adv.unlocked, &mut g.recipes_unlocked);
        std::mem::swap(&mut self.level.recipe_book, &mut g.recipe_book);
        std::mem::swap(&mut self.level.player_extra, &mut g.saved);
        std::mem::swap(&mut self.stats, &mut play.stats);
        std::mem::swap(&mut self.hook, &mut play.hook);
        let adv_loaded = std::mem::replace(&mut self.adv.loaded, true);
        let last_inventory = std::mem::replace(&mut self.adv.last_inventory, play.last_inventory);
        let levitation = std::mem::replace(&mut self.adv.levitation, play.levitation);
        let death_cause = std::mem::replace(&mut self.ui.death_cause, play.death_cause.take());
        let acting = (
            self.lan_acting.replace(conn),
            self.lan_acting_name.replace(g.name.clone()),
            self.lan_acting_uuid.replace(g.uuid),
        );
        let packets = std::mem::take(&mut self.guest_packets);
        let entity_events = std::mem::take(&mut self.guest_entity_events);
        let swings = self.swings_started;
        let projectiles = self.creatures.projectiles.len();
        let modifiers = std::mem::replace(
            &mut self.applied_modifiers,
            std::mem::take(&mut play.modifiers),
        );
        // Its worn armour on its attributes, when it changed.
        let mut equipped: Vec<Option<ItemStack>> = self.armor.to_vec();
        equipped.push(self.inventory.get(self.selected).cloned().flatten());
        if play.equipped.as_ref() != Some(&equipped) {
            self.apply_equipment();
            play.equipped = Some(equipped);
        }
        // The player's effects are on the new body before anything it does
        // (the movement that brought it, a landing): vanilla keeps the same
        // entity through a level change, with its attributes and effects.
        if fresh {
            self.apply_effect_status();
        }
        self.update_totem_ready();
        Some(GuestSwap {
            conn,
            i,
            play,
            player,
            eye,
            yaw,
            pitch,
            selected,
            mode,
            creative,
            xp,
            host_keys,
            screen,
            host_sign,
            host_book,
            host_command_block,
            host_mount_menu,
            craft_size,
            craft_table,
            craft_grid,
            container,
            mining,
            destroy_delay,
            using,
            use_delay,
            shield_cooldown,
            cooldowns,
            attack_ticks,
            attack_item,
            swing,
            left,
            right,
            riding,
            host_station,
            sleeping,
            sleep_timer,
            spawn,
            respawn,
            message,
            host_ender,
            adv_loaded,
            last_inventory,
            levitation,
            death_cause,
            acting,
            packets,
            entity_events,
            swings,
            projectiles,
            modifiers,
            combat: host_combat,
        })
    }
    /// The second half of `as_guest`: the guest's state back into their
    /// record, the game's back in place, and what the turn changed sent.
    pub(super) fn leave_guest(&mut self, host: &mut LanHost, swap: GuestSwap) {
        // A totem that saved the guest this turn (its hurt set it off).
        let totem = self.apply_totem();
        let GuestSwap {
            conn,
            i,
            mut play,
            mut player,
            eye,
            yaw,
            pitch,
            selected,
            mode,
            creative,
            xp,
            host_keys,
            screen,
            host_sign,
            host_book,
            host_command_block,
            host_mount_menu,
            craft_size,
            craft_table,
            craft_grid,
            container,
            mining,
            destroy_delay,
            using,
            use_delay,
            shield_cooldown,
            cooldowns,
            attack_ticks,
            attack_item,
            swing,
            left,
            right,
            riding,
            host_station,
            sleeping,
            sleep_timer,
            spawn,
            respawn,
            message,
            host_ender,
            adv_loaded,
            last_inventory,
            levitation,
            death_cause,
            combat,
            acting,
            packets,
            entity_events,
            swings,
            projectiles,
            modifiers,
        } = swap;
        self.sculk.swap_player(&mut play.sculk);
        // The guest's damage record back into their slot, the host's in place.
        let guest_combat = std::mem::replace(&mut self.combat, combat);
        self.guest_combat.insert(conn, guest_combat);
        play.modifiers = std::mem::replace(&mut self.applied_modifiers, modifiers);
        // What the guest shot is theirs (Projectile.setOwner).
        for p in self.creatures.projectiles.iter_mut().skip(projectiles) {
            if p.owner == Some(horizon_client::mobs::Who::Player) && p.guest.is_none() {
                p.guest = Some(conn);
            }
        }
        // The guest's swings are theirs: the host's own count stays the host's.
        play.swings += self.swings_started.wrapping_sub(swings);
        self.swings_started = swings;
        for packet in std::mem::replace(&mut self.guest_packets, packets) {
            host.hub.send_to(conn, &packet);
        }
        // ServerPlayer.connection.send: the guest's own entity events.
        let entity_id = host.hub.guests[i].entity_id;
        for event in std::mem::replace(&mut self.guest_entity_events, entity_events) {
            host.hub.send_to(
                conn,
                &C::EntityEvent(play::EntityEvent {
                    id: entity_id,
                    event,
                }),
            );
        }
        // ServerItemCooldowns.onCooldownStarted: the guest's own cooldown.
        for (group, duration) in std::mem::take(&mut self.guest_cooldowns) {
            host.hub.send_to(
                conn,
                &C::Cooldown(play::Cooldown {
                    group: group.to_string(),
                    duration,
                }),
            );
        }
        (self.lan_acting, self.lan_acting_name, self.lan_acting_uuid) = acting;
        play.death_cause = std::mem::replace(&mut self.ui.death_cause, death_cause);
        // An ender chest the guest opened: the menu shows their inventory
        // itself (lan_open_container), not a stand-in block entity.
        self.close_ender_chest();

        // What the action opened: handed to the guest instead of the host.
        let opened = self.container.take();
        let guest_table = std::mem::replace(&mut self.craft_table, craft_table);
        let opened_table = (std::mem::replace(&mut self.craft_size, craft_size) == 3)
            .then_some(guest_table)
            .flatten();
        self.craft_grid = craft_grid;
        let opened_station = std::mem::replace(&mut self.station, host_station);
        let bed = std::mem::replace(&mut self.sleeping, sleeping);
        let guest_sleep_timer = std::mem::replace(&mut self.sleep_timer, sleep_timer);
        let guest_spawn = std::mem::replace(&mut self.spawn, spawn).map(|s| [s.x, s.y, s.z]);
        let guest_respawn = std::mem::replace(&mut self.respawn_config, respawn.0);
        self.level.spawn = respawn.1;
        // ServerPlayer.displayClientMessage(_, true): the guest's action bar.
        let guest_message = std::mem::replace(&mut self.message, message);
        let guest_vehicle = std::mem::replace(&mut self.riding, riding);
        self.left = left;
        self.right = right;
        play.mining = std::mem::replace(&mut self.mining, mining);
        play.destroy_delay = std::mem::replace(&mut self.destroy_delay, destroy_delay);
        play.using = std::mem::replace(&mut self.using, using);
        play.use_delay = std::mem::replace(&mut self.use_delay, use_delay);
        play.shield_cooldown = std::mem::replace(&mut self.shield_cooldown, shield_cooldown);
        play.cooldowns = std::mem::replace(&mut self.cooldowns, cooldowns);
        play.attack_ticks = std::mem::replace(&mut self.attack_ticks, attack_ticks);
        play.attack_item = std::mem::replace(&mut self.attack_item, attack_item);
        play.swing = std::mem::replace(&mut self.swing, swing);
        std::mem::swap(&mut self.mount, &mut play.mount);
        self.container = container;
        self.screen = screen;
        // A sign the guest's placement opened (SignBlock.openTextEdit).
        let guest_sign = std::mem::replace(&mut self.sign_edit, host_sign);
        // A lectern the guest's use opened (LecternBlock.openScreen).
        let guest_lectern = match std::mem::replace(&mut self.book, host_book) {
            Some(super::books::BookScreen::View {
                lectern: Some(key), ..
            }) => Some(key),
            _ => None,
        };
        self.command_block_screen = host_command_block;
        // A mount inventory the guest's action opened (shift + use, or the
        // inventory key while riding).
        let guest_mount = std::mem::replace(&mut self.mount_menu, host_mount_menu).opened_mob();
        self.keys = host_keys;
        let g = &mut host.hub.guests[i];
        let left_vehicle = g.vehicle.filter(|_| guest_vehicle.is_none());
        if guest_vehicle.is_none() {
            g.vehicle_good = None;
        }
        g.vehicle = guest_vehicle;
        let fell_asleep = bed.is_some() && g.bed.is_none();
        (g.bed, g.sleep_timer, g.spawn) = (bed, guest_sleep_timer, guest_spawn);
        g.respawn_elsewhere = guest_respawn.map(|c| c.saved());
        g.sleeping = g.bed.is_some();
        if let Some((text, _)) = guest_message {
            g.send(&C::SystemChat(play::SystemChat {
                content: literal(&text),
                overlay: true,
            }));
        }
        std::mem::swap(&mut self.ender_items, &mut g.ender_items);
        std::mem::swap(&mut self.adv.progress, &mut g.advancements);
        std::mem::swap(&mut self.adv.changed, &mut g.advancements_changed);
        std::mem::swap(&mut self.adv.unlocked, &mut g.recipes_unlocked);
        std::mem::swap(&mut self.level.recipe_book, &mut g.recipe_book);
        std::mem::swap(&mut self.level.player_extra, &mut g.saved);
        std::mem::swap(&mut self.stats, &mut play.stats);
        std::mem::swap(&mut self.hook, &mut play.hook);
        self.adv.loaded = adv_loaded;
        play.last_inventory = std::mem::replace(&mut self.adv.last_inventory, last_inventory);
        play.levitation = std::mem::replace(&mut self.adv.levitation, levitation);
        let stat_events = std::mem::take(&mut play.stats.events);
        let name = g.name.clone();
        self.potions.swap_player(&mut g.effects, &mut play.potions);
        if let Some(key) = host_ender {
            self.world
                .block_entities
                .insert(key, BlockEntity::Chest(self.ender_items.clone()));
            self.ender_open = Some(key);
        }
        let new_xp = std::mem::replace(&mut self.experience, xp);
        g.xp_level = new_xp.level;
        g.xp_progress = new_xp.progress;
        g.xp_total = new_xp.total;
        let new_mode = self.mode_id();
        self.mode = mode;
        self.creative = creative;
        self.selected = std::mem::replace(&mut g.selected, selected);
        // `g.selected` now holds the host's value: swap the inventories back.
        std::mem::swap(&mut self.selected, &mut g.selected);
        std::mem::swap(&mut self.inventory, &mut g.inventory);
        std::mem::swap(&mut self.armor, &mut g.armor);
        std::mem::swap(&mut self.offhand, &mut g.offhand);
        std::mem::swap(&mut self.carried, &mut g.carried);
        // A teleport with a rotation (commands) turns the guest.
        let turned = [self.yaw, self.pitch] != [g.yaw, g.pitch];
        [g.yaw, g.pitch] = [self.yaw, self.pitch];
        self.yaw = yaw;
        self.pitch = pitch;
        self.eye = eye.1;
        std::mem::swap(&mut self.player, &mut player);
        if let Some(mut m) = player {
            g.health = m.health.health();
            g.food = m.health.food.food;
            g.saturation = m.health.food.saturation;
            // A teleport (commands, a pearl) moves the guest; a pearl's is
            // sent even to where they stand (ServerPlayer.teleport).
            let at = [m.position.x, m.position.y, m.position.z];
            // A hit with extra knockback stopped the guest's sprint
            // (Player.causeExtraKnockback's setSprinting(false)).
            if !m.sprinting {
                g.sprinting = false;
            }
            m.sprinting = false;
            // ServerPlayer.indicateDamage: the hit's direction to the player.
            if let Some(yaw) = m.health.indicated_dir.take() {
                g.send(&C::HurtAnimation(play::HurtAnimation {
                    id: g.entity_id,
                    yaw,
                }));
            }
            let teleported = std::mem::take(&mut m.teleported);
            let moved: f64 = (0..3).map(|k| (at[k] - g.position[k]).powi(2)).sum();
            g.motion = Some(Box::new(m));
            // Entity.teleportTo takes a rider out of its vehicle.
            let left_vehicle = match g.vehicle.filter(|_| teleported) {
                Some(v) => {
                    (g.vehicle, g.vehicle_good) = (None, None);
                    Some(v)
                }
                None => left_vehicle,
            };
            // ServerPlayer.dismountTo: the client hears it is out of the
            // vehicle before the teleport beside it. A rider sits where its
            // vehicle puts it (lan_vehicles.rs), with no teleport.
            if let Some(vehicle) = left_vehicle {
                g.send(&C::SetPassengers(play::SetPassengers {
                    vehicle: vehicle as i32,
                    passengers: Vec::new(),
                }));
            }
            if g.vehicle.is_none() && (moved > 0.0001 || turned || teleported) {
                host.hub.teleport(i, Some(at));
            }
        }
        let g = &mut host.hub.guests[i];
        if let Some(head) = g.bed.filter(|_| fell_asleep) {
            // LivingEntity.startSleeping: in the bed, still.
            let at = [
                f64::from(head[0]) + 0.5,
                f64::from(head[1]) + 0.5625,
                f64::from(head[2]) + 0.5,
            ];
            g.position = at;
            if let Some(m) = g.motion.as_mut() {
                m.position = Vec3::new(at[0], at[1], at[2]);
                m.velocity = Vec3::ZERO;
            }
        }
        if new_mode != g.game_mode {
            let conn = g.conn;
            host.hub.set_game_mode(conn, new_mode);
        }
        // LivingEntity.checkTotemDeathProtection's entity event, to every
        // client around, the guest's own included; this window draws it too.
        if let Some(body) = totem {
            let id = host.hub.guests[i].entity_id;
            host.hub
                .entity_event(id, super::totem::TOTEM_EVENT, body.at);
            if self.player.is_some() {
                self.totem_event(body, false);
            }
        }
        host.play.insert(conn, play);
        self.stat_criteria_changed(&name, stat_events);
        if let Some(pos) = opened
            && !self.lan_open_container(host, conn, pos)
            && !host.hub.guests[i].spectator()
        {
            // Nothing the guest can use opened: its lid closes again.
            self.container_openers_changed(pos, false);
        }
        if let Some(station) = opened_station {
            self.lan_station_opened(host, conn, station);
        }
        if let Some(edit) = guest_sign {
            self.lan_open_sign_editor(host, conn, edit);
        }
        if let Some(key) = guest_lectern {
            self.lan_open_lectern(host, conn, key);
        }
        if let Some(pos) = opened_table {
            self.lan_open_crafting(host, conn, pos);
        }
        if let Some(mob) = guest_mount {
            self.lan_open_mount(host, conn, mob);
        }
    }
    /// `ServerLevel.players()` in turn: the host's own player, then each
    /// guest in the level in place with their state swapped in (`as_guest`).
    /// What reaches the players around a point (a beacon's powers, a splash
    /// potion, a warden's darkness, an elder guardian's curse) lands on each
    /// of them, not only on the host. A server has no player of its own.
    /// Inside a guest's action (the hub taken) only that player is in place.
    pub(super) fn for_each_player(&mut self, mut f: impl FnMut(&mut Self)) {
        if self.player.is_some() {
            f(self);
        }
        let Some(mut host) = self.lan.take() else {
            return;
        };
        let conns: Vec<u32> = host
            .hub
            .guests
            .iter()
            .filter(|g| host.hub.here(g) && g.loaded && !g.dead)
            .map(|g| g.conn)
            .collect();
        for conn in conns {
            self.as_guest(&mut host, conn, &mut f);
        }
        self.lan = Some(host);
    }
    /// The game mode fields without `set_mode`'s side effects on the host.
    fn set_mode_raw(&mut self, id: i32) {
        use horizon_client::command::args::GameMode;
        self.mode = match id {
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => GameMode::Survival,
        };
        self.creative = self.mode == GameMode::Creative;
    }
    /// Hand a container the guest opened over (ServerPlayer.openMenu);
    /// false when it is not one a guest can open.
    fn lan_open_container(&mut self, host: &mut LanHost, conn: u32, pos: (i32, i32, i32)) -> bool {
        let name = &self
            .res
            .facts()
            .get(self.world.block(pos.0, pos.1, pos.2))
            .name;
        let (menu, title, slots) = if &**name == "minecraft:ender_chest" {
            // EnderChestBlock.useWithoutItem: the player's own inventory.
            (
                "minecraft:generic_9x3",
                "container.enderchest",
                Slots::Ender,
            )
        } else if let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get(&pos) {
            // AbstractFurnaceBlock.openContainer: FurnaceMenu, BlastFurnaceMenu, SmokerMenu.
            let (menu, title) = match f.kind.recipe_type() {
                "minecraft:blasting" => ("minecraft:blast_furnace", "container.blast_furnace"),
                "minecraft:smoking" => ("minecraft:smoker", "container.smoker"),
                _ => ("minecraft:furnace", "container.furnace"),
            };
            (menu, title, Slots::Block(pos))
        } else if let Some(BlockEntity::Crafter(_)) = self.world.block_entities.get(&pos) {
            // CrafterBlock.useWithoutItem: CrafterMenu over the block entity.
            (
                "minecraft:crafter_3x3",
                "container.crafter",
                Slots::Block(pos),
            )
        } else if let Some(BlockEntity::Brewing(_)) = self.world.block_entities.get(&pos) {
            // BrewingStandBlock.useWithoutItem: BrewingStandMenu.
            (
                "minecraft:brewing_stand",
                "container.brewing",
                Slots::Block(pos),
            )
        } else if let Some((other, right)) = self.chest_partner(pos) {
            // ChestBlock.combine: the right half's slots come first.
            let halves = if right {
                Slots::Double(pos, other)
            } else {
                Slots::Double(other, pos)
            };
            ("minecraft:generic_9x6", "container.chestDouble", halves)
        } else {
            let Some(BlockEntity::Chest(items)) = self.world.block_entities.get(&pos) else {
                return false;
            };
            let (menu, title) = match items.len() {
                27 => ("minecraft:generic_9x3", "container.chest"),
                9 => ("minecraft:generic_3x3", "container.dispenser"),
                5 => ("minecraft:hopper", "container.hopper"),
                _ => return false,
            };
            let title = match &**name {
                "minecraft:barrel" => "container.barrel",
                "minecraft:dropper" => "container.dropper",
                n if n.ends_with("shulker_box") => "container.shulkerBox",
                _ => title,
            };
            (menu, title, Slots::Block(pos))
        };
        let Some(items) = host
            .hub
            .guest(conn)
            .and_then(|g| slots.read(&self.world, g))
        else {
            return false;
        };
        // InventoryOpenEvent: cancelled, the use opens nothing.
        // ServerPlayer.openMenu closes the menu that was open.
        self.guest_container_closed(host, conn);
        self.lan_station_closed(host, conn, None);
        host.hub.open_container(
            conn,
            menu,
            horizon_protocol::text::translatable(title, &[]),
            pos,
            slots,
            items,
        );
        true
    }
    /// The guest's open container is closing (`AbstractContainerMenu.removed`):
    /// it no longer counts among the openers of the lid, and the lid's close
    /// sound is played for everyone around (`ChestBlockEntity.onClose`, a
    /// `playSound` with no player), the guest included, once the last opener
    /// has left.
    pub(super) fn guest_container_closed(&mut self, host: &mut LanHost, conn: u32) {
        if let Some(g) = host.hub.guest(conn)
            && let Some(pos) = g.menu.as_ref().and_then(|m| m.pos)
            && !g.spectator()
            && self.container_openers_changed(pos, false)
        {
            self.container_edge_sound(pos, false);
        }
    }
    pub(super) fn drop_guest_items(&mut self, host: &LanHost, gi: usize, stacks: Vec<ItemStack>) {
        let g = &host.hub.guests[gi];
        let eye = [
            g.position[0],
            g.position[1] + g.eye_height() - 0.3,
            g.position[2],
        ];
        let (yaw, pitch) = (
            f64::from(g.yaw).to_radians(),
            f64::from(g.pitch).to_radians(),
        );
        for stack in stacks {
            let v = [
                -yaw.sin() * pitch.cos() * 0.3,
                -pitch.sin() * 0.3 + 0.1,
                yaw.cos() * pitch.cos() * 0.3,
            ];
            self.spawn_item(stack, eye, v, 40);
        }
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
}

