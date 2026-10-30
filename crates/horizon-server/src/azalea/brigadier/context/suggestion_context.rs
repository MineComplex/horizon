use std::sync::Arc;

use parking_lot::RwLock;

use crate::azalea::brigadier::tree::CommandNode;

#[derive(Debug)]
pub struct SuggestionContext<S> {
    pub parent: Arc<RwLock<CommandNode<S>>>,
    pub start_pos: usize,
}
