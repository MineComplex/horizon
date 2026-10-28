//! Boats and minecarts for a server's players: LAN guests, players on a
//! dedicated server, and single player's window on its server thread.
//!
//! A player aims at a vehicle, item frame, armor stand or painting as at a
//! mob (`ServerGamePacketListenerImpl.handleInteract`), and the game's own
//! code for it runs as that player (`AbstractBoat.interact` seats it,
//! `VehicleEntity.hurtServer` breaks the vehicle). A boat is driven by its
//! rider's client (`handleMoveVehicle`, `handlePaddleBoat`); a minecart moves
//! here with the rider's input (`ServerPlayer.getLastClientMoveIntent`).
//! Sneaking gets out (`Player.rideTick`: `wantsToStopRiding`), and the client
//! is teleported where the player lands (`ServerPlayer.dismountTo`).
use super::Game;



impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// Whether a server player other than the one acting rides `vehicle`
    /// (`Entity.canAddPassenger`: a boat or minecart seats one player here).
    pub(super) fn ridden_by_guest(&self, vehicle: u32) -> bool {
        false
    }
}
