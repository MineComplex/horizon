//! Beehives and bee nests in the game (`crate::beehive` holds their logic):
//! their server tick, bees going in, and the bees let out, angry, when a
//! player breaks the hive or fire reaches it.
use horizon_client::beehive::{self};

impl crate::game::Sim<'_> {
    /// The hive's honey level and front, from its block state.
    pub(super) fn hive_state(&self, at: [i32; 3]) -> Option<(Option<i32>, [i32; 2])> {
        let state = self.world.block(at[0], at[1], at[2]);
        let name = &self.res.facts().get(state).name;
        if !self
            .res
            .items()
            .block_tags
            .get("minecraft:beehives")
            .is_some_and(|t| t.contains(&**name))
        {
            return None;
        }
        let props = &self.pack.assets.registry.state(state)?.properties;
        let honey = props.get("honey_level").and_then(|v| v.parse().ok());
        let facing = beehive::facing_step(props.get("facing").map(String::as_str));
        Some((honey, facing))
    }
}
