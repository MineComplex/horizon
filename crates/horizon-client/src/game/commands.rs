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
use horizon_core::geometry::Vec3;

/// An entity a selector found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Target {
    /// The player in place: the host's, or the guest whose turn it is.
    Player,
    /// Another player of a hosted world or server: a guest by connection
    /// (game/commands/players.rs).
    Guest(u32),
    Mob(u32),
    Item(u32),
    /// An experience orb, by its entity id.
    Orb(u32),
    /// A projectile, by its entity id.
    Projectile(u32),
    Stand(u32),
    Frame(u32),
    Painting(u32),
    Boat(u32),
    Cart(u32),
    /// An entity of a type Horizon does not simulate (markers, displays,
    /// interactions...): kept as its saved data at a position.
    Other(u32),
    /// An End crystal, by its entity id (`minecraft:end_crystal`).
    Crystal(u32),
    /// The Ender Dragon, by its entity id (`minecraft:ender_dragon`).
    Dragon(u32),
}
/// An entity of a type Horizon does not simulate: its compound lives in the
/// entity memory (`persist.raw`), its position and rotation here.
#[derive(Clone, Debug)]
pub(in crate::game) struct OtherEntity {
    pub id: u32,
    pub kind: String,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
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
        let ops = self.world_data.rules.bool("send_command_feedback");
        // MinecraftServer.sendSystemMessage, unless the server is the source.
        let console = self.console_broadcast.is_none()
            && false
            && false
            && self.world_data.rules.bool("log_admin_commands");
        if !ops && !console {
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
        self.admin_feedback.push((line, ops, console));
    }
    /// Player.getDisplayName: the name (team-formatted), suggesting a
    /// `/tell`, showing the player on hover, inserting the name.
    pub(super) fn player_display_name(&self) -> Text {
        let _name = self.acting_player_name().to_owned();
        self.target_name(Target::Player)
    }
    pub(super) fn target_type(&self, t: Target) -> String {
        match t {
            Target::Player | Target::Guest(_) => "minecraft:player".into(),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or_else(String::new, |m| {
                    horizon_client::command::args::normalize(&m.kind.id)
                }),
            Target::Item(_) => "minecraft:item".into(),
            Target::Orb(_) => "minecraft:experience_orb".into(),
            Target::Projectile(id) => self
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .map_or_else(String::new, |p| p.kind.id().into()),
            Target::Stand(_) => "minecraft:armor_stand".into(),
            Target::Frame(id) => {
                if self.item_frames.iter().any(|f| f.id == id && f.glow) {
                    "minecraft:glow_item_frame".into()
                } else {
                    "minecraft:item_frame".into()
                }
            }
            Target::Painting(_) => "minecraft:painting".into(),
            Target::Boat(id) => self
                .boats
                .iter()
                .find(|b| b.id == id)
                .map_or_else(|| "minecraft:oak_boat".into(), |b| b.entity_type()),
            Target::Cart(id) => self
                .carts
                .iter()
                .find(|c| c.id == id)
                .map_or_else(|| "minecraft:minecart".into(), |c| c.kind.id().into()),
            Target::Other(id) => self
                .others
                .iter()
                .find(|o| o.id == id)
                .map_or_else(String::new, |o| o.kind.clone()),
            Target::Crystal(_) => "minecraft:end_crystal".into(),
            Target::Dragon(_) => "minecraft:ender_dragon".into(),
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
            Target::Guest(conn) => match self.command_guest(conn) {
                Some(g) => self.player_text(&g.name, g.uuid),
                None => Text::empty(),
            },
            Target::Item(id) => {
                let item = self
                    .dropped
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| i.stack.item.to_string())
                    .unwrap_or_default();
                Text::literal(self.pack.names.display_item(&item))
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
                // Entity.getDisplayName: the CustomName component, else the type.
                let custom = match other {
                    Target::Stand(id)
                    | Target::Frame(id)
                    | Target::Painting(id)
                    | Target::Boat(id)
                    | Target::Cart(id)
                    | Target::Other(id) => self.persist_raw(id).and_then(|r| r.get("CustomName")),
                    _ => None,
                };
                if let Some(c) = custom {
                    return horizon_client::scoreboard::comp_text(
                        &horizon_client::scoreboard::tag_to_json(c),
                    );
                }
                let ty = self.target_type(other);
                Text::translate(&format!("entity.{}", ty.replace(':', ".")), &[])
            }
        }
    }
    /// Whether the level in place holds the entity a target names.
    fn level_holds(&self, t: Target) -> bool {
        match t {
            // A player is in no level of its own.
            Target::Player | Target::Guest(_) => true,
            Target::Mob(id) => self.mobs.iter().any(|m| m.id == id),
            Target::Item(id) => self.dropped.iter().any(|i| i.id == id),
            Target::Orb(id) => self.orbs.iter().any(|o| o.id == id),
            Target::Projectile(id) => self.creatures.projectiles.iter().any(|p| p.id == id),
            Target::Stand(id) => self.armor_stands.iter().any(|a| a.id == id),
            Target::Frame(id) => self.item_frames.iter().any(|f| f.id == id),
            Target::Painting(id) => self.paintings.iter().any(|p| p.id == id),
            Target::Boat(id) => self.boats.iter().any(|b| b.id == id),
            Target::Cart(id) => self.carts.iter().any(|c| c.id == id),
            Target::Other(id) => self.others.iter().any(|o| o.id == id),
            Target::Crystal(id) => self.crystals.iter().any(|c| c.id == id),
            Target::Dragon(id) => self.dragon.as_ref().is_some_and(|d| d.id == id),
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
