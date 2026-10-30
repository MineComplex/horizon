//! Port of `com.mojang.authlib.exceptions.MinecraftClientException` (a RuntimeException). The calls that
//! throw it return `Result<_, MinecraftClientException>` here, since its callers catch it.
use crate::com::mojang::authlib::exceptions::{AUTHENTICATION_EXCEPTION, AUTHENTICATION_UNAVAILABLE_EXCEPTION, exception};
use crate::java::lang::Throwable;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MinecraftClientExceptionErrorType {
    SERVICE_UNAVAILABLE,
    HTTP_ERROR,
    JSON_ERROR,
}

#[derive(Clone, Debug)]
pub struct MinecraftClientException {
    pub type_: MinecraftClientExceptionErrorType,
    pub message: String,
}
impl MinecraftClientException {
    pub fn new(type_: MinecraftClientExceptionErrorType, message: &str) -> Self {
        MinecraftClientException { type_, message: message.to_string() }
    }

    pub fn get_type(&self) -> MinecraftClientExceptionErrorType {
        self.type_
    }

    pub fn get_message(&self) -> String {
        self.message.clone()
    }

    pub fn to_authentication_exception(&self) -> Throwable {
        if self.type_ == MinecraftClientExceptionErrorType::SERVICE_UNAVAILABLE {
            return exception(AUTHENTICATION_UNAVAILABLE_EXCEPTION, None, None);
        }
        exception(AUTHENTICATION_EXCEPTION, Some(self.to_string()), None)
    }
}
impl std::fmt::Display for MinecraftClientException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "com.mojang.authlib.exceptions.MinecraftClientException: {}", self.message)
    }
}
