//! Port of `com.mojang.brigadier.exceptions.BuiltInExceptions` (Brigadier 1.3.11): the default
//! `BuiltInExceptionProvider`, with Brigadier's English messages.
use crate::com::mojang::brigadier::exceptions::built_in_exception_provider::BuiltInExceptionProvider;
use crate::com::mojang::brigadier::exceptions::dynamic_command_exception_type::DynamicCommandExceptionType;
use crate::com::mojang::brigadier::exceptions::dynamic2_command_exception_type::Dynamic2CommandExceptionType;
use crate::com::mojang::brigadier::exceptions::simple_command_exception_type::SimpleCommandExceptionType;
use crate::com::mojang::brigadier::java_compat::string_value_of;
use crate::com::mojang::brigadier::literal_message::LiteralMessage;
use crate::com::mojang::brigadier::message::Message;
use crate::prelude::*;

pub static DOUBLE_TOO_SMALL: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, min: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Double must not be less than {}, found {}",
            string_value_of(&min),
            string_value_of(&found)
        ))
    })
});
pub static DOUBLE_TOO_BIG: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, max: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Double must not be more than {}, found {}",
            string_value_of(&max),
            string_value_of(&found)
        ))
    })
});
pub static FLOAT_TOO_SMALL: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, min: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Float must not be less than {}, found {}",
            string_value_of(&min),
            string_value_of(&found)
        ))
    })
});
pub static FLOAT_TOO_BIG: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, max: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Float must not be more than {}, found {}",
            string_value_of(&max),
            string_value_of(&found)
        ))
    })
});
pub static INTEGER_TOO_SMALL: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, min: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Integer must not be less than {}, found {}",
            string_value_of(&min),
            string_value_of(&found)
        ))
    })
});
pub static INTEGER_TOO_BIG: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, max: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Integer must not be more than {}, found {}",
            string_value_of(&max),
            string_value_of(&found)
        ))
    })
});
pub static LONG_TOO_SMALL: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, min: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Long must not be less than {}, found {}",
            string_value_of(&min),
            string_value_of(&found)
        ))
    })
});
pub static LONG_TOO_BIG: LazyLock<Dynamic2CommandExceptionType> = LazyLock::new(|| {
    Dynamic2CommandExceptionType::new(|found: JObjectRef, max: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Long must not be more than {}, found {}",
            string_value_of(&max),
            string_value_of(&found)
        ))
    })
});
pub static LITERAL_INCORRECT: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|expected: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Expected literal {}", string_value_of(&expected)))
    })
});
pub static READER_EXPECTED_START_OF_QUOTE: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| {
        SimpleCommandExceptionType::new(LiteralMessage::new("Expected quote to start a string"))
    });
pub static READER_EXPECTED_END_OF_QUOTE: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| {
        SimpleCommandExceptionType::new(LiteralMessage::new("Unclosed quoted string"))
    });
pub static READER_INVALID_ESCAPE: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|character: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Invalid escape sequence '{}' in quoted string",
            string_value_of(&character)
        ))
    })
});
pub static READER_INVALID_BOOL: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|value: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!(
            "Invalid bool, expected true or false but found '{}'",
            string_value_of(&value)
        ))
    })
});
pub static READER_INVALID_INT: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|value: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Invalid integer '{}'", string_value_of(&value)))
    })
});
pub static READER_EXPECTED_INT: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Expected integer")));
pub static READER_INVALID_LONG: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|value: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Invalid long '{}'", string_value_of(&value)))
    })
});
pub static READER_EXPECTED_LONG: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Expected long")));
pub static READER_INVALID_DOUBLE: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|value: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Invalid double '{}'", string_value_of(&value)))
    })
});
pub static READER_EXPECTED_DOUBLE: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Expected double")));
pub static READER_INVALID_FLOAT: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|value: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Invalid float '{}'", string_value_of(&value)))
    })
});
pub static READER_EXPECTED_FLOAT: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Expected float")));
pub static READER_EXPECTED_BOOL: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Expected bool")));
pub static READER_EXPECTED_SYMBOL: LazyLock<DynamicCommandExceptionType> = LazyLock::new(|| {
    DynamicCommandExceptionType::new(|symbol: JObjectRef| -> JRef<dyn Message> {
        LiteralMessage::new(&format!("Expected '{}'", string_value_of(&symbol)))
    })
});
pub static DISPATCHER_UNKNOWN_COMMAND: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| SimpleCommandExceptionType::new(LiteralMessage::new("Unknown command")));
pub static DISPATCHER_UNKNOWN_ARGUMENT: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| {
        SimpleCommandExceptionType::new(LiteralMessage::new("Incorrect argument for command"))
    });
