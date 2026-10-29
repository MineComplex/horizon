//! `/bossbar` (BossBarCommands) and drawing the custom boss bars.
use super::Game;
use horizon_client::boss_overlay;
use horizon_protocol::packets::play;

impl Game<'_> {
    // ---- ServerBossEvent delivery ------------------------------------------------

    /// `ClientPacketListener.handleBossUpdate`.
    pub(super) fn apply_boss_event(&mut self, packet: &play::BossEvent) {
        self.boss_overlay.apply(packet, boss_overlay::now_millis());
    }
}
