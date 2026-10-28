//! `horizon-client`: Minecraft Java Edition 26.3 in Rust, the client players
//! run. It only joins servers (`horizon-server`, behind the proxy or not): it
//! has no single player, no LAN and no world of its own.
#![forbid(unsafe_code)]

fn main() {
    horizon_client::game::main(horizon_client::game::Program::Client);
}
