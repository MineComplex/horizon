//! A blockstate-driven inspection gallery, separate from gameplay simulation.
pub mod catalog;
mod cube;
use crate::{Result, sprites::Sprites};
pub use catalog::StateModels;
pub use cube::CubeStyle;
use horizon_core::position::BlockPos;
pub use horizon_game::face_visibility::load_visibility;
use horizon_game::gallery::{GALLERY, descriptor};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use serde_json::{Value, json};
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelInstance {
    pub positions: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub layer: u32,
    pub shade: f32,
}
pub struct PreparedModels {
    pub catalog: StateModels,
    pub placements: Vec<(BlockPos, BlockStateId)>,
    pub states: Vec<Value>,
}
pub fn prepare(
    root: &Path,
    registry: &BlockRegistry,
    sprites: &mut Sprites,
) -> Result<PreparedModels> {
    prepare_with_models(
        root,
        registry,
        sprites,
        &crate::model_library::ParsedModels::default(),
    )
}
pub(crate) fn prepare_with_models(
    root: &Path,
    registry: &BlockRegistry,
    sprites: &mut Sprites,
    models: &crate::model_library::ParsedModels,
) -> Result<PreparedModels> {
    let facts: std::collections::BTreeMap<String, Value> =
        serde_json::from_slice(&std::fs::read(root.join("model-states.json"))?)?;
    let blocks: Vec<&str> = facts
        .keys()
        .filter(|name| registry.default_state(name).is_some())
        .map(|name| name.strip_prefix("minecraft:").unwrap_or(name))
        .collect();
    let catalog =
        StateModels::from_facts_with_models(root, registry, &blocks, sprites, &facts, models)?;
    let mut placements = Vec::new();
    let mut states = Vec::new();
    for (index, (name, properties)) in GALLERY.iter().enumerate() {
        let state = registry.resolve(&descriptor(name, properties))?;
        let actual = registry
            .state(state)
            .ok_or("missing gallery registry state")?;
        let position = horizon_game::gallery::position(index);
        let selected = catalog.collect(state, position)?;
        states.push(json!({"block":actual.name,"properties":actual.properties,"state_id":state.0,
            "position":[position.x,position.y,position.z],
            "models":selected.iter().map(|(v,_)| json!({"model":v.model,"rotations":v.rotations,"uv_lock":v.uv_lock})).collect::<Vec<_>>()}));
        placements.push((position, state));
    }
    Ok(PreparedModels {
        catalog,
        placements,
        states,
    })
}
pub fn instance(face: &catalog::BakedFace, position: BlockPos) -> ModelInstance {
    let origin = [position.x as f32, position.y as f32, position.z as f32];
    ModelInstance {
        positions: face
            .quad
            .positions
            .map(|v| std::array::from_fn(|i| v[i] + origin[i])),
        uv: face.quad.uv,
        layer: face.layer,
        shade: cube::shade(face),
    }
}
