//! `/pregen`, Horizon's own command: generate, light and save every chunk of a
//! square or a circle ahead of the players (docs/pregeneration.md). The task
//! lives on the server and hands its band to the level's streaming, whose
//! workers do the work; chunks behind the band unload and are saved as any
//! chunk the players leave.

impl crate::game::Sim<'_> {}
