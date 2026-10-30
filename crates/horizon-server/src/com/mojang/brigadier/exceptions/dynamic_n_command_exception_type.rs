//! Port of `com.mojang.brigadier.exceptions.DynamicNCommandExceptionType` (Brigadier 1.3.11).
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;
use std::borrow::Borrow;

/// `DynamicNCommandExceptionType.Function`: `Message apply(Object[] args)`.
pub type DynamicNCommandExceptionTypeFunction = dyn Fn(&[JObjectRef]) -> JRef<dyn Message> + Send + Sync;

pub struct DynamicNCommandExceptionTypeData {
    pub function: JRef<DynamicNCommandExceptionTypeFunction>,
}
crate::jobject!(DynamicNCommandExceptionTypeData);

/// A shared handle (statics hold it by value; clones are the same exception type).
#[derive(Clone)]
pub struct DynamicNCommandExceptionType(pub JRef<DynamicNCommandExceptionTypeData>);

impl DynamicNCommandExceptionType {
    pub fn new(function: impl Fn(&[JObjectRef]) -> JRef<dyn Message> + Send + Sync + 'static) -> DynamicNCommandExceptionType {
        DynamicNCommandExceptionType(Arc::new(DynamicNCommandExceptionTypeData { function: Arc::new(function) }))
    }

    /// `create(Object a, Object... args)`: as in Brigadier, `a` is not passed to the function.
    pub fn create(&self, a: JObjectRef, args: Vec<JObjectRef>) -> CommandSyntaxException {
        let _ = a;
        CommandSyntaxException::new(self.into(), (self.0.function)(&args))
    }

    /// `createWithContext(ImmutableStringReader reader, Object... args)`.
    pub fn create_with_context(&self, reader: impl Borrow<ImmutableStringReader>, args: Vec<JObjectRef>) -> CommandSyntaxException {
        let reader = reader.borrow();
        CommandSyntaxException::new_4(self.into(), (self.0.function)(&args), reader.as_str(), reader.get_cursor())
    }
}

impl From<&DynamicNCommandExceptionType> for CommandExceptionType {
    fn from(t: &DynamicNCommandExceptionType) -> Self {
        CommandExceptionType(t.0.clone())
    }
}

impl From<DynamicNCommandExceptionType> for CommandExceptionType {
    fn from(t: DynamicNCommandExceptionType) -> Self {
        CommandExceptionType(t.0)
    }
}
