//! What the player is doing, for Discord Rich Presence (`horizon_client::discord_presence`).
use super::*;
use horizon_client::discord_presence::{self as presence, Dimension, Status};

impl Game<'_> {
    /// Each frame: only a change reaches the presence thread.
    pub(super) fn update_discord_presence(&self) {
        presence::set(self.discord_status());
    }
    fn discord_status(&self) -> Status {
        if !self.settings.extras.discord_presence {
            return Status::Off;
        }
        if self.title.is_some() {
            return Status::Menus;
        }
        Status::World {
            dimension: match self.dim {
                Dim::Overworld => Dimension::Overworld,
                Dim::Nether => Dimension::Nether,
                Dim::End => Dimension::End,
            },
            mode: self.mode,
        }
    }
}
