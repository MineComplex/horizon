//! The keys the game's code asks the acting player about. A server has no
//! keyboard: a guest's held keys are set from its movement packets
//! (`ServerboundPlayerInputPacket`: shift is sneaking, sprint is control).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeyCode {
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    KeyW,
    KeyS,
    KeyA,
    KeyD,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
}
