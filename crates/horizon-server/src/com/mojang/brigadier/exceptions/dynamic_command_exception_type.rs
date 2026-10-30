//! Port of `com.mojang.brigadier.exceptions.DynamicCommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

/// Java's `Function<Object, Message>`.
pub type DynamicCommandExceptionTypeFunction = dyn Fn(JObjectRef) -> JRef<dyn Message> + Send + Sync;

pub struct DynamicCommandExceptionTypeData {
    pub function: JRef<DynamicCommandExceptionTypeFunction>,
}
crate::jobject!(DynamicCommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct DynamicCommandExceptionType(pub JRef<DynamicCommandExceptionTypeData>);

impl DynamicCommandExceptionType {
    pub fn new(function: impl Fn(JObjectRef) -> JRef<dyn Message> + Send + Sync + 'static) -> DynamicCommandExceptionType {
        DynamicCommandExceptionType(Arc::new(DynamicCommandExceptionTypeData { function: Arc::new(function) }))
    }

    pub fn create(&self, arg: JObjectRef) -> CommandSyntaxException {
        CommandSyntaxException::new(self.into(), (self.0.function)(arg))
    }

    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>, arg: JObjectRef) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), (self.0.function)(arg), reader.as_str(), reader.get_cursor())
    }
}

impl From<&DynamicCommandExceptionType> for CommandExceptionType {
    fn from(t: &DynamicCommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<DynamicCommandExceptionType> for CommandExceptionType {
    fn from(t: DynamicCommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
