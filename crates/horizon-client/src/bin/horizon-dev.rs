//! `horizon-dev`: the game for development and the test harnesses, never
//! shipped to players. Besides what `horizon-client` does, it runs worlds of
//! its own (`--world` on a server thread inside the window, `--benchmark`),
//! the headless fixtures (`HORIZON_DISPENSE_FIXTURE`, `HORIZON_SCULK_FIXTURE`,
//! `HORIZON_COMMAND_SCRIPT`) and `--dedicated`.
#![forbid(unsafe_code)]

fn main() {
    horizon_client::game::main(horizon_client::game::Program::Dev);
}
