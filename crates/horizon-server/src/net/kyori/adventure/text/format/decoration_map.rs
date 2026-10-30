//! Port of `net.kyori.adventure.text.format.DecorationMap`: a style's five decoration states, in
//! `TextDecoration` order.
use crate::prelude::*;
use crate::net::kyori::adventure::text::format::{TextDecoration, TextDecorationState};

pub const DECORATIONS: [TextDecoration; 5] = TextDecoration::VALUES;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DecorationMap(pub [TextDecorationState; 5]);
impl DecorationMap {
    pub const EMPTY: DecorationMap = DecorationMap([TextDecorationState::NOT_SET; 5]);
    pub fn get(&self, decoration: TextDecoration) -> TextDecorationState {
        self.0[decoration as usize]
    }
    pub fn with(&self, decoration: TextDecoration, state: TextDecorationState) -> DecorationMap {
        let mut m = *self;
        m.0[decoration as usize] = state;
        m
    }
    /// `DecorationMap.fromMap`.
    pub fn from_map(decorations: &IndexMap<TextDecoration, TextDecorationState>) -> DecorationMap {
        let mut m = DecorationMap::EMPTY;
        for d in DECORATIONS {
            m.0[d as usize] = decorations.get(&d).copied().unwrap_or(TextDecorationState::NOT_SET);
        }
        m
    }
    /// `DecorationMap.merge(Map, DecorationMap)`: the map's states where set, else the other's.
    pub fn merge(first: &IndexMap<TextDecoration, TextDecorationState>, second: &DecorationMap) -> DecorationMap {
        let mut m = DecorationMap::EMPTY;
        for d in DECORATIONS {
            m.0[d as usize] = first.get(&d).copied().unwrap_or_else(|| second.get(d));
        }
        m
    }
    pub fn to_map(&self) -> IndexMap<TextDecoration, TextDecorationState> {
        DECORATIONS.iter().map(|d| (*d, self.get(*d))).collect()
    }
}
