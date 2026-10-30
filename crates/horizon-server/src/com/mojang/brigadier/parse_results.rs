//! Port of `com.mojang.brigadier.ParseResults` (Brigadier 1.3.11): immutable, a cheap handle.
use crate::com::mojang::brigadier::context::command_context_builder::CommandContextBuilder;
use crate::com::mojang::brigadier::exceptions::command_syntax_exception::CommandSyntaxException;
use crate::com::mojang::brigadier::immutable_string_reader::ImmutableStringReader;
use crate::com::mojang::brigadier::string_reader::StringReader;
use crate::com::mojang::brigadier::tree::command_node::CommandNodeTrait;
use crate::prelude::*;

/// `Map<CommandNode<S>, CommandSyntaxException>` (a `LinkedHashMap` where the dispatcher fills it).
pub type ParseExceptions<S> = IndexMap<ById<dyn CommandNodeTrait<S>>, CommandSyntaxException>;

pub struct ParseResultsData<S: JGeneric> {
    pub context: CommandContextBuilder<S>,
    pub exceptions: ParseExceptions<S>,
    pub reader: ImmutableStringReader,
}

pub struct ParseResults<S: JGeneric>(pub JRef<ParseResultsData<S>>);

impl<S: JGeneric> Clone for ParseResults<S> {
    fn clone(&self) -> Self {
        ParseResults(self.0.clone())
    }
}

impl<S: JGeneric> ParseResults<S> {
    /// `new ParseResults(context, reader, exceptions)`.
    pub fn new_3(context: CommandContextBuilder<S>, reader: ImmutableStringReader, exceptions: ParseExceptions<S>) -> Self {
        ParseResults(Arc::new(ParseResultsData { context, exceptions, reader }))
    }

    /// `new ParseResults(context)`.
    pub fn new(context: CommandContextBuilder<S>) -> Self {
        ParseResults::new_3(context, StringReader::new(""), IndexMap::new())
    }

    pub fn get_context(&self) -> CommandContextBuilder<S> {
        self.0.context.clone()
    }

    pub fn get_reader(&self) -> ImmutableStringReader {
        self.0.reader.clone()
    }

    pub fn get_exceptions(&self) -> &ParseExceptions<S> {
        &self.0.exceptions
    }
}
