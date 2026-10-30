//! Port of `com.mojang.brigadier.exceptions.Dynamic2CommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

/// `Dynamic2CommandExceptionType.Function`: `Message apply(Object a, Object b)`.
pub type Dynamic2CommandExceptionTypeFunction = dyn Fn(JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync;

pub struct Dynamic2CommandExceptionTypeData {
    pub function: JRef<Dynamic2CommandExceptionTypeFunction>,
}
crate::jobject!(Dynamic2CommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct Dynamic2CommandExceptionType(pub JRef<Dynamic2CommandExceptionTypeData>);

impl Dynamic2CommandExceptionType {
    pub fn new(function: impl Fn(JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync + 'static) -> Dynamic2CommandExceptionType {
        Dynamic2CommandExceptionType(Arc::new(Dynamic2CommandExceptionTypeData { function: Arc::new(function) }))
    }

    pub fn create(&self, a: JObjectRef, b: JObjectRef) -> CommandSyntaxException {
        CommandSyntaxException::new(self.into(), (self.0.function)(a, b))
    }

    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>, a: JObjectRef, b: JObjectRef) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), (self.0.function)(a, b), reader.as_str(), reader.get_cursor())
    }
}

impl From<&Dynamic2CommandExceptionType> for CommandExceptionType {
    fn from(t: &Dynamic2CommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<Dynamic2CommandExceptionType> for CommandExceptionType {
    fn from(t: Dynamic2CommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
