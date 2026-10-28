//! A server shows nothing: what the shared game code asks of a window (the
//! pointer, local particles, screens, outlines, chat lines) does nothing here.
//! Each call site is still to go (docs/crates.md); until then these keep the
//! server's copy of that code building.
use crate::hc::text::Text;
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;

impl crate::game::Sim<'_> {
    /// The window's pointer grab.
    pub(crate) fn capture(&mut self, _capture: bool) {}
    /// A block's particle sprite (the client's texture atlas).
    pub(crate) fn particle_sprite(&self, _state: BlockStateId, _pos: [i32; 3]) -> Option<(String, u32)> {
        None
    }
    /// The block outline the client draws.
    pub(crate) fn outline_boxes(&self, _pos: [i32; 3]) -> Vec<[f64; 6]> {
        Vec::new()
    }
    pub(crate) fn totem_event(&mut self, _body: super::totem::Body, _own: bool) {}
    pub(crate) fn dripstone_drip_particle(&mut self, _pos: BlockPos) {}
    pub(crate) fn open_sign_editor(&mut self, _pos: (i32, i32, i32), _front: bool) {}
    pub(crate) fn open_command_block(&mut self, _pos: (i32, i32, i32)) {}
    pub(crate) fn creative_inventory_open(&self) -> bool {
        false
    }
    pub(crate) fn creative_tab_open(&self) -> bool {
        false
    }
    pub(crate) fn lan_update_models(&mut self, _host: &mut super::lan::LanHost) {}
    pub(crate) fn local_chat_line(&mut self, _text: Text, _indicator: Option<u32>) {}
}
