//! The keys the game's code asks the acting player about. A server has no
//! keyboard: a player's held keys are set from its movement packets
//! (`ServerboundPlayerInputPacket`: shift is sneaking, sprint is control).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeyCode {
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    KeyW,
    KeyS,
    KeyA,
    KeyD,
}
