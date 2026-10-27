//! How far a chunk is through generation, as the loading screen's chunk map shows it.

/// `ChunkStatus`, as far as the chunk status map tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Empty,
    StructureStarts,
    StructureReferences,
    Biomes,
    Terrain,
    Features,
    InitializeLight,
    Light,
    Spawn,
    Full,
}
impl Status {
    /// `LevelLoadingScreen.COLORS` (RGB).
    pub fn color(self) -> u32 {
        match self {
            Self::Empty => 0x545454,
            Self::StructureStarts => 0x999999,
            Self::StructureReferences => 0x5F6191,
            Self::Biomes => 0x80B252,
            Self::Terrain => 0x303572,
            Self::Features => 0x21C600,
            Self::InitializeLight => 0xCCCCCC,
            Self::Light => 0xFFE0A0,
            Self::Spawn => 0xF26060,
            Self::Full => 0xFFFFFF,
        }
    }
}
