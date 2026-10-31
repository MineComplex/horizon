//! The world: storage, chunk data, scheduled ticks, the block-change path and fluids (more to
//! come: lighting).
pub mod block_changes;
pub mod block_rules;
pub mod block_update;
pub mod chunk;
pub mod chunk_io;
pub mod fluid;
#[cfg(test)]
mod fluid_oracle_tests;
#[cfg(test)]
mod fluid_tests;
pub mod level_chunk;
pub mod level_storage;
pub mod storage;
#[cfg(test)]
mod storage_tests;
pub mod ticks;
#[cfg(test)]
mod ticks_oracle_tests;
#[cfg(test)]
mod ticks_tests;
