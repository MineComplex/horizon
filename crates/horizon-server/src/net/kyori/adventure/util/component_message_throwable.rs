//! Port of `net.kyori.adventure.util.ComponentMessageThrowable`.
use crate::net::kyori::adventure::text::Component;

// ---- interface net.kyori.adventure.util.ComponentMessageThrowable
pub trait ComponentMessageThrowable: Send + Sync {
    fn component_message(&self) -> Option<Component>;
}

/// `ComponentMessageThrowable.getMessage(Throwable)`: the component message of a throwable that has one.
pub fn get_message(throwable: Option<&dyn ComponentMessageThrowable>) -> Option<Component> {
    throwable.and_then(|t| t.component_message())
}
/// `ComponentMessageThrowable.getOrConvertMessage(Throwable)`: its component message, else its
/// message as text.
pub fn get_or_convert_message(throwable: Option<&dyn ComponentMessageThrowable>, message: Option<String>) -> Option<Component> {
    if let Some(t) = throwable {
        return t.component_message();
    }
    message.map(|m| Component::text(m))
}
