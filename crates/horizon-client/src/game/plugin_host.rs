//! horizon-server's plugins (crate `horizon-server-api`, Bukkit-shaped) in
//! the game: the dedicated server loads and enables them, fires their events
//! where vanilla's server would (`PlayerList.placeNewPlayer`, chat, commands,
//! `ServerPlayerGameMode.destroyBlock`, `BlockItem.place`, `ServerPlayer.die`),
//! runs their commands and tasks, and applies what they asked for once their
//! handler returns (`apply_plugin_actions`), through the server's own commands
//! so every change reaches the players as any other does.
use super::{Game, lan::LanHost};
use horizon_client::lan::guest::Guest;
use horizon_server_api::{
    self as api, BlockPos, Location, PlayerRef, Plugin,
    event::{self as ev, Event}};
use std::sync::Mutex;

/// The plugins `horizon-server` was built with, until the server starts.
static PLUGINS: Mutex<Option<Vec<Box<dyn Plugin>>>> = Mutex::new(None);

/// The plugins the next dedicated server runs (`horizon-server`'s, before `run_with`).
pub fn set_plugins(plugins: Vec<Box<dyn Plugin>>) {
    *PLUGINS.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugins);
}

/// What plugins read of the server.
struct View<'a> {
    game: &'a crate::game::Sim<'a>,
    players: Vec<PlayerRef>,
}
impl api::server::View for View<'_> {
    fn players(&self) -> Vec<PlayerRef> {
        self.players.clone()
    }
    fn block(&self, world: &str, pos: BlockPos) -> Option<String> {
        let g = self.game;
        if world != g.dim.id() {
            return None;
        }
        if !g.world.chunks.contains_key(&(pos.x >> 4, pos.z >> 4)) {
            return None;
        }
        let state = g.world.block(pos.x, pos.y, pos.z);
        Some(g.state_string(state))
    }
    fn tick(&self) -> u64 {
        self.game.ticks
    }
    fn world_name(&self) -> String {
        self.game.level.name.clone()
    }
    fn max_players(&self) -> usize {
        self.game
            .lan
            .as_ref()
            .map_or(0, |h| h.hub.lan.shared.max_players())
    }
    fn time(&self) -> i64 {
        self.game.world.day_time.rem_euclid(24000)
    }
}

impl Game<'_> {
    /// A player as plugins see them.
    fn plugin_player(g: &Guest) -> PlayerRef {
        PlayerRef {
            name: g.name.clone(),
            uuid: g.uuid,
            location: Location {
                world: g.dimension.clone(),
                x: g.position[0],
                y: g.position[1],
                z: g.position[2],
                yaw: g.yaw,
                pitch: g.pitch,
            },
            health: g.health,
            game_mode: g.game_mode.clamp(0, 3) as u8,
            op_level: g.permission,
        }
    }

    // ---- The events -----------------------------------------------------
}
impl crate::game::Sim<'_> {
    fn plugin_log(&mut self) {
        let Some(host) = self.plugins.as_mut() else {
            return;
        };
        for line in host.take_log() {
            super::dedicated::log("INFO", &line);
        }
    }
    /// The online players, from the hub in hand or in place, else the last
    /// ones seen (the hub is out of its place during a player's turn).
    fn plugin_players(&mut self, host: Option<&LanHost>) -> Vec<PlayerRef> {
        if let Some(h) = host.or(self.lan.as_ref()) {
            let players: Vec<PlayerRef> = h
                .hub
                .guests
                .iter()
                .filter(|g| g.placed)
                .map(crate::game::Game::plugin_player)
                .collect();
            self.plugin_seen = players.clone();
            return players;
        }
        self.plugin_seen.clone()
    }
    /// The player whose turn it is (`as_guest`).
    fn plugin_acting(&self) -> Option<PlayerRef> {
        let uuid = self.lan_acting_uuid?;
        let mut p = self.plugin_seen.iter().find(|p| p.uuid == uuid)?.clone();
        // The turn's own state is in the game.
        if let Some(m) = self.player.as_ref() {
            p.location = Location {
                world: self.dim.id().to_owned(),
                x: m.position.x,
                y: m.position.y,
                z: m.position.z,
                yaw: self.yaw,
                pitch: self.pitch,
            };
            p.health = m.health.health();
        }
        Some(p)
    }
    fn plugins_listen<E: Event>(&self) -> bool {
        self.plugins.as_ref().is_some_and(|h| h.listens::<E>())
    }
    /// `callEvent`; what the listeners asked for waits for `apply_plugin_actions`.
    fn plugin_fire<E: Event>(&mut self, host: Option<&LanHost>, event: &mut E) {
        let players = self.plugin_players(host);
        let Some(mut plugins) = self.plugins.take() else {
            return;
        };
        let actions = {
            let view = View {
                game: self,
                players,
            };
            plugins.call(event, &view)
        };
        self.plugins = Some(plugins);
        self.plugin_actions.extend(actions);
        self.plugin_log();
    }
    /// The player whose turn it is breaks the block at `pos`:
    /// `BlockBreakEvent`. None when a plugin keeps it (the player's client,
    /// which broke it already, gets it back), else whether it drops its items.
    pub(super) fn plugin_block_break(&mut self, pos: [i32; 3]) -> Option<bool> {
        if !self.plugins_listen::<ev::BlockBreakEvent>() {
            return Some(true);
        }
        let Some(player) = self.plugin_acting() else {
            return Some(true);
        };
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let mut event = ev::BlockBreakEvent {
            player,
            world: self.dim.id().to_owned(),
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            block: self.state_string(state),
            drop_items: true,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        if event.cancelled {
            if let Some(conn) = self.lan_acting {
                self.plugin_resend.push((conn, pos, state.0 as i32));
            }
            return None;
        }
        Some(event.drop_items)
    }
    /// The player whose turn it is placed `state` at `pos` over `replaced`:
    /// `BlockPlaceEvent`. False when a plugin cancelled it.
    pub(super) fn plugin_block_place(
        &mut self,
        pos: [i32; 3],
        state: horizon_world::registry::BlockStateId,
        replaced: horizon_world::registry::BlockStateId,
        item: Option<api::ItemStack>,
    ) -> bool {
        if !self.plugins_listen::<ev::BlockPlaceEvent>() {
            return true;
        }
        let Some(player) = self.plugin_acting() else {
            return true;
        };
        let mut event = ev::BlockPlaceEvent {
            player,
            world: self.dim.id().to_owned(),
            pos: BlockPos::new(pos[0], pos[1], pos[2]),
            block: self.state_string(state),
            replaced: self.state_string(replaced),
            item,
            cancelled: false,
        };
        self.plugin_fire(None, &mut event);
        !event.cancelled
    }
}

