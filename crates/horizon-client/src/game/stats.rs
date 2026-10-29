//! Player statistics as the server last sent them (`ClientboundAwardStatsPacket`,
//! the client's `StatsCounter`): the statistics screen reads them. Counting
//! them is the server's (`ServerStatsCounter`).
use std::collections::BTreeMap;

/// `stats[type][key]`, with `type` and `key` namespaced ids.
#[derive(Default)]
pub(super) struct Stats {
    pub(super) values: BTreeMap<String, BTreeMap<String, i32>>,
}

impl Stats {
    pub(super) fn get(&self, ty: &str, key: &str) -> i32 {
        self.values
            .get(&format!("minecraft:{ty}"))
            .and_then(|m| m.get(key))
            .copied()
            .unwrap_or(0)
    }
}
