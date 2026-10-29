//! Nether portals on the client: `LocalPlayer.handlePortalTransitionEffect`
//! (the confusion effect, closing screens, the trigger sound). Travel is the
//! server's.
use super::{Game, Screen};
use horizon_core::random::RandomSource;

/// The nether portal state of the world (`Game::portals`).
#[derive(Default)]
pub(super) struct Portals {
    /// `LocalPlayer.portalProcess.isInsidePortalThisTick`.
    client_inside: bool,
    /// Per-chunk portal palette masks, kept across ticks (portal_sections).
    pub(super) sections: horizon_client::portal_sections::PortalSections,
}

impl Game<'_> {
    // ---- the player ----

    /// `LocalPlayer.handlePortalTransitionEffect`: whether the confusion
    /// effect grows this tick. Entering a portal closes screens other than
    /// the chat and death screens and plays `block.portal.trigger` when the
    /// effect starts from nothing.
    pub(super) fn client_portal_effect(&mut self) -> bool {
        if !std::mem::take(&mut self.portals.client_inside) {
            return false;
        }
        match self.screen {
            Screen::Playing | Screen::Chat | Screen::Dead => {}
            Screen::Inventory | Screen::Creative => {
                self.close_container();
                self.carried = None;
                self.screen = Screen::Playing;
                self.capture(true);
            }
            _ => {
                self.screen = Screen::Playing;
                self.capture(true);
            }
        }
        if self.screen_fx.portal == 0.0 {
            let pitch = self.world.random.next_float() * 0.4 + 0.8;
            self.sounds
                .play("minecraft:block.portal.trigger", None, 0.25, pitch);
        }
        true
    }

    // ---- other entities ----

    // ---- persistence ----
}
impl crate::game::Game<'_> {
    /// The same on a server's client: `Entity.canUsePortal` (alive, not a
    /// passenger), then only the client's `setAsInsidePortal`, which drives
    /// the confusion effect (`client_portal_effect`).
    pub(super) fn client_inside_nether_portal(&mut self) {
        let alive = self
            .player
            .as_ref()
            .is_some_and(|p| !p.health.dead_or_dying());
        if alive && self.riding.is_none() {
            self.portals.client_inside = true;
        }
    }
}
