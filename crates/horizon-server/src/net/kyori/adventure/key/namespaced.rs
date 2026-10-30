//! Port of `net.kyori.adventure.key.Namespaced` (adventure-key).

// ---- interface net.kyori.adventure.key.Namespaced
pub trait Namespaced: Send + Sync {
    fn namespace(&self) -> String;
}
