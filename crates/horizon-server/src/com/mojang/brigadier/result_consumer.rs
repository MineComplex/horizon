//! Port of `com.mojang.brigadier.ResultConsumer` (Brigadier 1.3.11): a functional interface only
//! lambdas implement, so a cheap handle around the closure.
use crate::com::mojang::brigadier::context::command_context::CommandContext;
use crate::prelude::*;

pub type ResultConsumerFn<S> = dyn Fn(&CommandContext<S>, bool, i32) + Send + Sync;

pub struct ResultConsumer<S: JGeneric>(pub JRef<ResultConsumerFn<S>>);

impl<S: JGeneric> Clone for ResultConsumer<S> {
    fn clone(&self) -> Self {
        ResultConsumer(self.0.clone())
    }
}

impl<S: JGeneric> ResultConsumer<S> {
    pub fn new(f: impl Fn(&CommandContext<S>, bool, i32) + Send + Sync + 'static) -> Self {
        ResultConsumer(Arc::new(f))
    }

    pub fn on_command_complete(&self, context: &CommandContext<S>, success: bool, result: i32) {
        (self.0)(context, success, result)
    }
}
