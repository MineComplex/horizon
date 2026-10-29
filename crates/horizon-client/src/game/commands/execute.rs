//! `/execute` (ExecuteCommand): walk the parsed redirect chain, forking and
//! filtering command sources, then run the final command once per source,
//! reporting each result to the `store` callbacks of its source.
use super::{Game, Target};

/// `CommandSourceStack` fields Horizon models: the executing entity (None for
/// the server), position, rotation, anchor, whether feedback is suppressed
/// and the chain of `store` callbacks.
#[derive(Clone, Copy, Debug)]
pub(in crate::game) struct Source {
    pub entity: Option<Target>,
    pub pos: [f64; 3],
    pub rot: [f32; 2],
    /// `withSuppressedOutput`: functions do not print feedback.
    pub silent: bool,
    /// The command block whose output this source is
    /// (`BaseCommandBlock.CloseableCommandBlockSource`).
    pub block: Option<(i32, i32, i32)>,
}

impl Game<'_> {
    // ---- running commands -----------------------------------------------

    // ---- conditions ----------------------------------------------------------
}
impl crate::game::Sim<'_> {
    /// The source a command typed by the player starts from.
    pub(in crate::game) fn player_source(&self) -> Source {
        Source {
            // The server console has no entity (CommandSourceStack of the server).
            entity: self.player.is_some().then_some(Target::Player),
            pos: self.source_position(),
            rot: self.source_rotation(),
            silent: false,
            block: None,
        }
    }
}
