//! The command harness's player (`tools/test_command_exec_parity.py`): a
//! headless `horizon-client` with `HORIZON_STATIONARY_CLIENT` that stays on
//! while `horizon-server` runs the script (`HORIZON_COMMAND_SCRIPT`).
use super::Game;
use horizon_client::Result;
use std::time::Duration;

impl Game<'_> {
    /// The command harness's player (`HORIZON_STATIONARY_CLIENT`): pump the
    /// connection without predicting movement, so the player stays where
    /// commands put it, until the server ends the connection.
    pub(super) fn run_stationary_client(&mut self) -> Result<()> {
        loop {
            self.poll_stationary_client()?;
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
