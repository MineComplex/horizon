//! Chunk data: paletted containers of block states and biomes (`PalettedContainer` and its bit
//! storage), in the bit layout the chunk packet and the region files carry.
pub mod bit_storage;
pub mod paletted;
#[cfg(test)]
mod tests;

pub use bit_storage::BitStorage;
pub use paletted::{ContainerError, Kind, PackedData, PaletteValue, PalettedContainer, Strategy};
