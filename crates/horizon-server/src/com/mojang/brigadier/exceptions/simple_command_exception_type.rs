//! Port of `com.mojang.brigadier.exceptions.SimpleCommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

pub struct SimpleCommandExceptionTypeData {
    pub message: JRef<dyn Message>,
}
crate::jobject!(SimpleCommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct SimpleCommandExceptionType(pub JRef<SimpleCommandExceptionTypeData>);

impl SimpleCommandExceptionType {
    pub fn new(message: JRef<dyn Message>) -> SimpleCommandExceptionType {
        SimpleCommandExceptionType(Arc::new(SimpleCommandExceptionTypeData { message }))
    }

    pub fn create(&self) -> CommandSyntaxException {
        CommandSyntaxException::new(self.into(), self.0.message.clone())
    }

    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), self.0.message.clone(), reader.as_str(), reader.get_cursor())
    }

    pub fn to_string(&self) -> String {
        self.0.message.get_string()
    }
}

impl From<&SimpleCommandExceptionType> for CommandExceptionType {
    fn from(t: &SimpleCommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<SimpleCommandExceptionType> for CommandExceptionType {
    fn from(t: SimpleCommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
