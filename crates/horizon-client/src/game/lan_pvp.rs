//! Players hitting players in an open world (`Player.attack` on a player):
//! the host hits a guest, a guest hits the host or another guest. The hit
//! is worked out as the attacker (weapon, attack strength, critical hit,
//! enchantments, a mace's smash) and lands on the victim like any player
//! attack (blocking, armour, knockback), when the `pvp` game rule allows it
//! (`ServerPlayer.canHarmPlayer`, `isPvpAllowed`). A player's arrow on
//! another player lands the same way.

impl crate::game::Sim<'_> {}
