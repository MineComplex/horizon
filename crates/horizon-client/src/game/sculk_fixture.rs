//! The Rust side of tools/test_sculk_parity.py: with
//! `HORIZON_SCULK_FIXTURE=SCENARIOS,OUTPUT` the game loads its resources and
//! runs every scenario (sculk blocks, the player with a warning level, scripted
//! game events and block changes) through its own sculk code on a fresh
//! headless world, ticking in `ServerLevel.tick`'s order, and prints what
//! changed after every tick in the format of `VanillaSculkOracle`.
use super::Game;





impl Game<'_> {}
impl crate::game::Sim<'_> {
}
