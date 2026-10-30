//! Port of `com.mojang.brigadier.exceptions.Dynamic3CommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

/// `Dynamic3CommandExceptionType.Function`: `Message apply(Object a, Object b, Object c)`.
pub type Dynamic3CommandExceptionTypeFunction = dyn Fn(JObjectRef, JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync;

pub struct Dynamic3CommandExceptionTypeData {
    pub function: JRef<Dynamic3CommandExceptionTypeFunction>,
}
crate::jobject!(Dynamic3CommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct Dynamic3CommandExceptionType(pub JRef<Dynamic3CommandExceptionTypeData>);

impl Dynamic3CommandExceptionType {
    pub fn new(function: impl Fn(JObjectRef, JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync + 'static) -> Dynamic3CommandExceptionType {
        Dynamic3CommandExceptionType(Arc::new(Dynamic3CommandExceptionTypeData { function: Arc::new(function) }))
    }

    pub fn create(&self, a: JObjectRef, b: JObjectRef, c: JObjectRef) -> CommandSyntaxException {
        CommandSyntaxException::new(self.into(), (self.0.function)(a, b, c))
    }

    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>, a: JObjectRef, b: JObjectRef, c: JObjectRef) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), (self.0.function)(a, b, c), reader.as_str(), reader.get_cursor())
    }
}

impl From<&Dynamic3CommandExceptionType> for CommandExceptionType {
    fn from(t: &Dynamic3CommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<Dynamic3CommandExceptionType> for CommandExceptionType {
    fn from(t: Dynamic3CommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
