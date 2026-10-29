//! The server's half of the game: what only a server runs (the network side
//! and its players, world generation binding, the server's commands and
//! rules). Everything of `horizon-game` is re-exported here, so these modules
//! reach it under `crate::` as they did when they lived there.
#![forbid(unsafe_code)]
pub use horizon_game::*;
/// server.properties, eula.txt and the user lists.
pub use horizon_server_files as server;

pub mod cauldron;
pub mod command_storage;
pub mod cushion;
pub mod datapack;
pub mod flatgen;
pub mod nether_portal;
pub mod patrol;
pub mod pick_item;
pub mod pregen;
pub mod raid;
pub mod scoreboard_cmd;
pub mod server_net;
pub mod stopwatches;
pub mod tick_chunk_membership;
pub mod vanillagen;
pub mod world_spawn;
