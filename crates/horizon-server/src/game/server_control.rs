//! What a client and the test tools ask a server on its control endpoint
//! (`horizon-server --control-socket PATH`), beside `server-status` and
//! `console` (dedicated/status.rs).
//!
//! A test drives a real client (`horizon-client --server ... --server-control
//! PATH`); what only the server knows (its mobs, its blocks, the world's clock,
//! its saves) the client asks here, as `as NAME WORDS...` for the player NAME:
//!
//! - `command TEXT`: run a command as the player, at the console's permission,
//!   in the player's level; answers with the feedback it got
//! - `summon KIND [X Y Z] [baby]`: a mob (3 blocks in front of the player
//!   without a position); answers with its entity id
//! - `give ITEM [COUNT]`, `time TICKS`, `mobs RADIUS`, `hurtlog`, `simulation`,
//!   `server-hold on|off`, `save`, `save-all`
//! - `setblock`, `fill`, `getblock`, `container`: the block commands
//!   (game/blockctl.rs) in the player's level
//! - `lan open [PORT] [commands] [force] | lan status | lan close`: the port the
//!   server listens on, and its players (what `horizon-ctl lan` reports)
//!
//! And for the tools: `world` (the level's name and the port), `exit-with PID`
//! (stop when that process ends: the client a test started the server for).
use crate::hc::Result;
use horizon_core::geometry::Vec3;
use serde_json::{Value, json};

