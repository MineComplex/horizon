//! Port of `com.mojang.datafixers.util.Pair` (DFU). A value type: `Pair::of(a, b)`.
use crate::prelude::*;

// ---- class com.mojang.datafixers.util.Pair
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pair<F, S> {
    pub first: F,
    pub second: S,
}

impl<F, S> Pair<F, S> {
    pub fn new(first: F, second: S) -> Self {
        Pair { first, second }
    }
    pub fn of(first: F, second: S) -> Self {
        Pair { first, second }
    }
    pub fn get_first(&self) -> F
    where
        F: Clone,
    {
        self.first.clone()
    }
    pub fn get_second(&self) -> S
    where
        S: Clone,
    {
        self.second.clone()
    }
    pub fn swap(self) -> Pair<S, F> {
        Pair::of(self.second, self.first)
    }
    pub fn map_first<F2>(self, function: impl FnOnce(F) -> F2) -> Pair<F2, S> {
        Pair::of(function(self.first), self.second)
    }
    pub fn map_second<S2>(self, function: impl FnOnce(S) -> S2) -> Pair<F, S2> {
        Pair::of(self.first, function(self.second))
    }
    pub fn into_tuple(self) -> (F, S) {
        (self.first, self.second)
    }
    /// `Pair.toMap()`: collects pairs into a map (Java's `Collectors.toMap` panics on a duplicate key).
    pub fn to_map<I: IntoIterator<Item = Pair<F, S>>>(pairs: I) -> HashMap<F, S>
    where
        F: std::hash::Hash + Eq + std::fmt::Debug,
    {
        let mut map = HashMap::new();
        for p in pairs {
            if map.contains_key(&p.first) {
                panic!("Duplicate key {:?}", p.first);
            }
            map.insert(p.first, p.second);
        }
        map
    }
}

impl<F: std::fmt::Debug, S: std::fmt::Debug> std::fmt::Debug for Pair<F, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?}, {:?})", self.first, self.second)
    }
}

impl<F: std::fmt::Display, S: std::fmt::Display> std::fmt::Display for Pair<F, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.first, self.second)
    }
}

impl<F, S> From<(F, S)> for Pair<F, S> {
    fn from((first, second): (F, S)) -> Self {
        Pair { first, second }
    }
}
