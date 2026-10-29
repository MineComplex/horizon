//! Headless command runner for `tools/test_command_exec_parity.py`: with
//! `HORIZON_COMMAND_SCRIPT` and `HORIZON_COMMAND_OUTPUT` set, `horizon-server`
//! runs the script once a player is on (a headless `horizon-client` with
//! `HORIZON_STATIONARY_CLIENT`), then stops. Each script line is a JSON
//! operation: `cmd` runs a
//! command as the player (Commands.performPrefixedCommand) and records every
//! system message it produced, as styled spans, with the results reported to
//! the player's `CommandResultCallback`; `tick` advances the game; `settle`
//! waits until the terrain and entities around the player are loaded;
//! `portal` sends the player through the nether portal it stands in.
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
