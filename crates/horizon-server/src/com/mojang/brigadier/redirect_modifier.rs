//! Port of `com.mojang.brigadier.RedirectModifier` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::context::command_context::CommandContext;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::prelude::*;

/// `@FunctionalInterface interface RedirectModifier<S>`: `Collection<S> apply(CommandContext<S>)`.
pub trait RedirectModifier<S: JGeneric>: Send + Sync {
    fn apply(&self, context: &CommandContext<S>) -> Result<Vec<S>, CommandSyntaxException>;

    /// The modifier as `java.lang.Object` for `instanceof` checks (see `Command::as_object`).
    fn as_object(self: Arc<Self>) -> Option<JObjectRef> {
        None
    }
}

impl<S: JGeneric, F> RedirectModifier<S> for F
where
    F: Fn(&CommandContext<S>) -> Result<Vec<S>, CommandSyntaxException> + Send + Sync,
{
    fn apply(&self, context: &CommandContext<S>) -> Result<Vec<S>, CommandSyntaxException> {
        self(context)
    }
}
