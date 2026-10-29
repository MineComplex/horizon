//! A server shows nothing: what the shared game code asks of a window (the
//! pointer, local particles, screens, outlines, chat lines) does nothing here.
//! Each call site is still to go (docs/crates.md); until then these keep the
//! server's copy of that code building.
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;

impl crate::game::Sim<'_> {
    /// The window's pointer grab.
    pub(crate) fn capture(&mut self, _capture: bool) {}
    /// A block's particle sprite (the client's texture atlas).
    pub(crate) fn particle_sprite(
        &self,
        _state: BlockStateId,
        _pos: [i32; 3],
    ) -> Option<(String, u32)> {
        None
    }
    /// The block outline the client draws.
    pub(crate) fn outline_boxes(&self, _pos: [i32; 3]) -> Vec<[f64; 6]> {
        Vec::new()
    }
    pub(crate) fn totem_event(&mut self, _body: super::totem::Body, _own: bool) {}
    pub(crate) fn dripstone_drip_particle(&mut self, _pos: BlockPos) {}
    pub(crate) fn open_command_block(&mut self, _pos: (i32, i32, i32)) {}
    pub(crate) fn creative_tab_open(&self) -> bool {
        false
    }
    pub(crate) fn lan_update_models(&mut self, _host: &mut super::lan::LanHost) {}
}

/// Particles the client spawns itself (`ItemParticle`, eating crumbs...): a
/// server sends its particles as packets (game/lan_particles.rs).
impl crate::game::Sim<'_> {
    pub(crate) fn item_particle(
        &self,
        _stack: &crate::hc::items::ItemStack,
    ) -> crate::hc::particles::ParticleOptions {
        crate::hc::particles::ParticleOptions::item(Vec::new())
    }
    pub(crate) fn eat_particles(&mut self, _stack: &crate::hc::items::ItemStack, _count: u32) {}
    /// Fireballs' smoke (`AbstractHurtingProjectile.createParticleTrail`): the clients draw it.
    pub(crate) fn smoke_trails(
        &mut self,
        _at: impl IntoIterator<Item = horizon_core::geometry::Vec3>,
    ) {
    }
    /// The shape break particles fill: the clients'.
    pub(crate) fn particle_shape(&self, _pos: [i32; 3], _state: BlockStateId) -> Vec<[f64; 6]> {
        Vec::new()
    }
    pub(crate) fn ensure_item_particle_sprites(&mut self) {}
    pub(crate) fn recipe_toast(&mut self, _recipes: &[String]) {}
    pub(crate) fn stop_spyglass(&mut self) {}
}
