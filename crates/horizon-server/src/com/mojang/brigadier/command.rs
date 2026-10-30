//! Port of `com.mojang.brigadier.Command` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::context::command_context::CommandContext;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::prelude::*;

pub const SINGLE_SUCCESS: i32 = 1;

/// `@FunctionalInterface interface Command<S>`; closures `|ctx| -> Result<i32, CommandSyntaxException>`
/// implement it.
pub trait Command<S: JGeneric>: Send + Sync {
    fn run(&self, context: &CommandContext<S>) -> Result<i32, CommandSyntaxException>;

    /// The command as `java.lang.Object`, for `instanceof` checks on commands (`command instanceof
    /// CustomCommandExecutor`): ported classes that implement `Command` return `Some(self)`; closures
    /// (Java lambdas) return `None`.
    fn as_object(self: Arc<Self>) -> Option<JObjectRef> {
        None
    }
}

impl<S: JGeneric, F> Command<S> for F
where
    F: Fn(&CommandContext<S>) -> Result<i32, CommandSyntaxException> + Send + Sync,
{
    fn run(&self, context: &CommandContext<S>) -> Result<i32, CommandSyntaxException> {
        self(context)
    }
}
