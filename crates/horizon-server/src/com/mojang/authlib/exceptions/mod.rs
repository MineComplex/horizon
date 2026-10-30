//! `com.mojang.authlib.exceptions`. Checked exceptions travel as `Throwable` (PORTING.md); these build
//! them with authlib's class names, so `e instanceof AuthenticationUnavailableException` is
//! `is_exception(&e, AUTHENTICATION_UNAVAILABLE_EXCEPTION)`.
pub mod minecraft_client_exception;
pub use self::minecraft_client_exception::{MinecraftClientException, MinecraftClientExceptionErrorType};
use crate::java::lang::Throwable;

pub const AUTHENTICATION_EXCEPTION: &str = "com.mojang.authlib.exceptions.AuthenticationException";
pub const AUTHENTICATION_UNAVAILABLE_EXCEPTION: &str =
    "com.mojang.authlib.exceptions.AuthenticationUnavailableException";
pub const INSECURE_PUBLIC_KEY_MISSING_EXCEPTION: &str =
    "com.mojang.authlib.minecraft.InsecurePublicKeyException$MissingException";
pub const INSECURE_PUBLIC_KEY_INVALID_EXCEPTION: &str =
    "com.mojang.authlib.minecraft.InsecurePublicKeyException$InvalidException";
pub const PROFILE_NOT_FOUND_EXCEPTION: &str = "com.mojang.authlib.services.ProfileNotFoundException";

/// `new <class>(message, cause)`.
pub fn exception(class: &'static str, message: Option<String>, cause: Option<Throwable>) -> Throwable {
    // PORT: java.lang.Throwable's constructor (class name, message, cause) from the java.lang port.
    Throwable::of(class, message, cause)
}

/// `t instanceof <class>` (or a subclass: AuthenticationUnavailableException is an AuthenticationException).
pub fn is_exception(t: &Throwable, class: &str) -> bool {
    // PORT: java.lang.Throwable's class name accessor from the java.lang port.
    let name = t.class_name();
    name == class || (class == AUTHENTICATION_EXCEPTION && name == AUTHENTICATION_UNAVAILABLE_EXCEPTION)
}
