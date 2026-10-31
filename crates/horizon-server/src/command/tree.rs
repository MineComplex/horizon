//! The command tree, parsing, suggestions and usage text (`CommandDispatcher`, `CommandNode`,
//! `CommandContextBuilder`, `ContextChain`).

use uuid::Uuid;

use super::{
    Angle, ArgumentType, ArgumentValue, CommandError, Coordinates, DoubleRange, IntRange,
    ProfileTarget, ScoreOperation, StringRange, StringReader, Suggestions, SuggestionsBuilder,
    Swizzle, TeamColor,
};

/// A node of a [`CommandTree`]. Ids are indexes; nodes are never removed, so an id stays valid for the
/// tree that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The id of the node at `index` of [`CommandTree::nodes`].
    pub(super) fn from_index(index: usize) -> Self {
        Self(index as u32)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Root,
    Literal(Box<str>),
    Argument { name: Box<str>, ty: ArgumentType },
}

/// `CommandNode`. `K` is the server's key for commands, requirements, redirect modifiers and suggestion
/// providers (see the module docs).
#[derive(Clone, Debug)]
pub struct Node<K> {
    kind: NodeKind,
    /// Every child in the order added (`children`), and the same split by kind (`literals`,
    /// `arguments`), as Brigadier keeps them.
    children: Vec<NodeId>,
    literals: Vec<NodeId>,
    arguments: Vec<NodeId>,
    command: Option<K>,
    requirement: Option<K>,
    redirect: Option<NodeId>,
    modifier: Option<K>,
    forks: bool,
    suggestions: Option<K>,
}

impl<K: Copy> Node<K> {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            children: Vec::new(),
            literals: Vec::new(),
            arguments: Vec::new(),
            command: None,
            requirement: None,
            redirect: None,
            modifier: None,
            forks: false,
            suggestions: None,
        }
    }

    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// `getName`: the literal, the argument's name, or `""` for the root.
    pub fn name(&self) -> &str {
        match &self.kind {
            NodeKind::Root => "",
            NodeKind::Literal(literal) => literal,
            NodeKind::Argument { name, .. } => name,
        }
    }

    /// `getUsageText`: `literal` or `<name>`.
    pub fn usage_text(&self) -> String {
        match &self.kind {
            NodeKind::Root => String::new(),
            NodeKind::Literal(literal) => literal.to_string(),
            NodeKind::Argument { name, .. } => format!("<{name}>"),
        }
    }

    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    /// The command run when parsing ends on this node.
    pub fn command(&self) -> Option<K> {
        self.command
    }

    /// What a source needs to use this node; `None` lets everyone.
    pub fn requirement(&self) -> Option<K> {
        self.requirement
    }

    pub fn redirect(&self) -> Option<NodeId> {
        self.redirect
    }

    /// The redirect modifier, which turns the source into the sources the redirect continues with.
    pub fn modifier(&self) -> Option<K> {
        self.modifier
    }

    pub fn forks(&self) -> bool {
        self.forks
    }

    /// An argument's custom suggestion provider, used instead of its type's suggestions.
    pub fn custom_suggestions(&self) -> Option<K> {
        self.suggestions
    }

    fn can_use(&self, can_use: &impl Fn(K) -> bool) -> bool {
        self.requirement.is_none_or(can_use)
    }

    /// `isValidInput`.
    fn is_valid_input(&self, input: &str) -> bool {
        match &self.kind {
            NodeKind::Root => false,
            NodeKind::Literal(literal) => {
                parse_literal(literal, &mut StringReader::new(input)).is_some()
            }
            NodeKind::Argument { ty, .. } => {
                let mut reader = StringReader::new(input);
                ty.parse(&mut reader).is_ok() && (!reader.can_read() || reader.peek() == ' ')
            }
        }
    }
}

