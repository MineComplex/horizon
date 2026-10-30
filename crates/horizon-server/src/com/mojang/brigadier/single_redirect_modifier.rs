//! Port of `com.mojang.brigadier.SingleRedirectModifier` (Brigadier 1.3.11): a functional interface
//! only lambdas implement, so a cheap handle around the closure.
use crate::com::mojang::brigadier::context::command_context::CommandContext;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::prelude::*;

pub type SingleRedirectModifierFn<S> =
    dyn Fn(&CommandContext<S>) -> Result<S, CommandSyntaxException> + Send + Sync;

pub struct SingleRedirectModifier<S: JGeneric>(pub JRef<SingleRedirectModifierFn<S>>);

impl<S: JGeneric> Clone for SingleRedirectModifier<S> {
    fn clone(&self) -> Self {
        SingleRedirectModifier(self.0.clone())
    }
}

impl<S: JGeneric> SingleRedirectModifier<S> {
    pub fn new(f: impl Fn(&CommandContext<S>) -> Result<S, CommandSyntaxException> + Send + Sync + 'static) -> Self {
        SingleRedirectModifier(Arc::new(f))
    }

    pub fn apply(&self, context: &CommandContext<S>) -> Result<S, CommandSyntaxException> {
        (self.0)(context)
    }
}
