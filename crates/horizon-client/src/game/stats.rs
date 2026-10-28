//! Player statistics (`ServerStatsCounter`): the custom counters, blocks
//! mined, items used/crafted/broken/picked up/dropped and mobs killed or
//! killed by, saved to `players/stats/<uuid>.json`, plus the phantom
//! spawner that reads the time since rest (`PhantomSpawner`).
use super::Game;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// `stats[type][key]`, with `type` and `key` namespaced ids.
#[derive(Default)]
pub(super) struct Stats {
    pub(super) values: BTreeMap<String, BTreeMap<String, i32>>,
    pub(super) loaded: bool,
    /// Statistic changes (type, key, amount; `None` = reset) for the
    /// scoreboard criteria to follow.
    pub(super) events: Vec<(String, String, Option<i32>)>,
    /// Changed since the client was last sent them (`StatsCounter`'s dirty
    /// set), by type.
    dirty: BTreeMap<String, BTreeSet<String>>,
}

/// `minecraft:` and `name`, given to `f` from the stack when short: some
/// counters go up every tick for every player, and they are found without
/// a string made for each.
fn namespaced<T>(name: &str, f: impl FnOnce(&str) -> T) -> T {
    const NAMESPACE: &str = "minecraft:";
    let mut buf = [0u8; 96];
    let len = NAMESPACE.len() + name.len();
    if len > buf.len() {
        return f(&format!("{NAMESPACE}{name}"));
    }
    buf[..NAMESPACE.len()].copy_from_slice(NAMESPACE.as_bytes());
    buf[NAMESPACE.len()..len].copy_from_slice(name.as_bytes());
    // Two strings one after the other are one.
    f(std::str::from_utf8(&buf[..len]).unwrap_or_default())
}

/// The map under `key`, made when missing, found without an owned key
/// when there.
fn entry<'a, V: Default>(map: &'a mut BTreeMap<String, V>, key: &str) -> &'a mut V {
    if !map.contains_key(key) {
        map.insert(key.to_owned(), V::default());
    }
    map.get_mut(key).expect("just inserted")
}

impl Stats {
    pub(super) fn add(&mut self, ty: &str, key: &str, n: i32) {
        namespaced(ty, |ty| {
            let v = entry(entry(&mut self.values, ty), key);
            *v = v.saturating_add(n);
            self.events.push((ty.to_owned(), key.to_owned(), Some(n)));
            self.mark_dirty(ty, key);
        });
    }
    fn mark_dirty(&mut self, ty: &str, key: &str) {
        let keys = entry(&mut self.dirty, ty);
        if !keys.contains(key) {
            keys.insert(key.to_owned());
        }
    }
    pub(super) fn get(&self, ty: &str, key: &str) -> i32 {
        self.values
            .get(&format!("minecraft:{ty}"))
            .and_then(|m| m.get(key))
            .copied()
            .unwrap_or(0)
    }
    pub(super) fn reset(&mut self, ty: &str, key: &str) {
        if let Some(m) = self.values.get_mut(&format!("minecraft:{ty}")) {
            m.insert(key.to_owned(), 0);
        }
        self.mark_dirty(&format!("minecraft:{ty}"), key);
        self.events
            .push((format!("minecraft:{ty}"), key.to_owned(), None));
    }
    pub(super) fn custom(&mut self, key: &str, n: i32) {
        namespaced(key, |key| self.add("custom", key, n));
    }
    /// `ServerStatsCounter.parseLocal`: the counters of a stats file.
    pub(super) fn read(path: &Path) -> Self {
        let mut stats = Self {
            loaded: true,
            ..Self::default()
        };
        let Some(json) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            return stats;
        };
        if let Some(all) = json.get("stats").and_then(|s| s.as_object()) {
            for (ty, entries) in all {
                let Some(entries) = entries.as_object() else {
                    continue;
                };
                let map = stats.values.entry(ty.clone()).or_default();
                for (key, v) in entries {
                    if let Some(n) = v.as_i64() {
                        map.insert(key.clone(), n as i32);
                    }
                }
            }
        }
        stats
    }
    /// `ServerStatsCounter.save`.
    pub(super) fn write(&self, path: &Path) {
        let stats: serde_json::Map<String, serde_json::Value> = self
            .values
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(ty, m)| {
                let entries: serde_json::Map<String, serde_json::Value> =
                    m.iter().map(|(k, v)| (k.clone(), (*v).into())).collect();
                (ty.clone(), serde_json::Value::Object(entries))
            })
            .collect();
        let json = serde_json::json!({
            "stats": stats,
            "DataVersion": horizon_world::DATA_VERSION,
        });
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&json) {
            let _ = std::fs::write(path, text);
        }
    }
}
/// `players/<dir>/<uuid>.json` of a world (advancements, stats).
pub(super) fn player_json(world: &Path, dir: &str, uuid: [i32; 4]) -> PathBuf {
    let hex: String = uuid.iter().map(|v| format!("{:08x}", *v as u32)).collect();
    let text = format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    world.join("players").join(dir).join(format!("{text}.json"))
}
impl Game<'_> {}
impl crate::game::Sim<'_> {
    fn stats_path(&self) -> Option<PathBuf> {
        Some(player_json(
            &self.world_root,
            "stats",
            self.level.player_uuid?,
        ))
    }
    /// `ServerStatsCounter.parseLocal`.
    pub(super) fn load_stats(&mut self) {
        if self.stats.loaded || self.remote.is_some() || self.no_player_here() {
            return;
        }
        self.stats.loaded = true;
        if let Some(path) = self.stats_path() {
            self.stats.values = Stats::read(&path).values;
        }
    }
    /// `ServerStatsCounter.save`.
    pub(super) fn save_stats(&self) {
        if !self.stats.loaded || self.remote.is_some() || self.no_player_here() {
            return;
        }
        if let Some(path) = self.stats_path() {
            self.stats.write(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
