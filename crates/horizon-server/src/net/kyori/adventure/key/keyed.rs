//! Port of `net.kyori.adventure.key.Keyed` (adventure-key).
use crate::prelude::*;
use crate::net::kyori::adventure::key::Key;

// ---- interface net.kyori.adventure.key.Keyed
pub trait Keyed: Send + Sync {
    fn key(&self) -> JRef<dyn Key>;
}