pub static DISPATCHER_EXPECTED_ARGUMENT_SEPARATOR: LazyLock<SimpleCommandExceptionType> =
    LazyLock::new(|| {
        SimpleCommandExceptionType::new(LiteralMessage::new(
            "Expected whitespace to end one argument, but found trailing data",
        ))
    });
pub static DISPATCHER_PARSE_EXCEPTION: LazyLock<DynamicCommandExceptionType> =
    LazyLock::new(|| {
        DynamicCommandExceptionType::new(|message: JObjectRef| -> JRef<dyn Message> {
            LiteralMessage::new(&format!(
                "Could not parse command: {}",
                string_value_of(&message)
            ))
        })
    });

pub struct BuiltInExceptions;

impl BuiltInExceptions {
    pub fn new() -> JRef<BuiltInExceptions> {
        Arc::new(BuiltInExceptions)
    }
}

impl BuiltInExceptionProvider for BuiltInExceptions {
    fn double_too_low(&self) -> Dynamic2CommandExceptionType {
        DOUBLE_TOO_SMALL.clone()
    }
    fn double_too_high(&self) -> Dynamic2CommandExceptionType {
        DOUBLE_TOO_BIG.clone()
    }
    fn float_too_low(&self) -> Dynamic2CommandExceptionType {
        FLOAT_TOO_SMALL.clone()
    }
    fn float_too_high(&self) -> Dynamic2CommandExceptionType {
        FLOAT_TOO_BIG.clone()
    }
    fn integer_too_low(&self) -> Dynamic2CommandExceptionType {
        INTEGER_TOO_SMALL.clone()
    }
    fn integer_too_high(&self) -> Dynamic2CommandExceptionType {
        INTEGER_TOO_BIG.clone()
    }
    fn long_too_low(&self) -> Dynamic2CommandExceptionType {
        LONG_TOO_SMALL.clone()
    }
    fn long_too_high(&self) -> Dynamic2CommandExceptionType {
        LONG_TOO_BIG.clone()
    }
    fn literal_incorrect(&self) -> DynamicCommandExceptionType {
        LITERAL_INCORRECT.clone()
    }
    fn reader_expected_start_of_quote(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_START_OF_QUOTE.clone()
    }
    fn reader_expected_end_of_quote(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_END_OF_QUOTE.clone()
    }
    fn reader_invalid_escape(&self) -> DynamicCommandExceptionType {
        READER_INVALID_ESCAPE.clone()
    }
    fn reader_invalid_bool(&self) -> DynamicCommandExceptionType {
        READER_INVALID_BOOL.clone()
    }
    fn reader_invalid_int(&self) -> DynamicCommandExceptionType {
        READER_INVALID_INT.clone()
    }
    fn reader_expected_int(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_INT.clone()
    }
    fn reader_invalid_long(&self) -> DynamicCommandExceptionType {
        READER_INVALID_LONG.clone()
    }
    fn reader_expected_long(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_LONG.clone()
    }
    fn reader_invalid_double(&self) -> DynamicCommandExceptionType {
        READER_INVALID_DOUBLE.clone()
    }
    fn reader_expected_double(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_DOUBLE.clone()
    }
    fn reader_invalid_float(&self) -> DynamicCommandExceptionType {
        READER_INVALID_FLOAT.clone()
    }
    fn reader_expected_float(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_FLOAT.clone()
    }
    fn reader_expected_bool(&self) -> SimpleCommandExceptionType {
        READER_EXPECTED_BOOL.clone()
    }
    fn reader_expected_symbol(&self) -> DynamicCommandExceptionType {
        READER_EXPECTED_SYMBOL.clone()
    }
    fn dispatcher_unknown_command(&self) -> SimpleCommandExceptionType {
        DISPATCHER_UNKNOWN_COMMAND.clone()
    }
    fn dispatcher_unknown_argument(&self) -> SimpleCommandExceptionType {
        DISPATCHER_UNKNOWN_ARGUMENT.clone()
    }
    fn dispatcher_expected_argument_separator(&self) -> SimpleCommandExceptionType {
        DISPATCHER_EXPECTED_ARGUMENT_SEPARATOR.clone()
    }
    fn dispatcher_parse_exception(&self) -> DynamicCommandExceptionType {
        DISPATCHER_PARSE_EXCEPTION.clone()
    }
}

crate::jobject!(BuiltInExceptions: dyn BuiltInExceptionProvider);
