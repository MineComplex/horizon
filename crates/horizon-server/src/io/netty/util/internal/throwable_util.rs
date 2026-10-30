//! Port of `io.netty.util.internal.ThrowableUtil`, plus the one place where the netty port meets
//! `java.lang.Throwable`.
//!
//! Every exception netty creates or inspects goes through the functions here, so the netty port depends
//! on exactly this much of the java.lang port:
//! - `Throwable::of_object(object, message, cause)`: a Throwable whose class is the ported exception
//!   object (`DecoderException`, `ClosedChannelException`...), with Java's message and cause;
//! - `t.get_message()`, `t.get_cause()`, `t.object()` (the exception object, for `instanceof`).
//!
//! Unchecked exceptions are panics (PORTING.md): `throw_(t)` panics with the `Throwable` as payload and
//! `from_panic` turns any panic payload back into a `Throwable` (the pipeline catches handler panics and
//! fires them as `exceptionCaught`, as Netty does with RuntimeExceptions).
use crate::java::lang::Throwable;
use crate::prelude::*;
use std::any::Any;

/// `java.lang.RuntimeException` raised by a Rust panic with a plain message.
pub struct PanicException {
    pub message: String,
}
crate::jobject!(PanicException);

/// `java.io.IOException` for an operating-system I/O error (Netty's transports surface these).
pub struct NativeIoException {
    pub message: String,
    pub kind: std::io::ErrorKind,
}
crate::jobject!(NativeIoException);

/// A Throwable for the exception object `object` (its class), with Java's message and cause.
pub fn new_throwable(
    object: JObjectRef,
    message: Option<String>,
    cause: Option<Throwable>,
) -> Throwable {
    // PORT: java.lang.Throwable's port provides the constructor from an exception object.
    Throwable::of_object(object, message, cause)
}

/// `t.getMessage()`.
pub fn message(t: &Throwable) -> Option<String> {
    t.get_message()
}

/// `t.getCause()`.
pub fn cause(t: &Throwable) -> Option<Throwable> {
    t.get_cause()
}

/// `t instanceof T`: the exception object is (or implements) `T`.
pub fn is<T: ?Sized + 'static>(t: &Throwable) -> bool {
    instance_of::<T, dyn JObject>(&t.object())
}

/// `(T) t`: the exception object as `T`.
pub fn as_<T: ?Sized + 'static>(t: &Throwable) -> Option<JRef<T>> {
    cast::<T, dyn JObject>(&t.object())
}

/// `throw t` for an unchecked exception: panics with `t` as the payload.
pub fn throw_(t: Throwable) -> ! {
    std::panic::panic_any(t)
}

/// The Throwable a panic carried: the thrown Throwable itself, or a RuntimeException with the panic's message.
pub fn from_panic(payload: Box<dyn Any + Send>) -> Throwable {
    let payload = match payload.downcast::<Throwable>() {
        Ok(t) => return *t,
        Err(p) => p,
    };
    let message = if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else {
        "panic".to_string()
    };
    new_throwable(
        Arc::new(PanicException {
            message: message.clone(),
        }),
        Some(message),
        None,
    )
}

/// An `IOException` for an operating-system error.
pub fn from_io_error(e: &std::io::Error) -> Throwable {
    let message = e.to_string();
    new_throwable(
        Arc::new(NativeIoException {
            message: message.clone(),
            kind: e.kind(),
        }),
        Some(message),
        None,
    )
}

/// Runs `f`, turning a panic into the Throwable it threw (Java: `try { .. } catch (Throwable t)`).
pub fn catch<R>(f: impl FnOnce() -> Result<R, Throwable>) -> Result<R, Throwable> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => Err(from_panic(p)),
    }
}

/// `ThrowableUtil.stackTraceToString(cause)`: the port keeps no stack traces, so the message chain.
pub fn stack_trace_to_string(cause: &Throwable) -> String {
    let mut out = describe(cause);
    let mut c = self::cause(cause);
    while let Some(t) = c {
        out.push_str("\nCaused by: ");
        out.push_str(&describe(&t));
        c = self::cause(&t);
    }
    out
}

/// `t.toString()`: the class name and the message.
pub fn describe(t: &Throwable) -> String {
    let class = t.object().class_name().to_string();
    match message(t) {
        Some(m) => format!("{class}: {m}"),
        None => class,
    }
}

/// Declares a ported exception class: a struct with Java's message and cause, its constructors in
/// Java's order (`new()`, `new_2(message, cause)`, `new_1(message)`, `new_1_2(cause)`), the conversion
/// to `Throwable` (`Throwable::from(e)`, `e.into()`), and the marker traits of its superclasses for
/// `instanceof` (`throwable_util::is::<dyn DecoderExceptionTrait>(&t)`).
#[macro_export]
macro_rules! netty_exception {
    ($name:ident $(: $($tr:ident),* $(,)?)?) => {
        pub struct $name {
            pub this: ::std::sync::Weak<Self>,
            pub message: Option<String>,
            pub cause: Option<$crate::java::lang::Throwable>,
        }
        impl $name {
            pub fn this(&self) -> ::std::sync::Arc<Self> {
                self.this.upgrade().expect("this")
            }
            fn create(message: Option<String>, cause: Option<$crate::java::lang::Throwable>) -> ::std::sync::Arc<Self> {
                ::std::sync::Arc::new_cyclic(|this| $name { this: this.clone(), message, cause })
            }
            pub fn new() -> ::std::sync::Arc<Self> {
                Self::create(None, None)
            }
            pub fn new_2(message: &str, cause: $crate::java::lang::Throwable) -> ::std::sync::Arc<Self> {
                Self::create(Some(message.to_string()), Some(cause))
            }
            pub fn new_1(message: &str) -> ::std::sync::Arc<Self> {
                Self::create(Some(message.to_string()), None)
            }
            /// Java: the message is `cause.toString()`.
            pub fn new_1_2(cause: $crate::java::lang::Throwable) -> ::std::sync::Arc<Self> {
                let message = $crate::io::netty::util::internal::throwable_util::describe(&cause);
                Self::create(Some(message), Some(cause))
            }
            pub fn get_message(&self) -> Option<String> {
                self.message.clone()
            }
            pub fn get_cause(&self) -> Option<$crate::java::lang::Throwable> {
                self.cause.clone()
            }
            /// This exception as a `Throwable` (`throw new X(..)` -> `Err(X::new_1(..).throwable())`).
            pub fn throwable(&self) -> $crate::java::lang::Throwable {
                $crate::io::netty::util::internal::throwable_util::new_throwable(
                    self.this(),
                    self.message.clone(),
                    self.cause.clone(),
                )
            }
        }
        impl From<::std::sync::Arc<$name>> for $crate::java::lang::Throwable {
            fn from(e: ::std::sync::Arc<$name>) -> Self {
                e.throwable()
            }
        }
        $($(impl $tr for $name {})*)?
        $crate::jobject!($name $(: $(dyn $tr),*)?);
    };
}
