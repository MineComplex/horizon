//! What a client keeps of the command code: item rarities, data pack tags,
//! entity data for F3 + I and the version lines. Commands run on the server.
mod data;
mod extra;
mod terrain_cmds;

use super::chat::PLAYER_NAME;

impl crate::game::Game<'_> {
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
}
