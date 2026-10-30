//! Port of `com.mojang.serialization.Lifecycle` (DFU): `stable`, `experimental`, `deprecated(since)`.
use crate::prelude::*;

// ---- class com.mojang.serialization.Lifecycle (and its subclass Lifecycle.Deprecated)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lifecycle {
    Stable,
    Experimental,
    /// `Lifecycle.Deprecated` with its `since` version.
    Deprecated(i32),
}

impl Lifecycle {
    pub fn experimental() -> Lifecycle {
        Lifecycle::Experimental
    }
    pub fn stable() -> Lifecycle {
        Lifecycle::Stable
    }
    pub fn deprecated(since: i32) -> Lifecycle {
        Lifecycle::Deprecated(since)
    }
    /// `Deprecated.since()`.
    pub fn since(&self) -> i32 {
        match self {
            Lifecycle::Deprecated(since) => *since,
            _ => panic!("not a deprecated lifecycle"),
        }
    }
    pub fn add(&self, other: Lifecycle) -> Lifecycle {
        if *self == Lifecycle::Experimental || other == Lifecycle::Experimental {
            return Lifecycle::Experimental;
        }
        if let Lifecycle::Deprecated(since) = *self {
            if let Lifecycle::Deprecated(other_since) = other {
                if other_since < since {
                    return other;
                }
            }
            return *self;
        } else if let Lifecycle::Deprecated(_) = other {
            return other;
        }
        Lifecycle::Stable
    }
    pub fn to_string(&self) -> String {
        match self {
            Lifecycle::Stable => "Stable".to_string(),
            Lifecycle::Experimental => "Experimental".to_string(),
            Lifecycle::Deprecated(since) => format!("Deprecated[{}]", since),
        }
    }
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&Lifecycle::to_string(self))
    }
}
