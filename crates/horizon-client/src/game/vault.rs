//! Vaults in the game: `VaultBlockEntity.Server.tick` for every vault
//! (`vault`), the block's state as it changes, the rewards it ejects, the
//! keys players insert (`VaultBlock.useItemOn`) and the display item spinning
//! inside (`VaultRenderer`).
use super::Game;
use horizon_assets::item_model::DisplayContext;
use horizon_client::{
    block_entity_render::vertex_light,
    containers::BlockEntity,
    item_render,
    mesher::Vertex};



impl Game<'_> {
    /// `VaultRenderer`: the display item of every vault near the camera that
    /// shows one, turning above its floor.
    pub(super) fn vault_geometry(&self, camera: glam::DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        for (key, entity) in &self.world.block_entities {
            let BlockEntity::Vault(vault) = entity else {
                continue;
            };
            let Some(stack) = &vault.display_item else {
                continue;
            };
            let at = glam::DVec3::new(f64::from(key.0), f64::from(key.1), f64::from(key.2));
            if at.distance_squared(camera) >= 64.0 * 64.0 {
                continue;
            }
            let state = self.resolve_item(stack, DisplayContext::Ground, None, 0);
            if state.is_empty() {
                continue;
            }
            let pose =
                glam::Mat4::from_translation(
                    (at - camera).as_vec3() + glam::Vec3::new(0.5, 0.4, 0.5),
                ) * glam::Mat4::from_rotation_y(vault.display_spin(alpha as f32).to_radians());
            let light = self.world.light(key.0, key.1, key.2);
            let seed =
                i64::from(self.pack.item_renderer.item_id(&stack.item)) + i64::from(stack.damage);
            for copy in item_render::cluster_poses(
                &state,
                pose,
                item_render::rendered_amount(stack.count),
                seed,
            ) {
                self.submit_item_world(&state, copy, vertex_light(light), out);
            }
        }
    }
}
