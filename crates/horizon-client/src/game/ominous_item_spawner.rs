//! `OminousItemSpawner`: the item an ominous trial spawner drops on its
//! fight. It floats, growing and turning (`OminousItemSpawnerRenderer`), for
//! 60 to 120 ticks, then falls as an item or is fired straight down as a
//! projectile (`spawnItem`). Saved with its chunk's entities (persist.rs).
use super::Game;
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::vertex_light, item_render, items::ItemStack, mesher::Vertex,
};

/// `OminousItemSpawnerRenderer`: grows over its first 50 ticks, turning 40
/// degrees a tick.
const TICKS_SCALING: f32 = 50.0;
const ROTATION_SPEED: f32 = 40.0;

pub(super) struct OminousItemSpawner {
    pub(super) item: ItemStack,
    pub(super) position: [f64; 3],
    tick_count: i64,
}

impl Game<'_> {
    /// `OminousItemSpawnerRenderer`: the item, full bright, growing over its
    /// first 50 ticks and turning.
    pub(super) fn ominous_item_spawner_geometry(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<Vertex>,
    ) {
        for s in &self.ominous_spawners {
            let at = glam::DVec3::from_array(s.position);
            if at.distance_squared(camera) >= 64.0 * 64.0 {
                continue;
            }
            let state = self.resolve_item(&s.item, DisplayContext::Ground, None, 0);
            if state.is_empty() {
                continue;
            }
            let age = s.tick_count as f32 + alpha as f32;
            let scale = if age <= TICKS_SCALING {
                age.min(TICKS_SCALING) / TICKS_SCALING
            } else {
                1.0
            };
            let spin = (age * ROTATION_SPEED).rem_euclid(360.0);
            let pose = glam::Mat4::from_translation((at - camera).as_vec3())
                * glam::Mat4::from_scale(glam::Vec3::splat(scale))
                * glam::Mat4::from_rotation_y(spin.to_radians());
            let seed =
                i64::from(self.pack.item_renderer.item_id(&s.item.item)) + i64::from(s.item.damage);
            for copy in item_render::cluster_poses(
                &state,
                pose,
                item_render::rendered_amount(s.item.count),
                seed,
            ) {
                self.submit_item_world(&state, copy, vertex_light(0xFF), out);
            }
        }
    }
}
