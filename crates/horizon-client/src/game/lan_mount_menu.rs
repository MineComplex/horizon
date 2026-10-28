//! The horse inventory for a server's players (`ServerPlayer.openHorseInventory`):
//! shift + use on a tamed horse, donkey, mule, camel or llama, or the
//! inventory key while riding one (`ServerboundPlayerCommandPacket`
//! OPEN_INVENTORY), opens `HorseInventoryMenu` over the mount with
//! `ClientboundMountScreenOpenPacket`. Clicks run on the menu engine
//! (`Kind::Mount`) with the mount's saddle, body item and chest copied in and
//! written back, and the slots go to the client after each change, as
//! `broadcastChanges` does.
use super::Game;


impl Game<'_> {
}
impl crate::game::Sim<'_> {
}