/// `LiteralCommandNode.parse`: the end of the literal when the input continues with it as a whole word.
fn parse_literal(literal: &str, reader: &mut StringReader) -> Option<usize> {
    let start = reader.cursor();
    if reader.remaining().starts_with(literal) {
        let end = start + literal.len();
        reader.set_cursor(end);
        if !reader.can_read() || reader.peek() == ' ' {
            return Some(end);
        }
        reader.set_cursor(start);
    }
    None
}

/// `ArgumentBuilder`: a node and its subtree, before it joins a tree.
#[derive(Clone, Debug)]
pub struct NodeBuilder<K> {
    kind: NodeKind,
    children: Vec<NodeBuilder<K>>,
    command: Option<K>,
    requirement: Option<K>,
    redirect: Option<NodeId>,
    modifier: Option<K>,
    forks: bool,
    suggestions: Option<K>,
}

/// `LiteralArgumentBuilder.literal`.
pub fn literal<K>(name: &str) -> NodeBuilder<K> {
    NodeBuilder::new(NodeKind::Literal(name.into()))
}

/// `RequiredArgumentBuilder.argument`.
pub fn argument<K>(name: &str, ty: ArgumentType) -> NodeBuilder<K> {
    NodeBuilder::new(NodeKind::Argument {
        name: name.into(),
        ty,
    })
}

impl<K> NodeBuilder<K> {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            children: Vec::new(),
            command: None,
            requirement: None,
            redirect: None,
            modifier: None,
            forks: false,
            suggestions: None,
        }
    }

    /// `then`. Panics on a redirected node, as Brigadier throws.
    pub fn then(mut self, child: NodeBuilder<K>) -> Self {
        assert!(
            self.redirect.is_none(),
            "Cannot add children to a redirected node"
        );
        self.children.push(child);
        self
    }

    pub fn executes(mut self, command: K) -> Self {
        self.command = Some(command);
        self
    }

    pub fn requires(mut self, requirement: K) -> Self {
        self.requirement = Some(requirement);
        self
    }

    /// `redirect(target)`: continue parsing at `target` with the same source.
    pub fn redirect(self, target: NodeId) -> Self {
        self.forward(target, None, false)
    }

    /// `redirect(target, modifier)`: continue at `target` with the source `modifier` makes.
    pub fn redirect_with(self, target: NodeId, modifier: K) -> Self {
        self.forward(target, Some(modifier), false)
    }

    /// `fork(target, modifier)`: continue at `target` once for each source `modifier` makes.
    pub fn fork(self, target: NodeId, modifier: K) -> Self {
        self.forward(target, Some(modifier), true)
    }

    /// `forward`. Panics on a node with children, as Brigadier throws.
    pub fn forward(mut self, target: NodeId, modifier: Option<K>, fork: bool) -> Self {
        assert!(
            self.children.is_empty(),
            "Cannot forward a node with children"
        );
        self.redirect = Some(target);
        self.modifier = modifier;
        self.forks = fork;
        self
    }

    /// `RequiredArgumentBuilder.suggests`. Panics on a literal, which has no such method.
    pub fn suggests(mut self, provider: K) -> Self {
        assert!(
            matches!(self.kind, NodeKind::Argument { .. }),
            "only arguments take suggestion providers"
        );
        self.suggestions = Some(provider);
        self
    }
}

/// `CommandDispatcher`: the whole tree, root at [`NodeId::ROOT`].
#[derive(Clone, Debug)]
pub struct CommandTree<K> {
    nodes: Vec<Node<K>>,
}

