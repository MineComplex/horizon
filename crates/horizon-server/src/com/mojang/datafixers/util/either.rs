//! Port of `com.mojang.datafixers.util.Either` (leaf-server/src/minecraft/java/com/mojang/datafixers/util/Either.java).
//!
//! Java's abstract class with its two private subclasses `Left`/`Right` is a value enum here:
//! `Either::Left(a)` / `Either::Right(b)` (Java's `Either.left(a)` / `Either.right(b)` are also
//! `Either::left_1(a)` / `Either::right_1(b)`). Paper's cached `Optional` fields are not needed: `left()`
//! and `right()` return `Option`s directly. The `Instance` applicative/traversable (kinds) is not ported
//! (DFU's datafixers half is left out, PORTING.md).
use crate::prelude::*;

// ---- class com.mojang.datafixers.util.Either (Either.java:17)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    /// `Either.left(value)`.
    pub fn left_1(value: L) -> Self {
        Either::Left(value)
    }
    /// `Either.right(value)`.
    pub fn right_1(value: R) -> Self {
        Either::Right(value)
    }
    pub fn map_both<C, D>(self, f1: impl FnOnce(L) -> C, f2: impl FnOnce(R) -> D) -> Either<C, D> {
        match self {
            Either::Left(v) => Either::Left(f1(v)),
            Either::Right(v) => Either::Right(f2(v)),
        }
    }
    pub fn map<T>(self, l: impl FnOnce(L) -> T, r: impl FnOnce(R) -> T) -> T {
        match self {
            Either::Left(v) => l(v),
            Either::Right(v) => r(v),
        }
    }
    pub fn if_left(self, consumer: impl FnOnce(&L)) -> Self {
        if let Either::Left(v) = &self {
            consumer(v);
        }
        self
    }
    pub fn if_right(self, consumer: impl FnOnce(&R)) -> Self {
        if let Either::Right(v) = &self {
            consumer(v);
        }
        self
    }
    pub fn left(&self) -> Option<L>
    where
        L: Clone,
    {
        match self {
            Either::Left(v) => Some(v.clone()),
            Either::Right(_) => None,
        }
    }
    pub fn right(&self) -> Option<R>
    where
        R: Clone,
    {
        match self {
            Either::Left(_) => None,
            Either::Right(v) => Some(v.clone()),
        }
    }
    pub fn is_left(&self) -> bool {
        matches!(self, Either::Left(_))
    }
    pub fn is_right(&self) -> bool {
        matches!(self, Either::Right(_))
    }
    pub fn as_ref(&self) -> Either<&L, &R> {
        match self {
            Either::Left(v) => Either::Left(v),
            Either::Right(v) => Either::Right(v),
        }
    }
    pub fn map_left<T>(self, l: impl FnOnce(L) -> T) -> Either<T, R> {
        self.map(|t| Either::Left(l(t)), Either::Right)
    }
    pub fn map_right<T>(self, l: impl FnOnce(R) -> T) -> Either<L, T> {
        self.map(Either::Left, |t| Either::Right(l(t)))
    }
    /// `orThrow()`: the left value, or a `RuntimeException` (a panic) carrying the right one.
    pub fn or_throw(self) -> L
    where
        R: std::fmt::Debug,
    {
        self.map(|l| l, |r| panic!("{:?}", r))
    }
    pub fn swap(self) -> Either<R, L> {
        self.map(Either::Right, Either::Left)
    }
    pub fn flat_map<L2>(self, function: impl FnOnce(L) -> Either<L2, R>) -> Either<L2, R> {
        self.map(function, Either::Right)
    }
}

impl<U> Either<U, U> {
    /// `Either.unwrap(either)`.
    pub fn unwrap(either: Either<U, U>) -> U {
        either.map(|l| l, |r| r)
    }
}

impl<L: std::fmt::Debug, R: std::fmt::Debug> std::fmt::Debug for Either<L, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Either::Left(v) => write!(f, "Left[{:?}]", v),
            Either::Right(v) => write!(f, "Right[{:?}]", v),
        }
    }
}

impl<L: std::fmt::Display, R: std::fmt::Display> std::fmt::Display for Either<L, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Either::Left(v) => write!(f, "Left[{}]", v),
            Either::Right(v) => write!(f, "Right[{}]", v),
        }
    }
}
