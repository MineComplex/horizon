//! Commands as Minecraft parses them: Brigadier's behaviour (the library vanilla and Leaf use) in a
//! native design.
//!
//! - The tree is one arena, [`CommandTree`]: nodes in a `Vec`, children and redirects as [`NodeId`]s.
//!   Brigadier's `CommandNode` objects with parent/child references, shared and locked, are not
//!   reproduced.
//! - What Brigadier stores as lambdas (the command to run, a node's requirement, a redirect modifier, a
//!   custom suggestion provider) is a key of the server's choosing, `K` (an enum naming them). The
//!   server matches on the key where it runs the command, checks a permission or lists suggestions.
//!   So the tree holds no closures and is plain data: `Send + Sync` without locks, cheap to read from
//!   any thread, and the tree the commands packet describes.
//! - Argument types and parsed values are enums ([`ArgumentType`], [`ArgumentValue`]), not
//!   `Arc<dyn Any>` cast back at run time.
//! - Parsing borrows the tree and the input; contexts hold node ids and `&str` names.
//! - Positions are byte offsets on `char` boundaries. Brigadier counts UTF-16 units: convert with
//!   [`utf16_offset`] where a position leaves the server (suggestion ranges, error cursors).
//!
//! Running a parsed command is the server's part, as in vanilla: [`ParseResults::context_chain`] gives
//! the modifier stages and the executable context (`Commands.finishParsing` + `ContextChain.tryFlatten`),
//! and the server's executor runs them (vanilla's `net.minecraft.commands.execution`).
//!
//! The parsing core is Horizon's own Brigadier (`crates/horizon-command`), which Horizon's parity
//! harnesses check against vanilla; it is copied here (this crate depends on no other Horizon crate),
//! made generic over the key type, and given Brigadier's redirect modifiers, forks, node merging and
//! ambiguity search.

mod argument;
mod coordinates;
mod error;
pub mod exec;
pub mod network;
mod reader;
mod suggestion;
mod tree;
mod values;

pub use argument::{
    ArgumentType, ArgumentValue, ProfileTarget, ResourceRegistry, SelectorKind, StringKind,
};
pub use coordinates::{CommandOrigin, Coordinates, WorldCoordinate, local_position};
pub use error::CommandError;
pub use reader::StringReader;
pub use suggestion::{Suggestion, Suggestions, SuggestionsBuilder, compare_strings_ignore_case};
pub use tree::{
    CommandContext, CommandTree, ContextChain, Node, NodeBuilder, NodeId, NodeKind, ParseResults,
    ParsedArgument, ParsedNode, argument, literal,
};
pub use values::{Angle, DoubleRange, IntRange, ScoreOperation, Swizzle, TeamColor, wrap_degrees};

/// A range of the input, in bytes (`StringRange`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StringRange {
    pub start: usize,
    pub end: usize,
}

impl StringRange {
    pub const fn at(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    pub const fn between(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// `StringRange.encompassing`.
    pub fn encompassing(a: Self, b: Self) -> Self {
        Self {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn get<'a>(&self, input: &'a str) -> &'a str {
        &input[self.start..self.end]
    }

    /// The same range in UTF-16 units, as Java and the protocol count.
    pub fn to_utf16(&self, input: &str) -> (usize, usize) {
        (
            utf16_offset(input, self.start),
            utf16_offset(input, self.end),
        )
    }
}

/// The UTF-16 offset of byte offset `byte` of `input` (clamped to the input).
pub fn utf16_offset(input: &str, byte: usize) -> usize {
    let byte = byte.min(input.len());
    input[..byte].chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests;