impl<K: Copy> Default for CommandTree<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Copy> CommandTree<K> {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::new(NodeKind::Root)],
        }
    }

    pub fn node(&self, id: NodeId) -> &Node<K> {
        &self.nodes[id.index()]
    }

    /// Every node, indexed by [`NodeId::index`].
    pub fn nodes(&self) -> &[Node<K>] {
        &self.nodes
    }

    /// `register`: add a command under the root; returns its node.
    pub fn register(&mut self, command: NodeBuilder<K>) -> NodeId {
        self.add_child(NodeId::ROOT, command)
    }

    /// `CommandNode.addChild`. A child with the same name is merged into: it takes the new command, if
    /// any, and the new children (merged the same way); the rest of the new node is dropped.
    pub fn add_child(&mut self, parent: NodeId, builder: NodeBuilder<K>) -> NodeId {
        let NodeBuilder {
            kind,
            children,
            command,
            requirement,
            redirect,
            modifier,
            forks,
            suggestions,
        } = builder;
        let name = match &kind {
            NodeKind::Root => {
                panic!("Cannot add a RootCommandNode as a child to any other CommandNode")
            }
            NodeKind::Literal(name) | NodeKind::Argument { name, .. } => name,
        };
        let id = match self.child(parent, name) {
            Some(existing) => {
                if command.is_some() {
                    self.nodes[existing.index()].command = command;
                }
                existing
            }
            None => {
                let id =
                    NodeId(u32::try_from(self.nodes.len()).expect("fewer than 2^32 command nodes"));
                let is_literal = matches!(kind, NodeKind::Literal(_));
                self.nodes.push(Node {
                    command,
                    requirement,
                    redirect,
                    modifier,
                    forks,
                    suggestions,
                    ..Node::new(kind)
                });
                let parent = &mut self.nodes[parent.index()];
                parent.children.push(id);
                if is_literal {
                    &mut parent.literals
                } else {
                    &mut parent.arguments
                }
                .push(id);
                id
            }
        };
        for child in children {
            self.add_child(id, child);
        }
        id
    }

    /// `getChild`.
    pub fn child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        self.node(parent)
            .children
            .iter()
            .copied()
            .find(|&c| self.node(c).name() == name)
    }

    /// `findNode`: the node at this path of names from the root.
    pub fn find_node<'p>(&self, path: impl IntoIterator<Item = &'p str>) -> Option<NodeId> {
        path.into_iter()
            .try_fold(NodeId::ROOT, |node, name| self.child(node, name))
    }

    /// `getPath`: the names from the root to `target` (the first path in depth-first order; redirects
    /// are not followed). Empty when `target` is the root or not reachable.
    pub fn path(&self, target: NodeId) -> Vec<&str> {
        fn walk<'t, K: Copy>(
            tree: &'t CommandTree<K>,
            node: NodeId,
            target: NodeId,
            path: &mut Vec<&'t str>,
        ) -> bool {
            if node == target {
                return true;
            }
            for &child in &tree.node(node).children {
                path.push(tree.node(child).name());
                if walk(tree, child, target, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        walk(self, NodeId::ROOT, target, &mut path);
        path
    }

    /// `getRelevantNodes`: the literal the next word names, else every argument child. When a node has
    /// literals and none matches, its literals are not tried at all (so they report no errors).
    fn relevant_nodes(&self, node: NodeId, input: &StringReader) -> &[NodeId] {
        let node = self.node(node);
        if node.literals.is_empty() {
            return &node.arguments;
        }
        let word = input.remaining().split(' ').next().unwrap_or("");
        match node
            .literals
            .iter()
            .position(|&l| self.node(l).name() == word)
        {
            Some(i) => &node.literals[i..=i],
            None => &node.arguments,
        }
    }

    /// `parse`: parse `input` from the start for a source that may use the nodes `can_use` accepts.
    pub fn parse<'a>(&'a self, input: &'a str, can_use: impl Fn(K) -> bool) -> ParseResults<'a, K> {
        self.parse_at(input, 0, can_use)
    }

    /// `parse(StringReader)`: parse from byte `start` (after a leading `/`).
    pub fn parse_at<'a>(
        &'a self,
        input: &'a str,
        start: usize,
        can_use: impl Fn(K) -> bool,
    ) -> ParseResults<'a, K> {
        let context = CommandContext::new(NodeId::ROOT, start);
        self.parse_nodes(
            NodeId::ROOT,
            StringReader::at(input, start),
            &context,
            &can_use,
        )
    }

    /// `parseNodes`: try every relevant child; keep the first result that read the whole input, else
    /// the first without errors, else the first.
    fn parse_nodes<'a>(
        &'a self,
        node: NodeId,
        original: StringReader<'a>,
        so_far: &CommandContext<'a, K>,
        can_use: &impl Fn(K) -> bool,
    ) -> ParseResults<'a, K> {
        let mut errors = Vec::new();
        let mut potentials: Vec<ParseResults<'a, K>> = Vec::new();
        for &child in self.relevant_nodes(node, &original) {
            let child_node = self.node(child);
            if !child_node.can_use(can_use) {
                continue;
            }
            let mut context = so_far.clone();
            let mut reader = original;
            let parsed = self
                .parse_node(child, &mut reader, &mut context)
                .and_then(|()| {
                    if reader.can_read() && reader.peek() != ' ' {
                        Err(CommandError::dispatcher_expected_argument_separator().at(&reader))
                    } else {
                        Ok(())
                    }
                });
            if let Err(error) = parsed {
                errors.push((child, error));
                continue;
            }
            context.command = child_node.command;
            let redirect = child_node.redirect;
            if reader.can_read_n(if redirect.is_none() { 2 } else { 1 }) {
                reader.skip();
                if let Some(target) = redirect {
                    let child_context = CommandContext::new(target, reader.cursor());
                    let parse = self.parse_nodes(target, reader, &child_context, can_use);
                    context.child = Some(Box::new(parse.context));
                    return ParseResults {
                        context,
                        input: parse.input,
                        cursor: parse.cursor,
                        errors: parse.errors,
                    };
                }
                potentials.push(self.parse_nodes(child, reader, &context, can_use));
            } else {
                potentials.push(ParseResults {
                    context,
                    input: reader.string(),
                    cursor: reader.cursor(),
                    errors: Vec::new(),
                });
            }
        }
        // `min_by_key` keeps the first of equals, like Brigadier's stable sort then `get(0)`.
        if let Some(best) = potentials
            .into_iter()
            .min_by_key(|p| (p.can_read(), !p.errors.is_empty()))
        {
            return best;
        }
        ParseResults {
            context: so_far.clone(),
            input: original.string(),
            cursor: original.cursor(),
            errors,
        }
    }

    /// `CommandNode.parse` for one child.
    fn parse_node<'a>(
        &'a self,
        id: NodeId,
        reader: &mut StringReader<'a>,
        context: &mut CommandContext<'a, K>,
    ) -> Result<(), CommandError> {
        let node = self.node(id);
        let start = reader.cursor();
        match &node.kind {
            NodeKind::Root => Ok(()),
            NodeKind::Literal(literal) => match parse_literal(literal, reader) {
                Some(end) => {
                    context.with_node(node, id, StringRange::between(start, end));
                    Ok(())
                }
                None => Err(CommandError::literal_incorrect(literal).at(reader)),
            },
            NodeKind::Argument { name, ty } => {
                let value = ty.parse(reader)?;
                let range = StringRange::between(start, reader.cursor());
                context.with_argument(ParsedArgument { name, range, value });
                context.with_node(node, id, range);
                Ok(())
            }
        }
    }

    /// `getCompletionSuggestions`: completions for `parse`'s input at byte `cursor`. Every child of the
    /// node before the cursor is asked, whatever its requirement (as in Brigadier; the tree a client
    /// gets is already pruned to what it may use). `provide` answers custom suggestion providers; a
    /// provider's error drops its suggestions, as Brigadier drops a failed future.
    pub fn completion_suggestions<F>(
        &self,
        parse: &ParseResults<'_, K>,
        cursor: usize,
        mut provide: F,
    ) -> Suggestions
    where
        F: FnMut(K, &CommandContext<'_, K>, &mut SuggestionsBuilder) -> Result<(), CommandError>,
    {
        let full = parse.input;
        let (Some(truncated), Some((parent, start))) = (
            full.get(..cursor),
            parse.context.find_suggestion_context(cursor),
        ) else {
            return Suggestions::default();
        };
        let start = start.min(cursor);
        let children = &self.node(parent).children;
        let mut all = Vec::with_capacity(children.len());
        for &child in children {
            let node = self.node(child);
            let mut builder = SuggestionsBuilder::new(truncated, start);
            match &node.kind {
                NodeKind::Root => {}
                NodeKind::Literal(literal) => {
                    if literal
                        .to_lowercase()
                        .starts_with(builder.remaining_lowercase())
                    {
                        builder.suggest(&**literal);
                    }
                }
                NodeKind::Argument { ty, .. } => match node.suggestions {
                    Some(provider) => {
                        if provide(provider, &parse.context, &mut builder).is_err() {
                            all.push(Suggestions::default());
                            continue;
                        }
                    }
                    None => ty.list_suggestions(&mut builder),
                },
            }
            all.push(builder.build());
        }
        Suggestions::merge(full, all)
    }

    /// `getAllUsage`: every command under `node` as a line, prefixed by the path below `node`. With
    /// `restricted`, nodes `can_use` refuses are left out.
    pub fn all_usage(
        &self,
        node: NodeId,
        can_use: impl Fn(K) -> bool,
        restricted: bool,
    ) -> Vec<String> {
        let mut result = Vec::new();
        self.all_usage_into(node, &can_use, restricted, String::new(), &mut result);
        result
    }

    fn all_usage_into(
        &self,
        id: NodeId,
        can_use: &impl Fn(K) -> bool,
        restricted: bool,
        prefix: String,
        result: &mut Vec<String>,
    ) {
        let node = self.node(id);
        if restricted && !node.can_use(can_use) {
            return;
        }
        if node.command.is_some() {
            result.push(prefix.clone());
        }
        if let Some(redirect) = node.redirect {
            let target = self.redirect_text(redirect);
            result.push(if prefix.is_empty() {
                format!("{} {target}", node.usage_text())
            } else {
                format!("{prefix} {target}")
            });
        } else {
            for &child in &node.children {
                let usage = self.node(child).usage_text();
                let prefix = if prefix.is_empty() {
                    usage
                } else {
                    format!("{prefix} {usage}")
                };
                self.all_usage_into(child, can_use, restricted, prefix, result);
            }
        }
    }

    fn redirect_text(&self, redirect: NodeId) -> String {
        if redirect == NodeId::ROOT {
            "...".to_owned()
        } else {
            format!("-> {}", self.node(redirect).usage_text())
        }
    }

    /// `getSmartUsage`: one condensed line per child of `node` the source may use (`/help`).
    pub fn smart_usage(&self, node: NodeId, can_use: impl Fn(K) -> bool) -> Vec<(NodeId, String)> {
        let optional = self.node(node).command.is_some();
        self.node(node)
            .children
            .iter()
            .filter_map(|&child| {
                self.smart_usage_of(child, &can_use, optional, false)
                    .map(|u| (child, u))
            })
            .collect()
    }

    fn smart_usage_of(
        &self,
        id: NodeId,
        can_use: &impl Fn(K) -> bool,
        optional: bool,
        deep: bool,
    ) -> Option<String> {
        let node = self.node(id);
        if !node.can_use(can_use) {
            return None;
        }
        let this = if optional {
            format!("[{}]", node.usage_text())
        } else {
            node.usage_text()
        };
        let child_optional = node.command.is_some();
        let (open, close) = if child_optional {
            ("[", "]")
        } else {
            ("(", ")")
        };
        if deep {
            return Some(this);
        }
        if let Some(redirect) = node.redirect {
            return Some(format!("{this} {}", self.redirect_text(redirect)));
        }
        let children: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|&c| self.node(c).can_use(can_use))
            .collect();
        if let [only] = children[..] {
            if let Some(usage) = self.smart_usage_of(only, can_use, child_optional, child_optional)
            {
                return Some(format!("{this} {usage}"));
            }
        } else if children.len() > 1 {
            // A `LinkedHashSet`: distinct usages in order.
            let mut child_usage: Vec<String> = Vec::new();
            for &child in &children {
                if let Some(usage) = self.smart_usage_of(child, can_use, child_optional, true)
                    && !child_usage.contains(&usage)
                {
                    child_usage.push(usage);
                }
            }
            match &child_usage[..] {
                [usage] => {
                    let usage = if child_optional {
                        format!("[{usage}]")
                    } else {
                        usage.clone()
                    };
                    return Some(format!("{this} {usage}"));
                }
                [_, _, ..] => {
                    let alternatives: Vec<String> = children
                        .iter()
                        .map(|&c| self.node(c).usage_text())
                        .collect();
                    return Some(format!("{this} {open}{}{close}", alternatives.join("|")));
                }
                [] => {}
            }
        }
        Some(this)
    }

    /// `findAmbiguities`: for each pair of siblings, the examples of one that the other also accepts.
    /// `report(parent, child, sibling, inputs)`; vanilla runs it in development builds to catch trees
    /// whose parse depends on child order.
    pub fn find_ambiguities(&self, mut report: impl FnMut(NodeId, NodeId, NodeId, &[&str])) {
        self.find_ambiguities_under(NodeId::ROOT, &mut report);
    }

    fn find_ambiguities_under(
        &self,
        parent: NodeId,
        report: &mut impl FnMut(NodeId, NodeId, NodeId, &[&str]),
    ) {
        let children = &self.node(parent).children;
        for &child in children {
            for &sibling in children {
                if child == sibling {
                    continue;
                }
                let mut matches: Vec<&str> = Vec::new();
                for &input in self.examples(child).iter() {
                    if self.node(sibling).is_valid_input(input) && !matches.contains(&input) {
                        matches.push(input);
                    }
                }
                if !matches.is_empty() {
                    report(parent, child, sibling, &matches);
                }
            }
            self.find_ambiguities_under(child, report);
        }
    }

    /// `getExamples`.
    fn examples(&self, id: NodeId) -> Vec<&str> {
        match &self.node(id).kind {
            NodeKind::Literal(literal) => vec![&**literal],
            NodeKind::Argument { ty, .. } => ty.examples().to_vec(),
            NodeKind::Root => Vec::new(),
        }
    }
}

