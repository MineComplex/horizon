//! The scoreboard in the running game: saved data, criteria that follow the
//! player (health, food, xp, deaths, kills, statistics), score holders for
//! entities, the selector options `scores=`, `team=` and `tag=`, the
//! commands' [`Env`] and the sidebar.
use super::{chat::PLAYER_NAME, commands::Target};
use crate::hc::{
    command::{
        Context,
        args::{IntRange, Selector},
    },
    save::{self, uuid_from_text, uuid_text},
    scoreboard::{self as sb, Comp, Scoreboard, literal},
    scoreboard_cmd::{self, Env, Holder},
    text::Text,
};
use horizon_world::nbt::Tag;
use std::collections::HashSet;

/// What `ServerPlayer` last recorded for the criteria that follow the player.
#[derive(Default)]
pub(super) struct Recorded {
    /// `lastRecordedHealthAndAbsorption` (compared as a float).
    health_and_absorption: Option<f32>,
    food: Option<i32>,
    air: Option<i32>,
    armor: Option<i32>,
    xp: Option<i32>,
    level: Option<i32>,
}
#[derive(Default)]
pub(super) struct ScoreState {
    pub board: Scoreboard,
    /// `/data ... storage`.
    pub storage: crate::hc::command_storage::CommandStorage,
    pub stopwatches: crate::hc::stopwatches::Stopwatches,
    loaded: bool,
    recorded: Recorded,
}

fn in_range(v: i32, (lo, hi): IntRange) -> bool {
    lo.is_none_or(|l| v >= l) && hi.is_none_or(|h| v <= h)
}

