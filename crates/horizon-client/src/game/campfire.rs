//! Campfires' cooking: `CampfireBlock.useItemOn` (`CampfireBlockEntity.placeFood`)
//! and the block entity's `cookTick` (lit) and `cooldownTick` (unlit). The
//! items drawn on the fire are `campfire_geometry`'s; a server's clients get
//! them through the block entity's update tag (`lan::chunks::entity_tag`).
use super::Game;
use horizon_assets::item_model::DisplayContext;
use horizon_client::{block_entity_render::vertex_light, containers::BlockEntity, mesher::Vertex};
use horizon_core::position::BlockPos;

impl Game<'_> {
    /// `CampfireRenderer`: each cooking item lies on the fire, slot `i`
    /// turned to `Direction.from2DDataValue((i + facing) % 4)`, laid flat
    /// (90° about X), at a corner and three eighths of its size, drawn as a
    /// FIXED item seeded by the block's position plus the slot.
    pub(super) fn campfire_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        for (key, entity) in &self.world.block_entities {
            let BlockEntity::Campfire(c) = entity else {
                continue;
            };
            if c.items.iter().all(Option::is_none) {
                continue;
            }
            let at = glam::DVec3::new(f64::from(key.0), f64::from(key.1), f64::from(key.2));
            if at.distance_squared(camera) >= 64.0 * 64.0 {
                continue;
            }
            // Direction.get2DDataValue: south 0, west 1, north 2, east 3.
            let state = self.world.block(key.0, key.1, key.2);
            let facing = match self
                .pack
                .assets
                .registry
                .state(state)
                .and_then(|s| s.properties.get("facing"))
                .map(String::as_str)
            {
                Some("west") => 1,
                Some("north") => 2,
                Some("east") => 3,
                _ => 0,
            };
            let light = vertex_light(self.world.light(key.0, key.1, key.2));
            let packed = BlockPos::new(key.0, key.1, key.2).pack();
            for (i, stack) in c.items.iter().enumerate() {
                let Some(stack) = stack else {
                    continue;
                };
                let seed = (packed as i32).wrapping_add(i as i32);
                let item = self.resolve_item(stack, DisplayContext::Fixed, None, seed);
                if item.is_empty() {
                    continue;
                }
                // Direction.toYRot: 90° per 2D step from south.
                let yaw = -(((i + facing) % 4) as f32 * 90.0);
                let pose = glam::Mat4::from_translation(
                    (at - camera).as_vec3() + glam::Vec3::new(0.5, 0.449_218_75, 0.5),
                ) * glam::Mat4::from_rotation_y(yaw.to_radians())
                    * glam::Mat4::from_rotation_x(90f32.to_radians())
                    * glam::Mat4::from_translation(glam::Vec3::new(-0.3125, -0.3125, 0.0))
                    * glam::Mat4::from_scale(glam::Vec3::splat(0.375));
                self.submit_item_world(&item, pose, light, out);
            }
        }
    }
}
