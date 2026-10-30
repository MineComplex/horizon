//! Port of `io.netty.util.ResourceLeakDetector`: only its level is kept (the port has no leak tracking:
//! Rust frees buffers when their last handle drops).
use crate::prelude::*;

/// `ResourceLeakDetector.Level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceLeakDetectorLevel {
    DISABLED,
    SIMPLE,
    ADVANCED,
    PARANOID,
}

impl ResourceLeakDetectorLevel {
    pub fn values() -> Vec<ResourceLeakDetectorLevel> {
        vec![Self::DISABLED, Self::SIMPLE, Self::ADVANCED, Self::PARANOID]
    }
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::DISABLED => "DISABLED",
            Self::SIMPLE => "SIMPLE",
            Self::ADVANCED => "ADVANCED",
            Self::PARANOID => "PARANOID",
        }
    }
    /// `Level.parseLevel(String)`: by name or ordinal, SIMPLE otherwise.
    pub fn parse_level(level_str: &str) -> ResourceLeakDetectorLevel {
        let trimmed = level_str.trim();
        for l in Self::values() {
            if trimmed.eq_ignore_ascii_case(l.name()) || trimmed == l.ordinal().to_string() {
                return l;
            }
        }
        Self::SIMPLE
    }
}

static LEVEL: LazyLock<Mut<ResourceLeakDetectorLevel>> =
    LazyLock::new(|| Mut::new(ResourceLeakDetectorLevel::SIMPLE));

pub struct ResourceLeakDetector;

impl ResourceLeakDetector {
    pub fn set_level(level: ResourceLeakDetectorLevel) {
        LEVEL.set(level);
    }
    pub fn get_level() -> ResourceLeakDetectorLevel {
        LEVEL.get()
    }
    pub fn is_enabled() -> bool {
        LEVEL.get() != ResourceLeakDetectorLevel::DISABLED
    }
}
