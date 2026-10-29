//! Command execution (the `execute` side of each vanilla command class):
//! entity selectors resolved against the game, vanilla feedback and errors.
mod admin;
pub(super) mod compute;
pub(super) mod data;
pub(super) mod execute;
pub(super) mod extra;
mod misc;
mod players;
pub(super) mod pregen;
mod terrain_cmds;
mod world_cmds;

use super::{Game, chat::PLAYER_NAME};
use horizon_client::text::{self, Text};

/// An entity a selector found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    /// The player in place: the host's, or the guest whose turn it is.
    Player,
    Mob(u32),
}

impl Game<'_> {
    // ---- entities -------------------------------------------------------

    // ---- dispatch -------------------------------------------------------
}
impl crate::game::Sim<'_> {
    /// sendSuccess honouring `send_command_feedback`.
    pub(super) fn feedback(&mut self, message: Text) {
        if self.command_source.is_some_and(|s| s.silent) {
            return;
        }
        if let Some(block) = self.command_source.and_then(|s| s.block) {
            self.command_block_success(block, message);
            return;
        }
        if self.world_data.rules.bool("send_command_feedback") {
            self.system_message(message.clone());
        }
        self.broadcast_to_admins(message);
    }
    /// CommandSourceStack.broadcastToAdmins (`sendSuccess` with `broadcast`)
    /// for a guest's command, the host's in a world open to LAN and the
    /// dedicated server console's when `broadcast-console-to-ops` is on:
    /// `[name: message]`, gray and italic, for the other operators when
    /// `send_command_feedback`, and for a player's command on a dedicated
    /// server the console when `log_admin_commands`. Queued while the
    /// command runs; `send_admin_feedback` sends it.
    pub(super) fn broadcast_to_admins(&mut self, message: Text) {
        let informs = self.console_broadcast.unwrap_or_default();
        if !informs
            || message
                .translation_key()
                .is_some_and(|k| admin::QUIET.contains(&k))
        {
            return;
        }
        if !self.world_data.rules.bool("send_command_feedback") {
            return;
        }
        // getDisplayName: the entity the command runs as (`execute as`), or
        // the server's own source, "Server".
        let who = match self.command_source.and_then(|s| s.entity) {
            Some(entity) => self.target_name(entity),
            None if self.console_broadcast.is_some() => Text::literal("Server".to_owned()),
            None => self.player_display_name(),
        };
        let line = Text::translate("chat.type.admin", &[who, message])
            .with_color(text::GRAY)
            .italic();
        self.admin_feedback.push((line, true, false));
    }
    /// Player.getDisplayName: the name (team-formatted), suggesting a
    /// `/tell`, showing the player on hover, inserting the name.
    pub(super) fn player_display_name(&self) -> Text {
        let _name = self.acting_player_name().to_owned();
        self.target_name(Target::Player)
    }
    pub(super) fn target_type(&self, t: Target) -> String {
        match t {
            Target::Player => "minecraft:player".into(),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or_else(String::new, |m| {
                    horizon_client::command::args::normalize(&m.kind.id)
                }),
        }
    }
    /// The name of the player the game acts for: a guest's own while their
    /// command or action runs (`Player.getName`).
    pub(super) fn acting_player_name(&self) -> &str {
        None::<String>.as_deref().unwrap_or(PLAYER_NAME)
    }
    /// Entity.getDisplayName.
    pub(super) fn target_name(&self, t: Target) -> Text {
        match t {
            Target::Player => {
                let name = self.acting_player_name();
                match self.score.board.players_team(name) {
                    // PlayerTeam.formatNameForTeam.
                    Some(team) => team.formatted_name(Text::literal(name)),
                    None => Text::literal(name),
                }
            }
            Target::Mob(id)
                if self
                    .mobs
                    .iter()
                    .any(|m| m.id == id && m.custom_name.is_some()) =>
            {
                let name = self
                    .mobs
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| m.custom_name.clone());
                Text::literal(name.unwrap_or_default())
            }
            other => {
                // Entity.getDisplayName: the type's name.
                let ty = self.target_type(other);
                Text::translate(&format!("entity.{}", ty.replace(':', ".")), &[])
            }
        }
    }
    /// Whether the level in place holds the entity a target names.
    fn level_holds(&self, t: Target) -> bool {
        match t {
            // A player is in no level of its own.
            Target::Player => true,
            Target::Mob(id) => self.mobs.iter().any(|m| m.id == id),
        }
    }
    /// Run `f` with the level that holds the entity a target names in place
    /// (the entity ids are unique across the loaded levels) and put the
    /// level back afterwards. `f` does not run when no level holds the
    /// entity.
    pub(in crate::game) fn in_target_level<R>(
        &mut self,
        t: Target,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if self.level_holds(t) {
            return Some(f(self));
        }
        for dim in self.levels.parked_dims() {
            let back = self.enter_command_level(dim);
            if self.level_holds(t) {
                let r = f(self);
                if let Some(back) = back {
                    self.enter_command_level(back);
                }
                return Some(r);
            }
            if let Some(back) = back {
                self.enter_command_level(back);
            }
        }
        None
    }
    pub(super) fn item_rarity(&self, item: &str) -> String {
        static RARITY: std::sync::OnceLock<std::collections::HashMap<String, String>> =
            std::sync::OnceLock::new();
        let map = RARITY.get_or_init(|| {
            let v: serde_json::Value = serde_json::from_slice(
                &std::fs::read(self.options.resources.join("items.json")).unwrap_or_default(),
            )
            .unwrap_or_default();
            v.as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, c)| {
                            c["minecraft:rarity"]
                                .as_str()
                                .map(|r| (k.clone(), r.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        map.get(item).cloned().unwrap_or_else(|| "common".into())
    }
    /// The structures of a `worldgen/structure` tag, nested tags expanded.
    pub(super) fn structure_tag(&self, tag: &str) -> Vec<String> {
        self.tag_entries("worldgen/structure", tag)
    }
    /// The entries of a data pack tag of `registry`, nested tags expanded.
    pub(super) fn tag_entries(&self, registry: &str, tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut pending = vec![tag.to_owned()];
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = pending.pop() {
            if !seen.insert(t.clone()) {
                continue;
            }
            let (ns, path) = t.split_once(':').unwrap_or(("minecraft", &t));
            let file = self
                .options
                .resources
                .join(format!("data/{ns}/tags/{registry}/{path}.json"));
            let Ok(v) = std::fs::read(file)
                .map_err(|_| ())
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ()))
            else {
                continue;
            };
            for e in v["values"].as_array().into_iter().flatten() {
                let id = e.as_str().or_else(|| e["id"].as_str()).unwrap_or_default();
                match id.strip_prefix('#') {
                    Some(nested) => pending.push(nested.to_owned()),
                    None if !id.is_empty() => out.push(id.to_owned()),
                    None => {}
                }
            }
        }
        out
    }
}
