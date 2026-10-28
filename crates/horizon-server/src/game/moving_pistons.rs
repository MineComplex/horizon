//! `PistonHeadRenderer`: while a piston moves them, blocks are
//! `minecraft:moving_piston` block entities (drawn as nothing by the chunk
//! mesher); this draws each moved block at its interpolated offset.
use crate::hc::mesher::Vertex;
use horizon_world::{piston_shape::MovingPistonState, registry::BlockStateId};

impl crate::game::Sim<'_> {
    /// The moved block and, for a retracting piston, its extended base.
    fn moving_piston_blocks(
        &self,
        piston: MovingPistonState,
        partial: f32,
    ) -> Option<(BlockStateId, Option<BlockStateId>)> {
        let facts = self.res.facts();
        if facts.is_air(piston.moved_state) {
            return None;
        }
        let progress = piston.interpolated_progress(partial);
        let name = &*facts.get(piston.moved_state).name;
        // Vanilla tests `progress <= 4.0`: a moving head is always drawn so.
        if name == "minecraft:piston_head" {
            let short = if progress <= 0.5 { "true" } else { "false" };
            return Some((
                self.with_props(piston.moved_state, &[("short", short)])?,
                None,
            ));
        }
        if piston.source && !piston.extending {
            let registry = &self.pack.assets.registry;
            let facing = registry
                .state(piston.moved_state)?
                .properties
                .get("facing")?
                .clone();
            let kind = if name == "minecraft:sticky_piston" {
                "sticky"
            } else {
                "normal"
            };
            let short = if progress >= 0.5 { "true" } else { "false" };
            let head = self.with_props(
                registry.default_state("minecraft:piston_head")?,
                &[("type", kind), ("facing", &facing), ("short", short)],
            )?;
            let base = self.with_props(piston.moved_state, &[("extended", "true")])?;
            return Some((head, Some(base)));
        }
        Some((piston.moved_state, None))
    }
}
