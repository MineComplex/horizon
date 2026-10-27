//! Block models and their baked sprites over the game's block registry: what the mesher and
//! renderer need, and the server never loads.
use crate::{
    Result,
    assets::{Assets, MATERIALS},
};
use horizon_world::registry::BlockStateId;
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// The block models and their baked sprites, read from the pack at `root` over
/// an existing block registry: everything the mesher and renderer need.
pub struct RenderAssets {
    pub sprite_size: u32,
    pub sprite_minimum_size: u32,
    pub pixels: Vec<u8>,
    pub texture_layers: u32,
    pub model_catalog: crate::models::StateModels,
    /// Shared parsed resources; item-only validation errors surface at item preparation.
    pub model_library: std::result::Result<Arc<horizon_assets::resolve::ModelLibrary>, String>,
    pub model_placements: Vec<(horizon_core::position::BlockPos, BlockStateId)>,
    pub model_gallery: Vec<Value>,
    pub animations: Vec<crate::sprites::SpriteAnimation>,
    pub sprite_layers: std::collections::BTreeMap<String, u32>,
    pub translucent_layers: Vec<bool>,
    /// Each layer's `MipmapGenerator` settings.
    pub sprite_mips: Vec<crate::mipmap::Settings>,
}
impl RenderAssets {
    pub fn load(root: &Path, assets: &Assets) -> Result<Self> {
        let registry = assets.registry;
        let mut sprites = crate::sprites::Sprites::default();
        for name in MATERIALS {
            sprites.insert(root, &format!("minecraft:block/{name}"))?;
        }
        let parsed = crate::model_library::ParsedModels::load(root);
        let models = crate::models::prepare_with_models(root, registry, &mut sprites, &parsed)?;
        let model_library = parsed.into_item_library();
        // Fluids are drawn by the fluid renderer, not block models; breaking
        // progress uses the destroy-stage overlays.
        for name in [
            "water_still",
            "water_flow",
            "water_overlay",
            "lava_still",
            "lava_flow",
        ] {
            sprites.insert(root, &format!("minecraft:block/{name}"))?;
        }
        for stage in 0..10 {
            sprites.insert(root, &format!("minecraft:block/destroy_stage_{stage}"))?;
        }
        sprites.finish();
        Ok(Self {
            sprite_size: sprites.size,
            sprite_minimum_size: sprites.minimum_size,
            texture_layers: u32::try_from(sprites.count())?,
            pixels: sprites.pixels,
            model_catalog: models.catalog,
            model_library,
            model_placements: models.placements,
            model_gallery: models.states,
            animations: sprites.animations,
            sprite_layers: sprites.layers,
            translucent_layers: sprites.translucent,
            sprite_mips: sprites.mips,
        })
    }
}
