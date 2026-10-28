//! Pillager patrols (`PatrolSpawner`), the raid omen, and raids (`Raids`):
//! their waves, boss bar, victory and the raiders' bookkeeping. The rules are
//! `horizon_client::patrol` and `horizon_client::raid`; this is the game's
//! side of them.
use super::{Dim, Game};
use horizon_client::{
    
    mobs::{
        Mob, Species, raider},
    patrol::GameplayAttributes,
    raid::{self, BarName, Raids}};
use horizon_core::{geometry::Vec3, random::RandomSource};
use horizon_world::nbt::Tag;
use std::collections::HashMap;

/// Patrol and raid state owned by the game.
#[derive(Default)]
pub(super) struct Raiding {
    /// The raids of the dimension the player is in, and that dimension.
    raids: Raids,
    loaded: Option<Dim>,
    /// `ServerPlayer.raidOmenPosition` (`raid_omen_position`), read from the
    /// player data on first use.
    omen_position: Option<[i32; 3]>,
    omen_loaded: bool,
    /// The last raider hurt each raid's bar has seen (`Raider.hurtServer`).
    bar_hurts: HashMap<i32, u64>,
    attributes: Option<std::sync::Arc<GameplayAttributes>>,
    /// Beds near each raid's centre, rescanned now and then (`HOME` points
    /// of interest for `RaiderMoveThroughVillageGoal`).
    homes: HashMap<i32, (i64, std::sync::Arc<Vec<[i32; 3]>>)>,
}


impl Raiding {
}




