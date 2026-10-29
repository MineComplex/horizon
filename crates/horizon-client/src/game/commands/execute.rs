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
impl crate::game::Sim<'_> {}