/// A node parsed and the input it covered (`ParsedCommandNode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedNode {
    pub node: NodeId,
    pub range: StringRange,
}

/// An argument parsed (`ParsedArgument` with its name).
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedArgument<'a> {
    pub name: &'a str,
    pub range: StringRange,
    pub value: ArgumentValue,
}

/// `CommandContextBuilder`/`CommandContext`: what one stage of a command parsed. A redirect starts a
/// new stage, the `child`.
#[derive(Clone, Debug)]
pub struct CommandContext<'a, K> {
    root: NodeId,
    nodes: Vec<ParsedNode>,
    arguments: Vec<ParsedArgument<'a>>,
    command: Option<K>,
    modifier: Option<K>,
    forks: bool,
    range: StringRange,
    child: Option<Box<CommandContext<'a, K>>>,
}

impl<'a, K: Copy> CommandContext<'a, K> {
    fn new(root: NodeId, start: usize) -> Self {
        Self {
            root,
            nodes: Vec::new(),
            arguments: Vec::new(),
            command: None,
            modifier: None,
            forks: false,
            range: StringRange::at(start),
            child: None,
        }
    }

    /// `withNode`: the stage's modifier and forking come from its last node.
    fn with_node(&mut self, node: &Node<K>, id: NodeId, range: StringRange) {
        self.nodes.push(ParsedNode { node: id, range });
        self.range = StringRange::encompassing(self.range, range);
        self.modifier = node.modifier;
        self.forks = node.forks;
    }

