//! Port of `com.mojang.brigadier.exceptions.CommandExceptionType` (Brigadier 1.3.11).
//!
//! A marker interface the exception-type classes implement; the port passes it as a handle naming the
//! exception type object (compared by identity, as Java's `==`). `SimpleCommandExceptionType` and the
//! `Dynamic*` types convert into it (`ERROR_X.clone().into()`, `(&ERROR_X).into()`).
use crate::prelude::*;

#[derive(Clone)]
pub struct CommandExceptionType(pub JObjectRef);

impl CommandExceptionType {
    /// The exception type object itself (`SimpleCommandExceptionTypeData`, `DynamicCommandExceptionTypeData`...).
    pub fn as_object(&self) -> JObjectRef {
        self.0.clone()
    }
}

impl PartialEq for CommandExceptionType {
    fn eq(&self, other: &Self) -> bool {
        same(&self.0, &other.0)
    }
}

impl Eq for CommandExceptionType {}

impl std::fmt::Debug for CommandExceptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.class_name())
    }
}
