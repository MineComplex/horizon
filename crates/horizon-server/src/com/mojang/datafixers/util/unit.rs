//! Port of `com.mojang.datafixers.util.Unit` (DFU): `Unit::INSTANCE`.
use crate::prelude::*;

// ---- enum com.mojang.datafixers.util.Unit
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
pub enum Unit {
    #[default]
    INSTANCE,
}

impl Unit {
    pub fn values() -> Vec<Unit> {
        vec![Unit::INSTANCE]
    }
    pub fn name(&self) -> String {
        "INSTANCE".to_string()
    }
    pub fn ordinal(&self) -> i32 {
        0
    }
    pub fn to_string(&self) -> String {
        "Unit".to_string()
    }
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Unit")
    }
}
crate::jobject!(Unit);