    /// `withArgument` (a `LinkedHashMap.put`: a repeated name replaces the value in place).
    fn with_argument(&mut self, argument: ParsedArgument<'a>) {
        match self.arguments.iter_mut().find(|a| a.name == argument.name) {
            Some(existing) => *existing = argument,
            None => self.arguments.push(argument),
        }
    }

    /// The node this stage started from (the root, or a redirect's target).
    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn nodes(&self) -> &[ParsedNode] {
        &self.nodes
    }

    pub fn arguments(&self) -> &[ParsedArgument<'a>] {
        &self.arguments
    }

    pub fn command(&self) -> Option<K> {
        self.command
    }

    pub fn modifier(&self) -> Option<K> {
        self.modifier
    }

    pub fn is_forked(&self) -> bool {
        self.forks
    }

    pub fn range(&self) -> StringRange {
        self.range
    }

    pub fn child(&self) -> Option<&CommandContext<'a, K>> {
        self.child.as_deref()
    }

    /// `getLastChild`.
    pub fn last_child(&self) -> &CommandContext<'a, K> {
        let mut context = self;
        while let Some(child) = &context.child {
            context = child;
        }
        context
    }

    /// `getArgument(name)`; `None` where Brigadier throws "No such argument".
    pub fn argument(&self, name: &str) -> Option<&ArgumentValue> {
        self.arguments
            .iter()
            .find(|a| a.name == name)
            .map(|a| &a.value)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.argument(name)? {
            ArgumentValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn integer(&self, name: &str) -> Option<i32> {
        match self.argument(name)? {
            ArgumentValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    pub fn long(&self, name: &str) -> Option<i64> {
        match self.argument(name)? {
            ArgumentValue::Long(v) => Some(*v),
            _ => None,
        }
    }

    pub fn float(&self, name: &str) -> Option<f32> {
        match self.argument(name)? {
            ArgumentValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn double(&self, name: &str) -> Option<f64> {
        match self.argument(name)? {
            ArgumentValue::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// A `resource_or_tag` or `resource_or_tag_key` argument: whether it is a tag and its identifier.
    pub fn resource(&self, name: &str) -> Option<(bool, &str)> {
        match self.argument(name)? {
            ArgumentValue::Resource { tag, id } => Some((*tag, id)),
            _ => None,
        }
    }

    /// `IdentifierArgument.getId`, written `namespace:path`.
    pub fn identifier(&self, name: &str) -> Option<&str> {
        self.string(name)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        match self.argument(name)? {
            ArgumentValue::String(v) => Some(v),
            _ => None,
        }
    }

    /// A `block_pos`, `column_pos`, `vec3`, `vec2` or `rotation` argument, unresolved
    /// (`Vec3Argument.getCoordinates`); [`Coordinates`] resolves it against a source.
    pub fn coordinates(&self, name: &str) -> Option<Coordinates> {
        match self.argument(name)? {
            ArgumentValue::Coordinates(v) => Some(*v),
            _ => None,
        }
    }

    /// An `angle` argument; [`Angle::resolve`] gives the degrees.
    pub fn angle(&self, name: &str) -> Option<Angle> {
        match self.argument(name)? {
            ArgumentValue::Angle(v) => Some(*v),
            _ => None,
        }
    }

    pub fn swizzle(&self, name: &str) -> Option<Swizzle> {
        match self.argument(name)? {
            ArgumentValue::Swizzle(v) => Some(*v),
            _ => None,
        }
    }

    pub fn int_range(&self, name: &str) -> Option<IntRange> {
        match self.argument(name)? {
            ArgumentValue::IntRange(v) => Some(*v),
            _ => None,
        }
    }

    pub fn float_range(&self, name: &str) -> Option<DoubleRange> {
        match self.argument(name)? {
            ArgumentValue::FloatRange(v) => Some(*v),
            _ => None,
        }
    }

    /// A `time` argument, in ticks (stored as an integer, as vanilla does).
    pub fn time(&self, name: &str) -> Option<i32> {
        self.integer(name)
    }

    pub fn uuid(&self, name: &str) -> Option<Uuid> {
        match self.argument(name)? {
            ArgumentValue::Uuid(v) => Some(*v),
            _ => None,
        }
    }

    pub fn operation(&self, name: &str) -> Option<ScoreOperation> {
        match self.argument(name)? {
            ArgumentValue::Operation(v) => Some(*v),
            _ => None,
        }
    }

    /// `HexColorArgument.getHexColor`: the opaque ARGB colour (`0xFFRRGGBB` as an `i32`).
    pub fn hex_color(&self, name: &str) -> Option<i32> {
        self.integer(name)
    }

    /// `GameProfileArgument`'s result.
    pub fn profile(&self, name: &str) -> Option<&ProfileTarget> {
        match self.argument(name)? {
            ArgumentValue::GameProfile(v) => Some(v),
            _ => None,
        }
    }

    pub fn team_color(&self, name: &str) -> Option<TeamColor> {
        match self.argument(name)? {
            ArgumentValue::TeamColor(v) => Some(*v),
            _ => None,
        }
    }

    /// `findSuggestionContext`: the node whose children complete the text at `cursor`, and where that
    /// text starts. `None` for a cursor before this context (Brigadier throws).
    pub fn find_suggestion_context(&self, cursor: usize) -> Option<(NodeId, usize)> {
        if self.range.start > cursor {
            return None;
        }
        if self.range.end < cursor {
            if let Some(child) = &self.child {
                return child.find_suggestion_context(cursor);
            }
            return Some(match self.nodes.last() {
                Some(last) => (last.node, last.range.end + 1),
                None => (self.root, self.range.start),
            });
        }
        let mut prev = self.root;
        for parsed in &self.nodes {
            if parsed.range.start <= cursor && cursor <= parsed.range.end {
                return Some((prev, parsed.range.start));
            }
            prev = parsed.node;
        }
        Some((prev, self.range.start))
    }

    /// `ContextChain.tryFlatten`: the redirect stages, then the stage that runs a command. `None` when
    /// the last stage has no command.
    pub fn flatten(&self) -> Option<ContextChain<'_, 'a, K>> {
        let mut modifiers = Vec::new();
        let mut current = self;
        while let Some(child) = &current.child {
            modifiers.push(current);
            current = child;
        }
        let command = current.command?;
        Some(ContextChain {
            modifiers,
            executable: current,
            command,
        })
    }
}

/// `ContextChain`: run each modifier stage on the sources so far (each turns a source into sources for
/// the next stage), then the command of the executable stage on each source left.
#[derive(Clone, Debug)]
pub struct ContextChain<'c, 'a, K> {
    pub modifiers: Vec<&'c CommandContext<'a, K>>,
    pub executable: &'c CommandContext<'a, K>,
    pub command: K,
}

/// `ParseResults`: how far parsing got, the context, and the errors of the children that failed at the
/// position it stopped.
#[derive(Clone, Debug)]
pub struct ParseResults<'a, K> {
    pub context: CommandContext<'a, K>,
    pub input: &'a str,
    /// Where parsing stopped (byte offset).
    pub cursor: usize,
    pub errors: Vec<(NodeId, CommandError)>,
}

impl<'a, K: Copy> ParseResults<'a, K> {
    pub fn reader(&self) -> StringReader<'a> {
        StringReader::at(self.input, self.cursor)
    }

    /// Whether input is left unparsed.
    pub fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    /// `Commands.getParseException`: none when the input was read to the end; the only error when there
    /// is one; otherwise unknown command (nothing parsed) or unknown argument.
    pub fn parse_error(&self) -> Option<CommandError> {
        if !self.can_read() {
            return None;
        }
        if let [(_, error)] = &self.errors[..] {
            return Some(error.clone());
        }
        let error = if self.context.range.is_empty() {
            CommandError::dispatcher_unknown_command()
        } else {
            CommandError::dispatcher_unknown_argument()
        };
        Some(error.at(&self.reader()))
    }

    /// `Commands.finishParsing`: the chain to run, or the error a player sees.
    pub fn context_chain(&self) -> Result<ContextChain<'_, 'a, K>, CommandError> {
        if let Some(error) = self.parse_error() {
            return Err(error);
        }
        self.context
            .flatten()
            .ok_or_else(|| CommandError::dispatcher_unknown_command().at(&self.reader()))
    }
}
