//! Port of `com.mojang.brigadier.AmbiguityConsumer` (Brigadier 1.3.11): a functional interface only
//! lambdas implement, so a cheap handle around the closure.
use crate::com::mojang::brigadier::tree::command_node::CommandNodeTrait;
use crate::prelude::*;

pub type AmbiguityConsumerFn<S> = dyn Fn(
        JRef<dyn CommandNodeTrait<S>>,
        JRef<dyn CommandNodeTrait<S>>,
        JRef<dyn CommandNodeTrait<S>>,
        &HashSet<String>,
    ) + Send
    + Sync;

pub struct AmbiguityConsumer<S: JGeneric>(pub JRef<AmbiguityConsumerFn<S>>);

impl<S: JGeneric> Clone for AmbiguityConsumer<S> {
    fn clone(&self) -> Self {
        AmbiguityConsumer(self.0.clone())
    }
}

impl<S: JGeneric> AmbiguityConsumer<S> {
    pub fn new(
        f: impl Fn(JRef<dyn CommandNodeTrait<S>>, JRef<dyn CommandNodeTrait<S>>, JRef<dyn CommandNodeTrait<S>>, &HashSet<String>)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        AmbiguityConsumer(Arc::new(f))
    }

    pub fn ambiguous(
        &self,
        parent: JRef<dyn CommandNodeTrait<S>>,
        child: JRef<dyn CommandNodeTrait<S>>,
        sibling: JRef<dyn CommandNodeTrait<S>>,
        inputs: &HashSet<String>,
    ) {
        (self.0)(parent, child, sibling, inputs)
    }
}
