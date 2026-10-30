//! Port of `com.mojang.brigadier.exceptions.Dynamic4CommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

/// `Dynamic4CommandExceptionType.Function`: `Message apply(Object a, Object b, Object c, Object d)`.
pub type Dynamic4CommandExceptionTypeFunction = dyn Fn(JObjectRef, JObjectRef, JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync;

pub struct Dynamic4CommandExceptionTypeData {
    pub function: JRef<Dynamic4CommandExceptionTypeFunction>,
}
crate::jobject!(Dynamic4CommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct Dynamic4CommandExceptionType(pub JRef<Dynamic4CommandExceptionTypeData>);

impl Dynamic4CommandExceptionType {
    pub fn new(function: impl Fn(JObjectRef, JObjectRef, JObjectRef, JObjectRef) -> JRef<dyn Message> + Send + Sync + 'static) -> Dynamic4CommandExceptionType {
        Dynamic4CommandExceptionType(Arc::new(Dynamic4CommandExceptionTypeData { function: Arc::new(function) }))
    }

    pub fn create(&self, a: JObjectRef, b: JObjectRef, c: JObjectRef, d: JObjectRef) -> CommandSyntaxException {
        CommandSyntaxException::new(self.into(), (self.0.function)(a, b, c, d))
    }

    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>, a: JObjectRef, b: JObjectRef, c: JObjectRef, d: JObjectRef) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), (self.0.function)(a, b, c, d), reader.as_str(), reader.get_cursor())
    }
}

impl From<&Dynamic4CommandExceptionType> for CommandExceptionType {
    fn from(t: &Dynamic4CommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<Dynamic4CommandExceptionType> for CommandExceptionType {
    fn from(t: Dynamic4CommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