impl crate::game::Sim<'_> {
    /// One control request beyond `server-status` and `console`; None when
    /// it is not one of them.
    pub(super) fn server_control(&mut self, line: &str) -> Option<Value> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let result = match words.as_slice() {
            ["world"] => Ok(json!({
                "world": self.level.name,
                "port": self.network.as_ref().map(|h| h.settings.port),
            })),
            ["as", name, rest @ ..] if !rest.is_empty() => self.player_control(name, line, rest),
            _ => return None,
        };
        Some(result.unwrap_or_else(|e| json!({ "error": e.to_string() })))
    }
    /// The player called `name`: its profile.
    fn player_uuid(&self, name: &str) -> Result<u128> {
        self.network
            .as_ref()
            .and_then(|h| h.hub.players.iter().find(|g| g.name == name && g.placed))
            .map(|g| g.uuid)
            .ok_or_else(|| format!("{name} is not on this server").into())
    }
    fn player_control(&mut self, name: &str, line: &str, words: &[&str]) -> Result<Value> {
        let uuid = self.player_uuid(name)?;
        let ok = || Ok(json!({ "ok": true }));
        match words {
            ["command", ..] => {
                // The command's text as typed, spaces and all.
                let text = line
                    .split_once(" command ")
                    .map_or("", |(_, t)| t)
                    .trim()
                    .trim_start_matches('/');
                if text.is_empty() {
                    return Err("command TEXT".into());
                }
                let chat = self.player_console_command(uuid, text);
                Ok(json!({ "ok": true, "chat": chat }))
            }
            ["give", item, rest @ ..] => {
                let count: u32 = rest.first().map_or(Ok(1), |c| c.parse())?;
                self.player_console_command(uuid, &format!("give @s {item} {count}"));
                Ok(json!({ "ok": true, "given": count }))
            }
            ["time", ticks] => {
                let time: i64 = ticks.parse()?;
                self.world.day_time = time;
                ok()
            }
            ["mobs", radius] => {
                let radius: f64 = radius.parse()?;
                let center = self
                    .network
                    .as_ref()
                    .and_then(|h| h.hub.players.iter().find(|g| g.uuid == uuid))
                    .map_or(Vec3::ZERO, |g| {
                        Vec3::new(g.position[0], g.position[1], g.position[2])
                    });
                Ok(self.in_player_level(uuid, |g| g.control_mobs_at(radius, center)))
            }
            ["summon", kind, rest @ ..] => {
                let baby = rest.contains(&"baby");
                let nums: Vec<f64> = rest.iter().filter_map(|w| w.parse().ok()).collect();
                let at = match nums.as_slice() {
                    [x, y, z, ..] => [*x, *y, *z],
                    _ => {
                        let g = self
                            .network
                            .as_ref()
                            .and_then(|h| h.hub.players.iter().find(|g| g.uuid == uuid))
                            .ok_or("no such player")?;
                        let yaw = f64::from(g.yaw).to_radians();
                        let p = g.position;
                        [p[0] - yaw.sin() * 3.0, p[1], p[2] + yaw.cos() * 3.0]
                    }
                };
                let kind = if kind.contains(':') {
                    (*kind).to_owned()
                } else {
                    format!("minecraft:{kind}")
                };
                let id = self.in_player_level(uuid, |g| g.summon(&kind, at, baby))?;
                Ok(json!({ "ok": true, "id": id }))
            }
            ["hurtlog"] => Ok(self.control_hurt_log()),
            ["simulation"] => Ok(self.in_player_level(uuid, |g| {
                json!({ "game_time": g.world.game_time, "projectiles": g.projectiles_status() })
            })),
            ["server-hold", on] => {
                self.agent.hold_server = match *on {
                    "on" => true,
                    "off" => false,
                    _ => return Err("server-hold on|off".into()),
                };
                ok()
            }
            ["save"] => self.control_save(false).and_then(|()| ok()),
            ["save-all"] => self.control_save(true).and_then(|()| ok()),
            [first, ..] if matches!(*first, "setblock" | "fill" | "getblock" | "container") => {
                let reply = self.in_player_level(uuid, |g| g.block_command(words))?;
                Ok(reply.unwrap_or_else(|| json!({ "ok": true })))
            }
            ["network", rest @ ..] => self.server_net(rest),
            _ => Err(format!("unknown request {line:?}").into()),
        }
    }
    /// `horizon-ctl lan` on a server: it is always open, on its own port.
    /// `commands` lets every player run commands, `force` forces the default
    /// game mode (Open to LAN's options).
    fn server_net(&mut self, words: &[&str]) -> Result<Value> {
        match words.first().copied() {
            Some("open") => {
                let net = self.network.as_mut().ok_or("the server is not listening")?;
                net.settings.player_commands = words.contains(&"commands");
                net.settings.force_game_mode = words.contains(&"force");
                net.hub.config.force_game_mode = net.settings.force_game_mode;
                net.hub.everyone_level = if net.settings.player_commands { 2 } else { 0 };
                net.hub.resend_commands();
                Ok(json!({ "ok": true, "port": net.settings.port }))
            }
            Some("status") => Ok(self.net_status()),
            Some("close") => Err("a server does not close".into()),
            _ => Err("network open|close|status".into()),
        }
    }
    /// Run `f` with the level of the player `uuid` in place (the server holds
    /// one per dimension), then put back the level that was.
    pub(super) fn in_player_level<T>(&mut self, uuid: u128, f: impl FnOnce(&mut Self) -> T) -> T {
        let dim = self.network.as_ref().and_then(|h| {
            let g = h.hub.players.iter().find(|g| g.uuid == uuid)?;
            Some(super::Dim::from_id(&g.dimension))
        });
        let back = dim.and_then(|d| self.enter_command_level(d));
        let out = f(self);
        if let Some(back) = back {
            self.enter_command_level(back);
        }
        out
    }
    /// A command as the player `uuid`, run as the console runs one (its
    /// permission), in the player's level: `execute as` the player. The
    /// feedback goes to the player's chat and is returned.
    pub(super) fn player_console_command(&mut self, uuid: u128, command: &str) -> Vec<String> {
        let Some(mut net) = self.network.take() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(g) = net.hub.players.iter().find(|g| g.uuid == uuid && g.placed) {
            let conn = g.conn;
            // Commands.performCommand runs in the player's level.
            let (back, theirs) = (self.dim, super::Dim::from_id(&g.dimension));
            let entered = theirs == back || self.enter_level(Some(&mut net), theirs).is_ok();
            if entered && let Some(swap) = self.enter_player(&mut net, conn) {
                let command = command.trim_start_matches('/').to_owned();
                // The hub is back in place while the command runs, as for a
                // player's own (`run_player_commands`): selectors see every
                // player (`team join … @a`), and `defaultgamemode` reaches it.
                self.network = Some(net);
                let lines = self.as_console(|g| {
                    g.chat_capture = Some(Vec::new());
                    g.run_command(&command);
                    g.chat_capture.take().unwrap_or_default()
                });
                net = self.network.take().expect("a command puts the hub back");
                self.leave_player(&mut net, swap);
                net.hub.sync_all_effects();
                // A teleport into another level happens before its feedback is
                // sent, and what the command earned is announced before it too.
                self.run_level_teleports(&mut net);
                self.announce_player_advancements(&mut net);
                for line in lines {
                    out.push(line.plain());
                    net.hub.send_to(
                        conn,
                        &horizon_protocol::packets::play::Clientbound::SystemChat(
                            horizon_protocol::packets::play::SystemChat {
                                content: crate::hc::server_net::data::component_nbt(
                                    &line.component(),
                                ),
                                overlay: false,
                            },
                        ),
                    );
                }
                self.send_admin_feedback(&mut net, Some(conn));
            }
            if theirs != back
                && let Err(e) = self.enter_level(Some(&mut net), back)
            {
                log::error!("could not reopen {}: {e}", back.id());
            }
        }
        self.network = Some(net);
        out
    }
}

impl crate::game::Sim<'_> {}

/// What the control socket holds for a test (`server-hold`).
#[derive(Default)]
pub(super) struct Agent {
    /// Control-fixture barrier: hold simulation and packet handling, not transport threads.
    pub hold_server: bool,
}
