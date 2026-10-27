//! The client's side of chunk streaming (`horizon_game::streaming`): section meshes built from
//! block models by the workers (`Mesher`).
use crate::{
    assets::Assets,
    biome::Biomes,
    light::LightTables,
    mesher::{MeshTables, Mesher, SectionMesh},
    render_assets::RenderAssets,
};
pub use horizon_game::streaming::*;
use horizon_world::block_render::FaceVisibility;
use std::sync::Arc;

pub type Streaming = horizon_game::streaming::Streaming<WorkerContext>;

/// What the workers read: block facts, models and sprites, swapped when resource packs reload.
pub struct WorkerContext {
    pub assets: Arc<Assets>,
    pub render: Arc<RenderAssets>,
    pub tables: Arc<MeshTables>,
    pub biomes: Arc<Biomes>,
    pub light: Arc<LightTables>,
}
impl MeshContext for WorkerContext {
    type Mesh = SectionMesh;
    /// The face visibility table and its occlusion cache stay with the worker across mesh
    /// jobs, like vanilla's per-thread cache: cloning it per job copied every state and face
    /// shape.
    type Scratch = Option<(Arc<MeshTables>, FaceVisibility)>;
    fn light(&self) -> &Arc<LightTables> {
        &self.light
    }
    fn mesh(
        &self,
        region: &[[Option<&horizon_game::chunk::Chunk>; 3]; 3],
        sy: i32,
        settings: MeshSettings,
        scratch: &mut Self::Scratch,
    ) -> SectionMesh {
        let visibility = match scratch.take() {
            Some((tables, visibility)) if Arc::ptr_eq(&tables, &self.tables) => visibility,
            _ => self.tables.visibility.clone(),
        };
        let mut mesher = Mesher {
            nether_shading: settings.nether,
            zoom: settings.zoom,
            blend_radius: settings.blend_radius,
            smooth_lighting: settings.smooth_lighting,
            cutout_leaves: settings.cutout_leaves,
            assets: &self.assets,
            render: &self.render,
            tables: &self.tables,
            biomes: &self.biomes,
            visibility,
        };
        let mut mesh = mesher.mesh(region, sy);
        mesh.sort_solid();
        *scratch = Some((self.tables.clone(), mesher.visibility));
        mesh
    }
}
