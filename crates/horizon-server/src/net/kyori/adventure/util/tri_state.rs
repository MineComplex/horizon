//! Port of `net.kyori.adventure.util.TriState`.

// ---- enum net.kyori.adventure.util.TriState
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Default)]
pub enum TriState {
    #[default]
    NOT_SET,
    FALSE,
    TRUE,
}
impl TriState {
    pub const VALUES: [TriState; 3] = [TriState::NOT_SET, TriState::FALSE, TriState::TRUE];
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            TriState::NOT_SET => "NOT_SET",
            TriState::FALSE => "FALSE",
            TriState::TRUE => "TRUE",
        }
    }
    pub fn value_of(name: &str) -> Option<TriState> {
        Self::VALUES.into_iter().find(|v| v.name() == name)
    }
    pub fn to_boolean(self) -> Option<bool> {
        match self {
            TriState::TRUE => Some(true),
            TriState::FALSE => Some(false),
            TriState::NOT_SET => None,
        }
    }
    pub fn to_boolean_or_else(self, other: bool) -> bool {
        match self {
            TriState::TRUE => true,
            TriState::FALSE => false,
            TriState::NOT_SET => other,
        }
    }
    pub fn to_boolean_or_else_get(self, supplier: impl FnOnce() -> bool) -> bool {
        match self {
            TriState::TRUE => true,
            TriState::FALSE => false,
            TriState::NOT_SET => supplier(),
        }
    }
    /// `byBoolean(boolean)`.
    pub fn by_boolean(value: bool) -> TriState {
        if value { TriState::TRUE } else { TriState::FALSE }
    }
    /// `byBoolean(Boolean)`.
    pub fn by_boolean_1(value: Option<bool>) -> TriState {
        match value {
            None => TriState::NOT_SET,
            Some(v) => TriState::by_boolean(v),
        }
    }
}
