//! A structure start as a chunk keeps it for its save: its name and its
//! `StructureStart.createTag`. Generation (`worldgen::structure::start`)
//! makes them; a chunk only needs to save them, so a client that generates
//! nothing never links what they are made of.
use crate::nbt::Tag;

pub trait SavedStructureStart: Send + Sync {
    /// The structure's id (`minecraft:village_plains`...).
    fn name(&self) -> &str;
    /// The start's tag in its own chunk; None while one of its pieces
    /// cannot be saved.
    fn save(&self) -> Option<Tag>;
}

impl SavedStructureStart for crate::worldgen::structure::start::StructureStart {
    fn name(&self) -> &str {
        &self.name
    }
    fn save(&self) -> Option<Tag> {
        crate::worldgen::structure::start::StructureStart::save(self)
    }
}