impl crate::game::Sim<'_> {
    fn scoreboard_path(&self) -> std::path::PathBuf {
        self.world_root.join("data/minecraft/scoreboard.dat")
    }
    /// `ScoreboardSaveData` read from `data/minecraft/scoreboard.dat`.
    pub(super) fn load_scoreboard(&mut self) {
        if self.score.loaded {
            return;
        }
        self.score.loaded = true;
        if let Some(root) = save::read_dat(&self.scoreboard_path())
            && let Some(data) = root.get("data")
        {
            self.score.board = Scoreboard::unpack(data);
        }
        self.score.storage = crate::hc::command_storage::CommandStorage::load(&self.world_root);
        if let Some(root) = save::read_dat(&self.world_root.join("data/minecraft/world_border.dat"))
            && let Some(data) = root.get("data")
        {
            self.border = crate::hc::worldborder::WorldBorder::unpack(data);
        }
        self.score.stopwatches = crate::hc::stopwatches::Stopwatches::load(&self.world_root);
        self.sync_score_registries();
    }
    pub(super) fn save_scoreboard(&mut self) {
        if !self.score.loaded {
            return;
        }
        self.score.storage.save(&self.world_root);
        self.score.stopwatches.save(&self.world_root);
        if self.border != crate::hc::worldborder::WorldBorder::default() {
            let _ = save::write_dat(
                &self.world_root.join("data/minecraft/world_border.dat"),
                Tag::compound([
                    ("data", self.border.pack()),
                    ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
                ]),
            );
        }
        let path = self.scoreboard_path();
        let board = &self.score.board;
        if board == &Scoreboard::default() && !path.exists() {
            return;
        }
        let _ = save::write_dat(
            &path,
            Tag::compound([
                ("data", board.pack()),
                ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            ]),
        );
    }
    /// The names commands complete: objectives, teams and tracked holders.
    pub(super) fn sync_score_registries(&mut self) {
        let board = &self.score.board;
        let reg = &mut self.world_data.registries;
        reg.objectives = board.objectives.iter().map(|o| o.name.clone()).collect();
        // Trigger objectives: the locks `/trigger` and `scoreboard players enable` complete from.
        reg.trigger_scores.clear();
        for o in board.objectives.iter().filter(|o| o.criteria == "trigger") {
            let locks = board
                .scores
                .iter()
                .filter_map(|(holder, s)| s.get(&o.name).map(|s| (holder.clone(), s.locked)))
                .collect();
            reg.trigger_scores.insert(o.name.clone(), locks);
        }
        reg.trigger_objectives = reg
            .trigger_scores
            .iter()
            .filter(|(_, locks)| locks.get(PLAYER_NAME) == Some(&false))
            .map(|(o, _)| o.clone())
            .collect();
        reg.teams = board.teams.iter().map(|t| t.name.clone()).collect();
        reg.tracked = board.tracked();
        reg.storages = self.score.storage.keys();
        reg.stopwatches = self.score.stopwatches.ids();
    }
    /// `ScoreHolder.getScoreboardName`: the player's name, an entity's UUID.
    pub(super) fn holder_name(&mut self, t: Target) -> Option<String> {
        match t {
            Target::Player => Some(self.acting_player_name().to_owned()),
            Target::ServerPlayer(conn) => self.command_player(conn).map(|g| g.name.clone()),
            Target::Mob(id)
            | Target::Item(id)
            | Target::Stand(id)
            | Target::Frame(id)
            | Target::Painting(id)
            | Target::Boat(id)
            | Target::Cart(id)
            | Target::Crystal(id)
            | Target::Dragon(id)
            | Target::Other(id) => Some(uuid_text(self.uuid_for(id))),
            // Orbs and projectiles have no UUID here.
            Target::Orb(_) | Target::Projectile(_) => None,
        }
    }
    fn holder_for(&mut self, t: Target) -> Option<Holder> {
        let name = self.holder_name(t)?;
        let shown = self.target_name(t);
        Some(Holder {
            display: Some(literal(&shown.plain())),
            text: shown,
            name,
        })
    }
    /// The entity with the given scoreboard name.
    fn target_of_holder(&self, name: &str) -> Option<Target> {
        if name == self.acting_player_name() {
            return Some(Target::Player);
        }
        let uuid = uuid_from_text(name).filter(|_| name.len() == 36)?;
        if Some(uuid) == self.level.player_uuid {
            return Some(Target::Player);
        }
        let id = self
            .persist
            .uuids
            .iter()
            .find(|(_, u)| **u == uuid)
            .map(|(id, _)| *id)?;
        if self.mobs.iter().any(|m| m.id == id) {
            Some(Target::Mob(id))
        } else if self.dropped.iter().any(|i| i.id == id) {
            Some(Target::Item(id))
        } else {
            None
        }
    }
    /// `Entity.getTags` (the `Tags` list of the saved entity).
    pub(super) fn tags_of(&self, t: Target) -> Vec<String> {
        let raw = match t {
            Target::Mob(id)
            | Target::Item(id)
            | Target::Stand(id)
            | Target::Frame(id)
            | Target::Painting(id)
            | Target::Boat(id)
            | Target::Cart(id)
            | Target::Other(id) => self.persist.raw.get(&id),
            Target::Player => self.level.player_extra.as_ref(),
            Target::ServerPlayer(_)
            | Target::Orb(_)
            | Target::Projectile(_)
            | Target::Crystal(_)
            | Target::Dragon(_) => None,
        };
        match raw.and_then(|r| r.get("Tags")) {
            Some(Tag::List(l)) => l
                .iter()
                .filter_map(|t| match t {
                    Tag::String(s) => Some(s.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    /// The selector options that need the scoreboard or entity data: which
    /// of `candidates` pass `scores=`, `team=` and `tag=`.
    pub(super) fn selector_extra_filter(
        &mut self,
        candidates: &[Target],
        s: &Selector,
    ) -> HashSet<Target> {
        let mut pass = HashSet::new();
        let needs_holder = !s.scores.is_empty() || !s.teams.is_empty();
        for &t in candidates {
            let mut ok = true;
            if needs_holder {
                let holder = self.holder_name(t);
                for (objective, range) in &s.scores {
                    let value = holder
                        .as_deref()
                        .and_then(|h| self.score.board.score(h, objective))
                        .map(|score| score.value);
                    if !value.is_some_and(|v| in_range(v, *range)) {
                        ok = false;
                    }
                }
                let team = holder
                    .as_deref()
                    .and_then(|h| self.score.board.players_team(h))
                    .map(|t| t.name.clone());
                for (want, negated) in &s.teams {
                    let matches = match &team {
                        Some(name) => name == want,
                        None => want.is_empty(),
                    };
                    if matches == *negated {
                        ok = false;
                    }
                }
            }
            if !s.tags.is_empty() {
                let tags = self.tags_of(t);
                for (tag, negated) in &s.tags {
                    let has = if tag.is_empty() {
                        !tags.is_empty()
                    } else {
                        tags.contains(tag)
                    };
                    if has == *negated {
                        ok = false;
                    }
                }
            }
            if ok && !s.nbt.is_empty() {
                // NbtPredicate: the entity's saved data contains the pattern.
                let data = self.entity_nbt(t);
                for (pattern, negated) in &s.nbt {
                    let matches = match (&data, horizon_world::nbt::parse_snbt(pattern)) {
                        (Some(d), Ok((p, _))) => crate::hc::nbt_path::compare_nbt(&p, Some(d)),
                        _ => false,
                    };
                    if matches == *negated {
                        ok = false;
                    }
                }
            }
            if ok && !s.predicates.is_empty() {
                // EntityPredicates: the datapack predicate with the entity as
                // `this` and the entity's position as origin.
                self.ensure_datapacks();
                let view = self.entity_view_of(t);
                let origin = self.target_position(t).unwrap_or_default();
                for (id, negated) in &s.predicates {
                    let matches = self
                        .fns
                        .store
                        .predicates
                        .get(id)
                        .is_some_and(|json| self.conditions_hold(json, Some(&view), origin));
                    if matches == *negated {
                        ok = false;
                    }
                }
            }
            if ok && !s.advancements.is_empty() {
                ok = t == Target::Player && self.advancements_match(&s.advancements);
            }
            if ok {
                pass.insert(t);
            }
        }
        pass
    }
    /// `advancements=` of a selector, against the player's progress.
    fn advancements_match(
        &self,
        wanted: &[(String, crate::hc::command::args::AdvCondition)],
    ) -> bool {
        use crate::hc::command::args::AdvCondition;
        wanted.iter().all(|(id, cond)| {
            let Some(a) = self.adv.tree.get(id) else {
                return false;
            };
            match cond {
                AdvCondition::Done(want) => self.adv.progress.done(a) == *want,
                AdvCondition::Criteria(list) => list
                    .iter()
                    .all(|(c, want)| self.adv.progress.has(id, c) == *want),
            }
        })
    }
    /// `ServerStatsCounter.setValue`'s scoreboard side: the objectives of
    /// each changed statistic's criteria follow it for the player `name`.
    pub(super) fn stat_criteria_changed(
        &mut self,
        name: &str,
        events: Vec<(String, String, Option<i32>)>,
    ) {
        let board = &mut self.score.board;
        if board.objectives.is_empty() {
            return;
        }
        for (ty, key, amount) in events {
            let criteria = sb::stat_criteria(&ty, &key);
            if board.has_objectives_for(&criteria) {
                board.for_all_objectives(&criteria, name, |v| {
                    amount.map_or(0, |n| v.wrapping_add(n))
                });
            }
        }
    }
    /// `ServerScoreboard`'s packets: each placed player's client is sent
    /// what changed on the board since it was last sent it (all of it on
    /// joining), server_net/scoreboard.rs.
    pub(super) fn net_sync_scoreboard(&mut self, net: &mut super::server_net::Network) {
        if net.hub.players.iter().all(|g| !g.placed) {
            return;
        }
        self.load_scoreboard();
        net.hub.teams.clone_from(&self.score.board.teams);
        let view = crate::hc::server_net::scoreboard::View::of(&self.score.board);
        for g in net.hub.players.iter_mut().filter(|g| g.placed) {
            if g.scoreboard != view {
                g.send_all(&g.scoreboard.changes(&view));
                g.scoreboard = view.clone();
            }
        }
    }
    /// `ServerPlayer.doTick` score updates and the statistic criteria.
    pub(super) fn tick_scoreboard(&mut self) {
        self.load_scoreboard();
        let events = std::mem::take(&mut self.stats.events);
        self.stat_criteria_changed(PLAYER_NAME, events);
        let board = &mut self.score.board;
        if !board.objectives.is_empty() {
            let display = self.player_display_comp();
            if let Some(p) = self.player.as_mut() {
                let health_and_absorption = p.health.health() + p.health.absorption();
                let food = p.health.food.food;
                // getArmorValue: the armor attribute rounded down.
                let armor = p
                    .attribute("minecraft:armor")
                    .or_else(|_| p.attribute("armor"))
                    .unwrap_or(0.0)
                    .floor() as i32;
                let rec = &mut self.score.recorded;
                let board = &mut self.score.board;
                if rec.health_and_absorption != Some(health_and_absorption) {
                    rec.health_and_absorption = Some(health_and_absorption);
                    let value = health_and_absorption.ceil() as i32;
                    board.for_all_objectives_with(
                        sb::CRITERIA_HEALTH,
                        PLAYER_NAME,
                        Some(&display),
                        |_| value,
                    );
                }
                for (criteria, last, value) in [
                    (sb::CRITERIA_FOOD, &mut rec.food, food),
                    // The air supply is full unless the player is drowning.
                    (sb::CRITERIA_AIR, &mut rec.air, 300),
                    (sb::CRITERIA_ARMOR, &mut rec.armor, armor),
                    (sb::CRITERIA_XP, &mut rec.xp, self.experience.total),
                    (sb::CRITERIA_LEVEL, &mut rec.level, self.experience.level),
                ] {
                    if *last != Some(value) {
                        *last = Some(value);
                        board.for_all_objectives_with(
                            criteria,
                            PLAYER_NAME,
                            Some(&display),
                            |_| value,
                        );
                    }
                }
            }
        }
        if self.ticks.is_multiple_of(20) {
            self.sync_score_registries();
        }
    }
    /// `ServerPlayer.awardKillScore`: total kills and the team kill counts.
    pub(super) fn score_player_killed(&mut self, victim: Target) {
        let victim_name = self.holder_name(victim);
        let display = self.player_display_comp();
        let me = self.acting_player_name().to_owned();
        let board = &mut self.score.board;
        let inc = |v: i32| v.wrapping_add(1);
        board.for_all_objectives_with(sb::CRITERIA_TOTAL_KILLS, &me, Some(&display), inc);
        if victim == Target::Player {
            board.for_all_objectives_with(sb::CRITERIA_PLAYER_KILLS, &me, Some(&display), inc);
        }
        let color_of =
            |board: &Scoreboard, name: &str| board.players_team(name).and_then(|t| t.color.clone());
        if let Some(victim_name) = victim_name {
            if let Some(color) = color_of(board, &victim_name) {
                let criteria = format!("teamkill.{color}");
                board.for_all_objectives_with(&criteria, &me, Some(&display), inc);
            }
            if let Some(color) = color_of(board, &me) {
                let criteria = format!("killedByTeam.{color}");
                board.for_all_objectives(&criteria, &victim_name, inc);
            }
        }
    }
    /// `Scoreboard.entityRemoved`: a dead non-player entity leaves the board.
    pub(super) fn score_entity_removed(&mut self, id: u32) {
        let Some(uuid) = self.persist.uuids.get(&id).copied() else {
            return;
        };
        self.score.board.entity_removed(&uuid_text(uuid));
    }
    /// `Entity.getTeam` of a mob (a mob joins a team by its UUID).
    pub(super) fn mob_team(&self, id: u32) -> Option<&sb::Team> {
        let uuid = self.persist.uuids.get(&id)?;
        self.score.board.players_team(&uuid_text(*uuid))
    }
    pub(super) fn player_team(&self) -> Option<&sb::Team> {
        self.score.board.players_team(self.acting_player_name())
    }
    /// The teams of the player and of the mobs on one (empty without teams).
    pub(super) fn mob_teams(&self) -> crate::hc::mobs::Teams<'_> {
        let board = &self.score.board;
        if board.teams.is_empty() {
            return Default::default();
        }
        crate::hc::mobs::Teams {
            player: self.player_team(),
            mobs: self
                .persist
                .uuids
                .iter()
                .filter_map(|(id, uuid)| Some((*id, board.players_team(&uuid_text(*uuid))?)))
                .collect(),
        }
    }
    /// The mobs on the player's team (`isAlliedTo`).
    pub(super) fn player_allies(&self) -> HashSet<u32> {
        let Some(team) = self.player_team() else {
            return HashSet::new();
        };
        self.persist
            .uuids
            .iter()
            .filter(|(_, uuid)| team.players.contains(&uuid_text(**uuid)))
            .map(|(id, _)| *id)
            .collect()
    }
    /// `Player.getDisplayName`: the name with the team's prefix, suffix and colour.
    pub(super) fn player_display_comp(&self) -> Comp {
        let name = literal(self.acting_player_name());
        match self.player_team() {
            Some(t) => t.formatted_name_comp(&name),
            None => name,
        }
    }
    pub(super) fn player_display_text(&self) -> Text {
        let name = Text::literal(self.acting_player_name());
        match self.player_team() {
            Some(t) => t.formatted_name(name),
            None => name,
        }
    }
    /// The scoreboard names a score holder argument resolves to.
    pub(super) fn score_holder_names(
        &mut self,
        ctx: &Context,
        name: &str,
        default_wildcard: bool,
    ) -> super::commands::R<Vec<String>> {
        let mut env = GameEnv { g: self };
        Ok(
            scoreboard_cmd::holders(ctx, name, &mut env, default_wildcard)?
                .into_iter()
                .map(|h| h.name)
                .collect(),
        )
    }
    pub(super) fn cmd_scoreboard(&mut self, id: &str, ctx: &Context) -> super::commands::R<i32> {
        let mut env = GameEnv { g: self };
        let result = scoreboard_cmd::execute(id, ctx, &mut env)
            .unwrap_or_else(|| Err(crate::hc::command::CommandError::tr("command.failed", &[])));
        self.sync_score_registries();
        result
    }
    /// ComponentUtils.updateForEntity for the pieces scoreboards store:
    /// `selector` and `score` components become their text.
    pub(super) fn resolve_component(&mut self, c: Comp) -> Comp {
        match c {
            Comp::Object(mut o) => {
                if let Some(sel) = o.get("selector").and_then(Comp::as_str).map(str::to_owned) {
                    let text = self.resolve_selector_text(&sel);
                    o.remove("selector");
                    o.insert("text".into(), Comp::String(text));
                } else if let Some(score) = o.get("score").cloned() {
                    let name = score.get("name").and_then(Comp::as_str).unwrap_or("");
                    let objective = score.get("objective").and_then(Comp::as_str).unwrap_or("");
                    let name = if name == "*" { PLAYER_NAME } else { name };
                    let text = self
                        .score
                        .board
                        .score(name, objective)
                        .map_or_else(String::new, |s| s.value.to_string());
                    o.remove("score");
                    o.insert("text".into(), Comp::String(text));
                }
                for key in ["extra", "with"] {
                    if let Some(Comp::Array(list)) = o.remove(key) {
                        let list = list
                            .into_iter()
                            .map(|c| self.resolve_component(c))
                            .collect();
                        o.insert(key.into(), Comp::Array(list));
                    }
                }
                Comp::Object(o)
            }
            Comp::Array(list) => Comp::Array(
                list.into_iter()
                    .map(|c| self.resolve_component(c))
                    .collect(),
            ),
            other => other,
        }
    }
    fn resolve_selector_text(&mut self, raw: &str) -> String {
        let mut reader = crate::hc::command::Reader::new(raw);
        let ty = crate::hc::command::args::ArgType::Entity {
            single: false,
            players: false,
        };
        let Ok(crate::hc::command::Arg::Entity(sel)) =
            ty.parse(&mut reader, &self.world_data.registries)
        else {
            return raw.to_owned();
        };
        let names: Vec<String> = self
            .select(&sel)
            .into_iter()
            .map(|t| self.target_name(t).plain())
            .collect();
        names.join(", ")
    }
}

/// The commands' view of the game.
struct GameEnv<'a, 'b> {
    g: &'a mut crate::game::Sim<'b>,
}
impl Env for GameEnv<'_, '_> {
    fn sb(&self) -> &Scoreboard {
        &self.g.score.board
    }
    fn sb_mut(&mut self) -> &mut Scoreboard {
        &mut self.g.score.board
    }
    fn select(&mut self, selector: &Selector) -> Vec<Holder> {
        let found = self.g.select(selector);
        found
            .into_iter()
            .filter_map(|t| self.g.holder_for(t))
            .collect()
    }
    fn named(&mut self, name: &str) -> Holder {
        if name.eq_ignore_ascii_case(self.g.acting_player_name()) && self.g.player.is_some() {
            return self
                .g
                .holder_for(Target::Player)
                .unwrap_or_else(|| Holder::named(name));
        }
        match self.g.target_of_holder(name) {
            Some(t) => self.g.holder_for(t).unwrap_or_else(|| Holder::named(name)),
            None => Holder::named(name),
        }
    }
    fn source(&mut self) -> Option<(Holder, bool)> {
        let entity = match self.g.command_source {
            Some(s) => s.entity,
            None => Some(Target::Player),
        }?;
        let name = match entity {
            Target::Player => self.g.acting_player_name().to_owned(),
            Target::Mob(id) | Target::Item(id) => uuid_text(self.g.uuid_for(id)),
            _ => return None,
        };
        let text = self.g.target_name(entity);
        Some((
            Holder {
                display: Some(literal(&text.plain())),
                text,
                name,
            },
            entity == Target::Player,
        ))
    }
    fn resolve(&mut self, c: Comp) -> Comp {
        self.g.resolve_component(c)
    }
    fn feedback(&mut self, message: Text) {
        self.g.feedback(message);
    }
}
