use super::{Suggestions, SuggestionsBuilder};
use crate::azalea::brigadier::context::CommandContext;

pub trait SuggestionProvider<S> {
    fn get_suggestions(
        &self,
        context: CommandContext<S>,
        builder: SuggestionsBuilder,
    ) -> Suggestions;
}
