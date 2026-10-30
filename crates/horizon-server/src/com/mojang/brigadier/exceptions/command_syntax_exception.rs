//! Port of `com.mojang.brigadier.exceptions.CommandSyntaxException`
//! (leaf-server/src/minecraft/java/com/mojang/brigadier/exceptions/CommandSyntaxException.java, Paper's
//! patched Brigadier: implements adventure's `ComponentMessageThrowable`).
//!
//! The error value of every Brigadier `Result` (`Command` bodies, argument parsing, the dispatcher):
//! all its fields are final, so it is a plain cloneable value. Its one subclass,
//! `TagParseCommandSyntaxException`, is recognized by its exception type:
//! `e.get_type() == (&tag_parse_command_syntax_exception::EXCEPTION_TYPE.clone()).into()`.
use crate::com::mojang::brigadier::exceptions::built_in_exception_provider::BuiltInExceptionProvider;
use crate::com::mojang::brigadier::exceptions::built_in_exceptions::BuiltInExceptions;
use crate::com::mojang::brigadier::exceptions::command_exception_type::CommandExceptionType;
use crate::com::mojang::brigadier::java_compat;
use crate::com::mojang::brigadier::message::Message;
use crate::net::kyori::adventure::text::Component;
use crate::net::kyori::adventure::util::ComponentMessageThrowable;
use crate::prelude::*;

// ---- class com.mojang.brigadier.exceptions.CommandSyntaxException (CommandSyntaxException.java:8)
#[derive(Clone)]
pub struct CommandSyntaxException {
    pub type_: CommandExceptionType,  // CommandSyntaxException.java:13
    pub message: JRef<dyn Message>,   // CommandSyntaxException.java:14
    pub input: Option<String>,        // CommandSyntaxException.java:15
    pub cursor: i32,                  // CommandSyntaxException.java:16
}

/// The virtual methods, for subclasses (`TagParseCommandSyntaxException`) and `JRef<dyn ..>` holders.
pub trait CommandSyntaxExceptionTrait: ComponentMessageThrowable + JObject {
    fn as_command_syntax_exception(&self) -> &CommandSyntaxException;
    fn get_message(&self) -> String; // CommandSyntaxException.java:34
    fn get_raw_message(&self) -> JRef<dyn Message>; // CommandSyntaxException.java:44
    fn get_context(&self) -> Option<String>; // CommandSyntaxException.java:48
    fn get_type(&self) -> CommandExceptionType; // CommandSyntaxException.java:65
    fn get_input(&self) -> Option<String>; // CommandSyntaxException.java:69
    fn get_cursor(&self) -> i32; // CommandSyntaxException.java:73
    fn component_message(&self) -> Component; // CommandSyntaxException.java:78
}
impl std::ops::Deref for dyn CommandSyntaxExceptionTrait {
    type Target = CommandSyntaxException;
    fn deref(&self) -> &CommandSyntaxException {
        self.as_command_syntax_exception()
    }
}

pub const CONTEXT_AMOUNT: i32 = 10; // CommandSyntaxException.java:9
pub static ENABLE_COMMAND_STACK_TRACES: Mut<bool> = Mut::new(true); // CommandSyntaxException.java:10
/// `BUILT_IN_EXCEPTIONS` (CommandSyntaxException.java:11); `SharedConstants` replaces it with NMS's
/// `BrigadierExceptions`: `BUILT_IN_EXCEPTIONS.set(BrigadierExceptions::new())`.
pub static BUILT_IN_EXCEPTIONS: LazyLock<Mut<JRef<dyn BuiltInExceptionProvider>>> =
    LazyLock::new(|| Mut::new(BuiltInExceptions::new()));

/// `CommandSyntaxException.BUILT_IN_EXCEPTIONS` read.
pub fn built_in_exceptions() -> JRef<dyn BuiltInExceptionProvider> {
    BUILT_IN_EXCEPTIONS.get()
}

impl CommandSyntaxException {
    /// `new CommandSyntaxException(type, message)` (CommandSyntaxException.java:18).
    pub fn new(type_: CommandExceptionType, message: JRef<dyn Message>) -> CommandSyntaxException {
        CommandSyntaxException { type_, message, input: None, cursor: -1 }
    }

    /// `new CommandSyntaxException(type, message, input, cursor)` (CommandSyntaxException.java:26).
    pub fn new_4(type_: CommandExceptionType, message: JRef<dyn Message>, input: &str, cursor: i32) -> CommandSyntaxException {
        CommandSyntaxException { type_, message, input: Some(input.to_string()), cursor }
    }

    pub fn get_message(&self) -> String {
        let mut message = self.message.get_string();
        let context = self.get_context();
        if let Some(context) = context {
            message += &format!(" at position {}: {}", self.cursor, context);
        }
        message
    }

    pub fn get_raw_message(&self) -> JRef<dyn Message> {
        self.message.clone()
    }

    pub fn get_context(&self) -> Option<String> {
        let input = self.input.as_ref()?;
        if self.cursor < 0 {
            return None;
        }
        let mut builder = String::new();
        let cursor = java_compat::utf16_len(input).min(self.cursor);

        if cursor > CONTEXT_AMOUNT {
            builder.push_str("...");
        }

        builder.push_str(&java_compat::substring(input, 0.max(cursor - CONTEXT_AMOUNT), cursor));
        builder.push_str("<--[HERE]");

        Some(builder)
    }

    pub fn get_type(&self) -> CommandExceptionType {
        self.type_.clone()
    }

    pub fn get_input(&self) -> Option<String> {
        self.input.clone()
    }

    pub fn get_cursor(&self) -> i32 {
        self.cursor
    }

    // Paper start - Brigadier API
    pub fn component_message(&self) -> Component {
        crate::io::papermc::paper::brigadier::paper_brigadier::PaperBrigadier::component_from_message(self.message.clone())
    }
    // Paper end - Brigadier API
}

impl std::fmt::Debug for CommandSyntaxException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CommandSyntaxException: {}", self.get_message())
    }
}

impl std::fmt::Display for CommandSyntaxException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.get_message())
    }
}

impl std::error::Error for CommandSyntaxException {}

// ---- hierarchy glue
impl ComponentMessageThrowable for CommandSyntaxException {}
impl CommandSyntaxExceptionTrait for CommandSyntaxException {
    fn as_command_syntax_exception(&self) -> &CommandSyntaxException {
        self
    }
    fn get_message(&self) -> String {
        CommandSyntaxException::get_message(self)
    }
    fn get_raw_message(&self) -> JRef<dyn Message> {
        CommandSyntaxException::get_raw_message(self)
    }
    fn get_context(&self) -> Option<String> {
        CommandSyntaxException::get_context(self)
    }
    fn get_type(&self) -> CommandExceptionType {
        CommandSyntaxException::get_type(self)
    }
    fn get_input(&self) -> Option<String> {
        CommandSyntaxException::get_input(self)
    }
    fn get_cursor(&self) -> i32 {
        CommandSyntaxException::get_cursor(self)
    }
    fn component_message(&self) -> Component {
        CommandSyntaxException::component_message(self)
    }
}
crate::jobject!(CommandSyntaxException: dyn CommandSyntaxExceptionTrait);
