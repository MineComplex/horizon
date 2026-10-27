//! The vertex of entity, item, particle and effect meshes (the entity pipelines), built on
//! the CPU by mob and particle code; the client uploads them.

/// An entity, item and effect vertex (the entity pipelines), with a face
/// normal for `entity.wgsl`'s directional light.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    /// RGBA8: tint multiplied by directional shade and ambient occlusion.
    pub color: u32,
    /// Smooth light, 16x fixed point: sky in bits 0..8, block in bits 8..16.
    pub light: u32,
    /// RGBA8_SNORM face normal; w = -127 for two-sided lighting, 0 to keep baked/unlit colour.
    pub normal: [i8; 4],
    /// The `GLINT_TEXTURING` atlas UV for a foil item quad; zero elsewhere.
    pub glint_uv: [f32; 2],
}

/// The light bytes of a crack vertex: the pass is unlit, so they carry the origin of the
/// vertex's destroy stage sprite (each crack in `LevelRenderer.destroyingBlocks` has its own
/// stage; every stage sprite has the same size, `crumbling_sprite.zw`).
pub fn stage_light([u0, v0]: [f32; 2]) -> u32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 65535.0).round() as u32;
    q(u0) | (q(v0) << 16)
}
