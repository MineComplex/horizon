//! `/forceload`, `/fillbiome`, `/worldborder` and `/loot`.
use super::Game;

impl Game<'_> {
    // ---- /forceload ----------------------------------------------------------

    // ---- /fillbiome ----------------------------------------------------------------

    // ---- /worldborder -----------------------------------------------------------------------

    // ---- /loot ---------------------------------------------------------------------------------
}
impl crate::game::Sim<'_> {
    /// The player's movement collides with the border as it is now
    /// (`CollisionGetter.borderCollision`): from inside, its walls stop it.
    pub(in crate::game) fn player_border(&mut self) {
        if let Some(p) = self.player.as_mut() {
            p.border = Some(self.border.geometry().clone());
        }
    }
}
