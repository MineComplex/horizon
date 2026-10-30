//! Port of `com.mojang.brigadier.ImmutableStringReader` (Brigadier 1.3.11).
//!
//! Java's interface has one implementation, `StringReader`; the port names that type (a read-only view
//! is a `&StringReader`): `getString`, `getRemainingLength`, `getTotalLength`, `getCursor`, `getRead`,
//! `getRemaining`, `canRead(int)` -> `can_read_1`, `canRead()`, `peek()`, `peek(int)` -> `peek_1`.
pub type ImmutableStringReader = crate::com::mojang::brigadier::string_reader::StringReader;
