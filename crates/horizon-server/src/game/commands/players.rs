//! Commands acting on the other players of a server or a server
//! (EntitySelector.findPlayers and EntityArgument's player targets). The
//! player in place (the game's, or the player whose turn it is) is
//! `Target::Player`; every other player is `Target::Player`, found by name,
//! UUID or selector, and what a command does to them runs with their state
//! swapped in (`for_player`), as their own turn, what it tells them going to
//! their chat.
use super::Target;
use crate::hc::{command::args::Selector, server_net::player::ServerPlayer, text::Text};
use horizon_protocol::packets::play::{self, Clientbound as C};

impl Target {
    /// A player, in place or not (`instanceof ServerPlayer`).
    pub(in crate::game) fn is_player(self) -> bool {
        matches!(self, Target::Player | Target::ServerPlayer(_))
    }
}

/// GameType.getName for a player's game mode id.
fn mode_name(id: i32) -> &'static str {
    match id {
        1 => "creative",
        2 => "adventure",
        3 => "spectator",
        _ => "survival",
    }
}

impl crate::game::Sim<'_> {
    /// The player `conn`, while the hub is in `self.network`.
    pub(in crate::game) fn command_player(&self, conn: u32) -> Option<&ServerPlayer> {
        self.network.as_ref()?.hub.player(conn)
    }
    /// `PlayerList.getPlayers` but the player in place: the players who
    /// joined the world.
    fn command_players(&self) -> impl Iterator<Item = &ServerPlayer> {
        let acting = self.net_acting;
        self.network
            .iter()
            .flat_map(|h| h.hub.players.iter())
            .filter(move |g| g.placed && Some(g.conn) != acting)
    }
    /// The entity the command runs as (`CommandSourceStack.getEntity`): a
    /// command typed by the player in place runs as them, the server
    /// console's as nobody.
    pub(in crate::game) fn source_entity(&self) -> Option<Target> {
        match self.command_source {
            Some(s) => s.entity,
            None => self.player.is_some().then_some(Target::Player),
        }
    }
    /// `CommandSourceStack.getPlayerOrException` for the commands whose
    /// player argument is optional: the source's player (the player in
    /// place when it has none, as before).
    pub(in crate::game) fn implied_player(&self) -> Target {
        self.source_entity()
            .filter(|t| t.is_player())
            .unwrap_or(Target::Player)
    }
    /// `PlayerList.getPlayerByName`: names are not case sensitive.
    pub(super) fn player_named(&self, name: &str) -> Option<Target> {
        if self.player.is_some() && name.eq_ignore_ascii_case(self.acting_player_name()) {
            return Some(Target::Player);
        }
        self.command_players()
            .find(|g| g.name.eq_ignore_ascii_case(name))
            .map(|g| Target::ServerPlayer(g.conn))
    }
    /// `EntitySelector.findEntities` with a UUID: the player with it, else
    /// the entity.
    pub(super) fn target_by_uuid(&self, uuid: u128) -> Option<Target> {
        let own = self.net_acting_uuid.or_else(|| {
            self.level.player_uuid.map(|[a, b, c, d]| {
                (u128::from(a as u32) << 96)
                    | (u128::from(b as u32) << 64)
                    | (u128::from(c as u32) << 32)
                    | u128::from(d as u32)
            })
        });
        if self.player.is_some() && own == Some(uuid) {
            return Some(Target::Player);
        }
        if let Some(g) = self.command_players().find(|g| g.uuid == uuid) {
            return Some(Target::ServerPlayer(g.conn));
        }
        let words = [96, 64, 32, 0].map(|shift| (uuid >> shift) as u32 as i32);
        self.target_with_uuid(words)
    }
    /// The players a selector looks at, in join order (`PlayerList.getPlayers`),
    /// the one in place as `Target::Player`: all of them, or those in the
    /// source's level when it limits the search to a place
    /// (`isWorldLimited`); `@e` and `@n` skip the dead (`Entity::isAlive`).
    pub(super) fn player_candidates(&self, s: &Selector) -> Vec<Target> {
        let limited = s.distance.is_some()
            || s.pos.iter().any(Option::is_some)
            || s.delta.iter().any(Option::is_some);
        let alive = matches!(s.kind, 'e' | 'n');
        let here = self.dim.id();
        let in_place = self.net_acting.filter(|_| self.player.is_some());
        self.network
            .iter()
            .flat_map(|h| h.hub.players.iter())
            .filter(|g| g.placed)
            .filter(|g| !limited || g.dimension == here || Some(g.conn) == in_place)
            .filter(|g| !alive || (!g.dead && g.health > 0.0))
            .map(|g| {
                if Some(g.conn) == in_place {
                    Target::Player
                } else {
                    Target::ServerPlayer(g.conn)
                }
            })
            .collect()
    }
    /// A player's experience level and game mode, for `level=` and `gamemode=`.
    pub(super) fn player_level_and_mode(&self, conn: u32) -> Option<(i32, &'static str)> {
        self.command_player(conn)
            .map(|g| (g.xp_level, mode_name(g.game_mode)))
    }
    /// `Player.getDisplayName`: the name as its team shows it, suggesting a
    /// `/tell`, showing the player on hover, inserting the name.
    pub(in crate::game) fn player_text(&self, name: &str, uuid: u128) -> Text {
        let shown = match self.score.board.players_team(name) {
            Some(team) => team.formatted_name(Text::literal(name)),
            None => Text::literal(name),
        };
        shown.with_component_fields(vec![
            (
                "click_event".into(),
                serde_json::json!({"action": "suggest_command", "command": format!("/tell {name} ")}),
            ),
            (
                "hover_event".into(),
                serde_json::json!({"action": "show_entity", "id": "minecraft:player",
                    "uuid": horizon_protocol::crypto::uuid_string(uuid), "name": name}),
            ),
            ("insertion".into(), name.into()),
        ])
    }
    /// Run `f` for the player `t` (None for anything else): in place, or
    /// with the player's state swapped in, in their level, as their own turn.
    /// What `f` tells the player (`system_message`) goes to their chat; the
    /// command's own feedback is the caller's to send.
    pub(in crate::game) fn for_player<T>(
        &mut self,
        t: Target,
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        let conn = match t {
            Target::Player => return Some(f(self)),
            Target::ServerPlayer(conn) if self.net_acting == Some(conn) => return Some(f(self)),
            Target::ServerPlayer(conn) => conn,
            _ => return None,
        };
        let mut net = self.network.take()?;
        let back = self.dim;
        let theirs = net
            .hub
            .player(conn)
            .map(|g| super::super::Dim::from_id(&g.dimension));
        if let Some(dim) = theirs.filter(|d| *d != back)
            && let Err(e) = self.enter_level(Some(&mut net), dim)
        {
            log::error!("could not open {}: {e}", dim.id());
        }
        let Some(swap) = self.enter_player(&mut net, conn) else {
            if self.dim != back
                && let Err(e) = self.enter_level(Some(&mut net), back)
            {
                log::error!("could not reopen {}: {e}", back.id());
            }
            self.network = Some(net);
            return None;
        };
        self.network = Some(net);
        let source = self.chat_capture.replace(Vec::new());
        let out = f(self);
        let told = std::mem::replace(&mut self.chat_capture, source).unwrap_or_default();
        let mut net = self
            .network
            .take()
            .expect("the hub is back after a player's turn");
        self.leave_player(&mut net, swap);
        for line in told {
            net.hub.send_to(
                conn,
                &C::SystemChat(play::SystemChat {
                    content: crate::hc::server_net::data::component_nbt(&line.component()),
                    overlay: false,
                }),
            );
        }
        if self.dim != back
            && let Err(e) = self.enter_level(Some(&mut net), back)
        {
            log::error!("could not reopen {}: {e}", back.id());
        }
        self.network = Some(net);
        Some(out)
    }
}
