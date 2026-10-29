//! What a client keeps of the command code: item rarities, data pack tags,
//! entity data for F3 + I and the version lines. Commands run on the server.
mod data;
mod extra;
mod terrain_cmds;

use super::chat::PLAYER_NAME;

impl crate::game::Sim<'_> {
    /// The player's name (`Player.getName`).
    pub(super) fn acting_player_name(&self) -> &str {
        PLAYER_NAME
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
    /// Whether `block` is in the block tag `tag`.
    pub(super) fn block_in_tag(&self, tag: &str, block: &str) -> bool {
        self.res
            .items()
            .block_tags
            .get(tag)
            .is_some_and(|s| s.contains(block))
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