fn block_of(p: Vec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Every claimed village point of interest (job sites, beds, bells).
    fn village_pois(&self) -> Vec<[i32; 3]> {
        let mut out: Vec<[i32; 3]> = Vec::new();
        for m in &self.mobs {
            if m.species() != Species::Villager || !m.alive() {
                continue;
            }
            for p in [m.extra.home, m.extra.bed, m.extra.meeting]
                .into_iter()
                .flatten()
            {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }
    pub(super) fn difficulty_id(&self) -> i32 {
        use horizon_world::food::Difficulty;
        match self.world.health_rules.difficulty {
            Difficulty::Peaceful => 0,
            Difficulty::Easy => 1,
            Difficulty::Normal => 2,
            Difficulty::Hard => 3,
        }
    }
    /// `dimensions/<dimension>/data/minecraft/raids.dat`: each dimension
    /// keeps its own raids.
    fn raids_path(&self, dim: Dim) -> std::path::PathBuf {
        horizon_client::save::dimension_dir(&self.world_root, dim.id())
            .join("data/minecraft/raids.dat")
    }
    /// The raids of the player's dimension (the previous one saved first).
    fn load_raids(&mut self) {
        if self.raiding.loaded == Some(self.dim) {
            return;
        }
        if self.raiding.loaded.is_some() {
            self.save_raids();
        }
        self.raiding.loaded = Some(self.dim);
        self.raiding.bar_hurts.clear();
        self.raiding.homes.clear();
        let seed = self.world.random.next_long();
        self.raiding.raids = match horizon_client::save::read_dat(&self.raids_path(self.dim))
            .and_then(|tag| tag.get("data").cloned())
        {
            Some(data) => Raids::from_nbt(&data, seed),
            None => Raids::default(),
        };
    }
    /// `Raids` as its dimension's `raids.dat`, when it changed.
    pub(super) fn save_raids(&mut self) {
        let Some(dim) = self.raiding.loaded else {
            return;
        };
        if !self.raiding.raids.dirty || self.remote.is_some() {
            return;
        }
        let tag = horizon_world::nbt::Tag::compound([
            ("data", self.raiding.raids.to_nbt()),
            (
                "DataVersion",
                horizon_world::nbt::Tag::Int(horizon_world::DATA_VERSION),
            ),
        ]);
        if horizon_client::save::write_dat(&self.raids_path(dim), tag).is_ok() {
            self.raiding.raids.dirty = false;
        }
        self.save_raid_omen_position();
    }
    /// Writes `raid_omen_position` into the player data being saved.
    fn save_raid_omen_position(&mut self) {
        if !self.raiding.omen_loaded {
            return;
        }
        let extra = self
            .level
            .player_extra
            .get_or_insert_with(|| horizon_world::nbt::Tag::Compound(Default::default()));
        if let horizon_world::nbt::Tag::Compound(map) = extra {
            match self.raiding.omen_position {
                Some(p) => {
                    map.insert(
                        "raid_omen_position".into(),
                        horizon_world::nbt::Tag::IntArray(p.to_vec()),
                    );
                }
                None => {
                    map.remove(&"raid_omen_position".into());
                }
            }
        }
    }
    fn gameplay_attributes(&mut self) -> std::sync::Arc<GameplayAttributes> {
        if self.raiding.attributes.is_none() {
            self.raiding.attributes = Some(std::sync::Arc::new(GameplayAttributes::load(
                &self.options.resources,
            )));
        }
        self.raiding.attributes.clone().unwrap_or_default()
    }
    /// A boolean gameplay attribute at a position (dimension, biome, then
    /// the timelines at the overworld clock).
    fn gameplay_attribute(&mut self, key: &str, at: [i32; 3]) -> bool {
        let attributes = self.gameplay_attributes();
        let biome = self
            .world
            .chunks
            .get(&(at[0] >> 4, at[2] >> 4))
            .map(|c| {
                self.res
                    .biomes()
                    .get(c.biome(at[0], at[1], at[2]))
                    .name
                    .clone()
            })
            .unwrap_or_default();
        attributes.value(key, self.dim.id(), &biome, self.world.day_time)
    }
    /// `Raids.createOrExtendRaid`: not for spectators, with the `raids` rule
    /// and where raids may start; the raid centres on the occupied village
    /// points of interest within 64 blocks.
    fn create_or_extend_raid(&mut self, at: [i32; 3], amplifier: Option<i32>) -> Option<i32> {
        if self.spectator()
            || !self.world_data.rules.bool("raids")
            || !self.gameplay_attribute(horizon_client::patrol::CAN_START_RAID, at)
        {
            return None;
        }
        self.load_raids();
        let center = raid::raid_center(at, &self.village_pois());
        let seed = self.world.random.next_long();
        let difficulty = self.difficulty_id();
        let (id, triggered) = self
            .raiding
            .raids
            .create_or_extend(center, difficulty, amplifier, seed);
        if triggered {
            self.stats.custom("raid_trigger", 1);
            self.trigger_player("minecraft:voluntary_exile");
        }
        Some(id)
    }
    /// `ServerLevel.getRaidAt`.
    pub(super) fn raiding_nearby(&self, at: [i32; 3]) -> Option<i32> {
        self.raiding.raids.nearby(at).map(|(id, _)| id)
    }
    pub(super) fn raid_started(&self, id: i32) -> bool {
        self.raiding.raids.get(id).is_some_and(|r| r.started)
    }
    /// `/raid start`: a raid in the local village at the given omen level.
    pub(super) fn raid_command_create(&mut self, at: [i32; 3], omen_level: i32) -> bool {
        self.load_raids();
        let Some(id) = self.create_or_extend_raid(at, None) else {
            return false;
        };
        if let Some(r) = self.raiding.raids.get_mut(id) {
            r.omen_level = omen_level;
        }
        self.raiding.raids.dirty = true;
        true
    }
    pub(super) fn raid_command_stop(&mut self, id: i32) {
        if let Some(r) = self.raiding.raids.get_mut(id) {
            r.stop();
        }
    }
    pub(super) fn raid_set_omen(&mut self, id: i32, level: i32) -> i32 {
        let Some(r) = self.raiding.raids.get_mut(id) else {
            return 0;
        };
        std::mem::replace(&mut r.omen_level, level)
    }
    pub(super) fn raid_glow(&mut self, id: i32) {
        for m in &mut self.mobs {
            if raider::state(m).is_some_and(|s| s.raid_id == Some(id)) {
                m.add_effect("glowing", 1000, 1);
            }
        }
    }
    /// The `/raid check` summary line.
    pub(super) fn raid_status_text(&self, id: i32) -> String {
        let Some(r) = self.raiding.raids.get(id) else {
            return String::new();
        };
        let members: Vec<&Mob> = self.raid_members(id).collect();
        let health: f32 = members.iter().fold(0.0, |a, m| a + m.health);
        format!(
            "Num groups spawned: {} Raid omen level: {} Num mobs: {} Raid health: {} / {}",
            r.groups_spawned,
            r.omen_level,
            members.len(),
            health,
            r.total_health
        )
    }
    /// A raid's members (`groupRaiderMap`): the living raiders it counts.
    fn raid_members(&self, id: i32) -> impl Iterator<Item = &Mob> {
        self.mobs.iter().filter(move |m| {
            m.alive() && !m.removed && raider::state(m).is_some_and(|s| s.raid_id == Some(id))
        })
    }
    /// The raid bar a living player at `at` is in (`Raid.getPlayers`): red,
    /// notched in tens. The name is the translate component the client draws.
    pub(super) fn raid_boss_at(&self, at: [f64; 3], alive: bool) -> Option<(i32, Tag, f32)> {
        if !alive || self.dim != Dim::Overworld {
            return None;
        }
        let block = block_of(Vec3::new(at[0], at[1], at[2]));
        let (id, raid) = self.raiding.raids.nearby(block)?;
        if !raid.bar.visible || raid.is_stopped() {
            return None;
        }
        Some((id, raid_name_tag(raid.bar.name), raid.bar.progress))
    }
    /// A firework a villager launches to celebrate (`CelebrateVillagersSurvivedRaid`).
    pub(super) fn villager_firework(&mut self, at: [f64; 3], color: usize, flight: u8) {
        let mut stack = horizon_client::items::ItemStack::new("minecraft:firework_rocket", 1);
        stack.data_mut().fireworks = Some((
            flight,
            vec![horizon_client::particles::FireworkExplosion {
                shape: horizon_client::particles::FireworkShape::Burst,
                colors: vec![horizon_client::particles::DYE_FIREWORK_COLORS[color & 15].1],
                fade: Vec::new(),
                trail: false,
                twinkle: false,
            }],
        ));
        let rocket = self.new_rocket(Vec3::new(at[0], at[1], at[2]), &stack, false);
        self.rockets.push(rocket);
    }
    /// `BehaviorUtils.throwItem`: a hero's gift tossed toward the player.
    pub(super) fn throw_gift(&mut self, table: &str, from: [f64; 3], to: [f64; 3]) {
        let drops = self
            .res
            .items()
            .loot_table(table, false, None, &mut self.world.random);
        let d = Vec3::new(to[0] - from[0], to[1] - from[1], to[2] - from[2]).normalize();
        for stack in drops {
            self.spawn_item(stack, from, [d.x * 0.3, d.y * 0.3, d.z * 0.3], 10);
        }
    }
}

impl Game<'_> {
}
impl crate::game::Sim<'_> {
}

/// `Raid.raidEvent`'s name: the raid title, the remaining-raiders line, or
/// the victory and defeat lines.
fn raid_name_tag(name: BarName) -> Tag {
    let key = |k: &str| Tag::compound([("translate", Tag::String(k.into()))]);
    match name {
        BarName::Raid => key("event.minecraft.raid"),
        BarName::Victory => key("event.minecraft.raid.victory.full"),
        BarName::Defeat => key("event.minecraft.raid.defeat.full"),
        BarName::Remaining(n) => Tag::compound([
            ("translate", Tag::String("event.minecraft.raid".into())),
            (
                "extra",
                Tag::List(vec![
                    Tag::String(" - ".into()),
                    Tag::compound([
                        (
                            "translate",
                            Tag::String("event.minecraft.raid.raiders_remaining".into()),
                        ),
                        (
                            "with",
                            Tag::List(vec![Tag::String(n.to_string().as_str().into())]),
                        ),
                    ]),
                ]),
            ),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
