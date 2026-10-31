//! The command tree as the client receives it: `ClientboundCommandsPacket` and the copy of the tree
//! each player is sent (`Commands.sendCommands`).
//!
//! Sending is two steps, neither of which touches the server's [`CommandTree`]:
//!
//! 1. [`PlayerCommands::new`] keeps the nodes a player may use (`Commands.fillUsableCommands`, with
//!    Paper's patch for redirects whose target was not kept: the alias becomes a literal carrying the
//!    target's command, requirement and children).
//! 2. [`PlayerCommands::to_packet`] numbers the nodes breadth-first (`enumerateNodes`) and builds the
//!    entries (`createEntries`). The server's keys `K` reach the wire through two functions the caller
//!    passes: whether a requirement key fails for a source with no permissions (the restricted flag)
//!    and which registered [`SuggestionProvider`] a suggestion key is.
//!
//! Brigadier's nodes are hash keys compared with `equals`: same kind, name, argument type, command and
//! equal children (requirements, redirects and suggestions are not compared). Equal nodes share one
//! packet entry, and a redirect finds any kept node equal to its target. Both are reproduced with
//! structural classes; two commands are "equal" when their keys are equal, so a server gives one key
//! only to what Java would share as one `Command` object.
//!
//! The tree is plain data, so the packet can be built off the tick thread as Leaf does ("Move
//! ClientboundCommandsPacket creation off the main thread"). Paper's plugin hooks are not here:
//! `AsyncPlayerSendCommandsEvent` and `PlayerCommandSendEvent` (which may remove root commands) run
//! between the two steps, and `CommandNode.clientNode` / `RestrictedMarker` replace a node or its
//! restricted flag. Spigot's `tab-complete < 0` sends [`CommandsPacket::empty`].

use std::collections::{HashMap, VecDeque, hash_map::Entry as MapEntry};
use std::hash::Hash;
use std::io::{self, Cursor, Write};

use super::{ArgumentType, CommandTree, NodeId, NodeKind, StringKind};
use crate::azalea::buf::{AzBuf, AzBufVar, BufReadError};

const MASK_TYPE: u8 = 3;
const FLAG_EXECUTABLE: u8 = 4;
const FLAG_REDIRECT: u8 = 8;
const FLAG_CUSTOM_SUGGESTIONS: u8 = 16;
const FLAG_RESTRICTED: u8 = 32;
const TYPE_ROOT: u8 = 0;
const TYPE_LITERAL: u8 = 1;
const TYPE_ARGUMENT: u8 = 2;

/// `minecraft:command_argument_type` in 26.3's registration order (`ArgumentTypeInfos.bootstrap`):
/// the index is the id sent for an argument's parser.
#[rustfmt::skip]
pub const ARGUMENT_PARSERS: [&str; 62] = [
    "brigadier:bool", "brigadier:float", "brigadier:double", "brigadier:integer", "brigadier:long",
    "brigadier:string", "minecraft:entity", "minecraft:game_profile", "minecraft:block_pos",
    "minecraft:column_pos", "minecraft:vec3", "minecraft:vec2", "minecraft:block_state",
    "minecraft:block_predicate", "minecraft:item_stack", "minecraft:item_predicate",
    "minecraft:team_color", "minecraft:hex_color", "minecraft:component", "minecraft:style",
    "minecraft:message", "minecraft:nbt_compound_tag", "minecraft:nbt_tag", "minecraft:nbt_path",
    "minecraft:objective", "minecraft:objective_criteria", "minecraft:operation",
    "minecraft:particle", "minecraft:angle", "minecraft:rotation", "minecraft:scoreboard_slot",
    "minecraft:score_holder", "minecraft:swizzle", "minecraft:team", "minecraft:item_slot",
    "minecraft:item_slots", "minecraft:resource_location", "minecraft:function",
    "minecraft:entity_anchor", "minecraft:int_range", "minecraft:float_range", "minecraft:dimension",
    "minecraft:gamemode", "minecraft:time", "minecraft:resource_or_tag",
    "minecraft:resource_or_tag_key", "minecraft:resource", "minecraft:resource_key",
    "minecraft:resource_selector", "minecraft:template_mirror", "minecraft:template_rotation",
    "minecraft:heightmap", "minecraft:loot_table", "minecraft:loot_predicate",
    "minecraft:loot_modifier", "minecraft:context_float_provider", "minecraft:context_int_provider",
    "minecraft:slot_source", "minecraft:dialog", "minecraft:feature", "minecraft:swing_animation",
    "minecraft:uuid",
];

/// Ids in [`ARGUMENT_PARSERS`] of the types [`ArgumentType`] has; Minecraft's join with their variants.
pub mod parser_id {
    pub const BOOL: i32 = 0;
    pub const FLOAT: i32 = 1;
    pub const DOUBLE: i32 = 2;
    pub const INTEGER: i32 = 3;
    pub const LONG: i32 = 4;
    pub const STRING: i32 = 5;
    pub const GAME_PROFILE: i32 = 7;
    pub const BLOCK_POS: i32 = 8;
    pub const COLUMN_POS: i32 = 9;
    pub const VEC3: i32 = 10;
    pub const VEC2: i32 = 11;
    pub const TEAM_COLOR: i32 = 16;
    pub const RESOURCE_LOCATION: i32 = 36;
    pub const HEX_COLOR: i32 = 17;
    pub const OPERATION: i32 = 26;
    pub const ANGLE: i32 = 28;
    pub const ROTATION: i32 = 29;
    pub const SWIZZLE: i32 = 32;
    pub const INT_RANGE: i32 = 39;
    pub const FLOAT_RANGE: i32 = 40;
    pub const TIME: i32 = 43;
    pub const UUID: i32 = 61;
}

/// `ArgumentUtils.createNumberFlags`.
const NUMBER_FLAG_MIN: u8 = 1;
const NUMBER_FLAG_MAX: u8 = 2;

/// Writes the parser id and its properties (`ArgumentTypeInfo.serializeToNetwork`): number bounds only
/// when not the type's default, a string's kind as its ordinal, `time`'s minimum as an `int`. The
/// other Minecraft types are `SingletonArgumentInfo`s with no properties (`vec3` and `vec2` lose
/// `center_correct`: the client makes the default `vec3()`/`vec2()`).
fn write_argument_type(ty: &ArgumentType, buf: &mut impl Write) -> io::Result<()> {
    fn bounds<T: AzBuf>(
        buf: &mut impl Write,
        (min, has_min): (T, bool),
        (max, has_max): (T, bool),
    ) -> io::Result<()> {
        let flags =
            if has_min { NUMBER_FLAG_MIN } else { 0 } | if has_max { NUMBER_FLAG_MAX } else { 0 };
        flags.azalea_write(buf)?;
        if has_min {
            min.azalea_write(buf)?;
        }
        if has_max {
            max.azalea_write(buf)?;
        }
        Ok(())
    }
    match *ty {
        ArgumentType::Bool => parser_id::BOOL.azalea_write_var(buf),
        ArgumentType::Integer { min, max } => {
            parser_id::INTEGER.azalea_write_var(buf)?;
            bounds(buf, (min, min != i32::MIN), (max, max != i32::MAX))
        }
        ArgumentType::Long { min, max } => {
            parser_id::LONG.azalea_write_var(buf)?;
            bounds(buf, (min, min != i64::MIN), (max, max != i64::MAX))
        }
        ArgumentType::Float { min, max } => {
            parser_id::FLOAT.azalea_write_var(buf)?;
            bounds(buf, (min, min != -f32::MAX), (max, max != f32::MAX))
        }
        ArgumentType::Double { min, max } => {
            parser_id::DOUBLE.azalea_write_var(buf)?;
            bounds(buf, (min, min != -f64::MAX), (max, max != f64::MAX))
        }
        ArgumentType::String(kind) => {
            parser_id::STRING.azalea_write_var(buf)?;
            let ordinal = match kind {
                StringKind::SingleWord => 0,
                StringKind::QuotablePhrase => 1,
                StringKind::GreedyPhrase => 2,
            };
            i32::azalea_write_var(&ordinal, buf)
        }
        ArgumentType::Time { min } => {
            parser_id::TIME.azalea_write_var(buf)?;
            min.azalea_write(buf)
        }
        ArgumentType::BlockPos => parser_id::BLOCK_POS.azalea_write_var(buf),
        ArgumentType::ColumnPos => parser_id::COLUMN_POS.azalea_write_var(buf),
        ArgumentType::Vec3 { .. } => parser_id::VEC3.azalea_write_var(buf),
        ArgumentType::Vec2 { .. } => parser_id::VEC2.azalea_write_var(buf),
        ArgumentType::Rotation => parser_id::ROTATION.azalea_write_var(buf),
        ArgumentType::Angle => parser_id::ANGLE.azalea_write_var(buf),
        ArgumentType::Swizzle => parser_id::SWIZZLE.azalea_write_var(buf),
        ArgumentType::IntRange => parser_id::INT_RANGE.azalea_write_var(buf),
        ArgumentType::FloatRange => parser_id::FLOAT_RANGE.azalea_write_var(buf),
        ArgumentType::Uuid => parser_id::UUID.azalea_write_var(buf),
        ArgumentType::Operation => parser_id::OPERATION.azalea_write_var(buf),
        ArgumentType::TeamColor => parser_id::TEAM_COLOR.azalea_write_var(buf),
        ArgumentType::HexColor => parser_id::HEX_COLOR.azalea_write_var(buf),
        ArgumentType::GameProfile => parser_id::GAME_PROFILE.azalea_write_var(buf),
        ArgumentType::Identifier => parser_id::RESOURCE_LOCATION.azalea_write_var(buf),
    }
}

/// `ArgumentTypeInfo.deserializeFromNetwork` for the parser ids [`ArgumentType`] knows.
fn read_argument_type(buf: &mut Cursor<&[u8]>) -> Result<ArgumentType, BufReadError> {
    fn bounds<T: AzBuf>(buf: &mut Cursor<&[u8]>, default: (T, T)) -> Result<(T, T), BufReadError> {
        let flags = u8::azalea_read(buf)?;
        let min = if flags & NUMBER_FLAG_MIN != 0 {
            T::azalea_read(buf)?
        } else {
            default.0
        };
        let max = if flags & NUMBER_FLAG_MAX != 0 {
            T::azalea_read(buf)?
        } else {
            default.1
        };
        Ok((min, max))
    }
    Ok(match i32::azalea_read_var(buf)? {
        parser_id::BOOL => ArgumentType::Bool,
        parser_id::INTEGER => {
            let (min, max) = bounds(buf, (i32::MIN, i32::MAX))?;
            ArgumentType::Integer { min, max }
        }
        parser_id::LONG => {
            let (min, max) = bounds(buf, (i64::MIN, i64::MAX))?;
            ArgumentType::Long { min, max }
        }
        parser_id::FLOAT => {
            let (min, max) = bounds(buf, (-f32::MAX, f32::MAX))?;
            ArgumentType::Float { min, max }
        }
        parser_id::DOUBLE => {
            let (min, max) = bounds(buf, (-f64::MAX, f64::MAX))?;
            ArgumentType::Double { min, max }
        }
        parser_id::STRING => ArgumentType::String(match i32::azalea_read_var(buf)? {
            0 => StringKind::SingleWord,
            1 => StringKind::QuotablePhrase,
            2 => StringKind::GreedyPhrase,
            id => return Err(BufReadError::UnexpectedEnumVariant { id }),
        }),
        parser_id::TIME => ArgumentType::Time {
            min: i32::azalea_read(buf)?,
        },
        parser_id::BLOCK_POS => ArgumentType::BlockPos,
        parser_id::COLUMN_POS => ArgumentType::ColumnPos,
        parser_id::VEC3 => ArgumentType::vec3(),
        parser_id::VEC2 => ArgumentType::vec2(),
        parser_id::ROTATION => ArgumentType::Rotation,
        parser_id::ANGLE => ArgumentType::Angle,
        parser_id::SWIZZLE => ArgumentType::Swizzle,
        parser_id::INT_RANGE => ArgumentType::IntRange,
        parser_id::FLOAT_RANGE => ArgumentType::FloatRange,
        parser_id::UUID => ArgumentType::Uuid,
        parser_id::OPERATION => ArgumentType::Operation,
        parser_id::TEAM_COLOR => ArgumentType::TeamColor,
        parser_id::HEX_COLOR => ArgumentType::HexColor,
        parser_id::GAME_PROFILE => ArgumentType::GameProfile,
        parser_id::RESOURCE_LOCATION => ArgumentType::Identifier,
        id => {
            let name = usize::try_from(id)
                .ok()
                .and_then(|i| ARGUMENT_PARSERS.get(i));
            return Err(BufReadError::Custom(format!(
                "unsupported argument parser {id} ({})",
                name.unwrap_or(&"not registered")
            )));
        }
    })
}

/// The suggestion providers registered by name (`SuggestionProviders`). Any other provider, a
/// plugin's included, is sent as `ask_server`: the client asks the server for its suggestions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuggestionProvider {
    AskServer,
    AvailableSounds,
    SummonableEntities,
    PostEffects,
}

impl SuggestionProvider {
    pub const fn id(self) -> &'static str {
        match self {
            Self::AskServer => "minecraft:ask_server",
            Self::AvailableSounds => "minecraft:available_sounds",
            Self::SummonableEntities => "minecraft:summonable_entities",
            Self::PostEffects => "minecraft:post_effects",
        }
    }
}

/// An entry's node (`ClientboundCommandsPacket.NodeStub`; `None` there for the root).
#[derive(Clone, Debug, PartialEq)]
pub enum Stub {
    Root,
    Literal(Box<str>),
    Argument {
        name: Box<str>,
        ty: ArgumentType,
        /// The suggestion provider's identifier.
        suggestions: Option<Box<str>>,
    },
}

/// One node of the packet. Children and redirect are indexes of entries.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub stub: Stub,
    pub executable: bool,
    /// The node's requirement fails for a source with no permissions.
    pub restricted: bool,
    pub children: Vec<u32>,
    pub redirect: Option<u32>,
}

/// `ClientboundCommandsPacket`: the entries and the root's index.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandsPacket {
    pub entries: Vec<Entry>,
    pub root: u32,
}

impl CommandsPacket {
    /// A tree with only a root (Paper, when tab completion is off).
    pub fn empty() -> Self {
        Self {
            entries: vec![Entry {
                stub: Stub::Root,
                executable: false,
                restricted: false,
                children: Vec::new(),
                redirect: None,
            }],
            root: 0,
        }
    }

    /// The checks the client makes on receiving the packet (`validateEntries`): indexes in range,
    /// and no node whose redirects or children lead back to itself ("Server sent an impossible
    /// command tree").
    fn validate(&self) -> Result<(), BufReadError> {
        let len = self.entries.len();
        let in_range = |i: u32| (i as usize) < len;
        if !in_range(self.root)
            || !self
                .entries
                .iter()
                .all(|e| e.children.iter().all(|&c| in_range(c)) && e.redirect.is_none_or(in_range))
        {
            return Err(BufReadError::Custom(
                "command tree index out of range".into(),
            ));
        }
        // `canBuild` (the redirect is built first), then `canResolve` (the children are).
        if settles(&self.entries, |e| e.redirect.as_slice())
            && settles(&self.entries, |e| &e.children)
        {
            Ok(())
        } else {
            Err(BufReadError::Custom(
                "Server sent an impossible command tree".into(),
            ))
        }
    }
}

/// Whether every entry can be taken once its `deps` are, repeating passes until none is left.
fn settles(entries: &[Entry], deps: impl Fn(&Entry) -> &[u32]) -> bool {
    let mut pending = vec![true; entries.len()];
    let mut left = entries.len();
    while left > 0 {
        let before = left;
        for (i, entry) in entries.iter().enumerate() {
            if pending[i] && deps(entry).iter().all(|&d| !pending[d as usize]) {
                pending[i] = false;
                left -= 1;
            }
        }
        if left == before {
            return false;
        }
    }
    true
}

impl AzBuf for CommandsPacket {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        fn count(buf: &mut Cursor<&[u8]>) -> Result<usize, BufReadError> {
            let n = i32::azalea_read_var(buf)?;
            usize::try_from(n).map_err(|_| BufReadError::Custom(format!("negative length {n}")))
        }
        let len = count(buf)?;
        let mut entries = Vec::with_capacity(len.min(65536));
        for _ in 0..len {
            let flags = u8::azalea_read(buf)?;
            let children = (0..count(buf)?)
                .map(|_| u32::azalea_read_var(buf))
                .collect::<Result<_, _>>()?;
            let redirect = if flags & FLAG_REDIRECT != 0 {
                Some(u32::azalea_read_var(buf)?)
            } else {
                None
            };
            let stub = match flags & MASK_TYPE {
                TYPE_ROOT => Stub::Root,
                TYPE_LITERAL => Stub::Literal(Box::azalea_read(buf)?),
                TYPE_ARGUMENT => Stub::Argument {
                    name: Box::azalea_read(buf)?,
                    ty: read_argument_type(buf)?,
                    suggestions: if flags & FLAG_CUSTOM_SUGGESTIONS != 0 {
                        Some(Box::azalea_read(buf)?)
                    } else {
                        None
                    },
                },
                _ => return Err(BufReadError::Custom("command node type 3".into())),
            };
            entries.push(Entry {
                stub,
                executable: flags & FLAG_EXECUTABLE != 0,
                restricted: flags & FLAG_RESTRICTED != 0,
                children,
                redirect,
            });
        }
        let packet = Self {
            entries,
            root: u32::azalea_read_var(buf)?,
        };
        packet.validate()?;
        Ok(packet)
    }

    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        (self.entries.len() as u32).azalea_write_var(buf)?;
        for entry in &self.entries {
            let mut flags = match entry.stub {
                Stub::Root => TYPE_ROOT,
                Stub::Literal(_) => TYPE_LITERAL,
                Stub::Argument { .. } => TYPE_ARGUMENT,
            };
            for (set, flag) in [
                (entry.executable, FLAG_EXECUTABLE),
                (entry.redirect.is_some(), FLAG_REDIRECT),
                (
                    matches!(
                        &entry.stub,
                        Stub::Argument {
                            suggestions: Some(_),
                            ..
                        }
                    ),
                    FLAG_CUSTOM_SUGGESTIONS,
                ),
                (entry.restricted, FLAG_RESTRICTED),
            ] {
                if set {
                    flags |= flag;
                }
            }
            flags.azalea_write(buf)?;
            entry.children.azalea_write_var(buf)?;
            if let Some(redirect) = entry.redirect {
                redirect.azalea_write_var(buf)?;
            }
            match &entry.stub {
                Stub::Root => {}
                Stub::Literal(literal) => literal.azalea_write(buf)?,
                Stub::Argument {
                    name,
                    ty,
                    suggestions,
                } => {
                    name.azalea_write(buf)?;
                    write_argument_type(ty, buf)?;
                    if let Some(id) = suggestions {
                        id.azalea_write(buf)?;
                    }
                }
            }
        }
        self.root.azalea_write_var(buf)
    }
}

/// Where a sent node comes from.
#[derive(Clone, Copy, Debug)]
enum Origin {
    /// A copy of a server node (`createBuilder`), with the children and redirect it is sent with.
    Copy(NodeId),
    /// The server's node itself, as Paper's flattening adds it: its own children and redirect.
    Raw(NodeId),
    /// Paper's literal named like `alias`, whose redirect `target` was not kept: the target's
    /// requirement and command.
    Flattened { alias: NodeId, target: NodeId },
}

impl Origin {
    /// The node whose command and requirement this one has.
    fn behaviour(self) -> NodeId {
        match self {
            Self::Copy(id) | Self::Raw(id) | Self::Flattened { target: id, .. } => id,
        }
    }
}

#[derive(Clone, Debug)]
struct SentNode {
    origin: Origin,
    children: Vec<u32>,
    redirect: Option<u32>,
}

/// A node's identity for Brigadier's `equals`: kind with name and argument type, command and the
/// classes of its children (a set: `LinkedHashMap.equals` ignores order).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClassKey<'t, K> {
    kind: KindKey<'t>,
    command: Option<K>,
    children: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KindKey<'t> {
    Root,
    Literal(&'t str),
    Argument(&'t str, TypeKey),
}

/// `ArgumentType.equals`: Brigadier's types compare their bounds with `==` (so `-0.0` equals `0.0`).
/// Minecraft's types keep `Object.equals` (identity), and vanilla makes a new instance for each
/// node, so their key is the server node that holds them: copies of one node are equal, two nodes
/// never are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TypeKey {
    Bool,
    Integer(i32, i32),
    Long(i64, i64),
    Float(u32, u32),
    Double(u64, u64),
    String(StringKind),
    Instance(NodeId),
}

/// The key of the server node `id`, whose kind is `kind`.
fn kind_key(kind: &NodeKind, id: NodeId) -> KindKey<'_> {
    let f = |v: f32| if v == 0.0 { 0 } else { v.to_bits() };
    let d = |v: f64| if v == 0.0 { 0 } else { v.to_bits() };
    match kind {
        NodeKind::Root => KindKey::Root,
        NodeKind::Literal(name) => KindKey::Literal(name),
        NodeKind::Argument { name, ty } => KindKey::Argument(
            name,
            match *ty {
                ArgumentType::Bool => TypeKey::Bool,
                ArgumentType::Integer { min, max } => TypeKey::Integer(min, max),
                ArgumentType::Long { min, max } => TypeKey::Long(min, max),
                ArgumentType::Float { min, max } => TypeKey::Float(f(min), f(max)),
                ArgumentType::Double { min, max } => TypeKey::Double(d(min), d(max)),
                ArgumentType::String(kind) => TypeKey::String(kind),
                _ => TypeKey::Instance(id),
            },
        ),
    }
}

/// Interned [`ClassKey`]s.
#[derive(Clone, Debug)]
struct Classes<'t, K> {
    keys: HashMap<ClassKey<'t, K>, u32>,
}

impl<'t, K: Copy + Eq + Hash> Classes<'t, K> {
    fn intern(&mut self, kind: KindKey<'t>, command: Option<K>, mut children: Vec<u32>) -> u32 {
        children.sort_unstable();
        let next = self.keys.len() as u32;
        *self
            .keys
            .entry(ClassKey {
                kind,
                command,
                children,
            })
            .or_insert(next)
    }

    /// The class of every node of `tree`. A child is always added after its parent, so a reverse
    /// pass sees every node's children first.
    fn of_tree(&mut self, tree: &'t CommandTree<K>) -> Vec<u32> {
        let mut of = vec![0; tree.nodes().len()];
        for (i, node) in tree.nodes().iter().enumerate().rev() {
            let children = node.children().iter().map(|c| of[c.index()]).collect();
            let id = NodeId::from_index(i);
            of[i] = self.intern(kind_key(node.kind(), id), node.command(), children);
        }
        of
    }
}

/// The commands one player is sent: the nodes of a [`CommandTree`] they may use, as copies that point
/// back into the tree.
#[derive(Clone, Debug)]
pub struct PlayerCommands<'t, K> {
    tree: &'t CommandTree<K>,
    /// Index 0 is the root.
    nodes: Vec<SentNode>,
    /// Raw nodes made so far, by server node index.
    raw: HashMap<usize, u32>,
    classes: Classes<'t, K>,
    /// The class of every server node.
    tree_class: Vec<u32>,
}

impl<'t, K: Copy + Eq + Hash> PlayerCommands<'t, K> {
    /// `Commands.sendCommands` up to the packet: the nodes whose requirement `can_use` accepts, in the
    /// tree's order. A redirect points at the kept copy of (a node equal to) its target; when there is
    /// none yet, Paper sends the alias as a literal with the target's requirement, command and
    /// (unfiltered) children. `send_namespaced` is Spigot's `commands.send-namespaced` (default true):
    /// when false, nodes whose name has a `:` are left out.
    pub fn new(
        tree: &'t CommandTree<K>,
        can_use: impl Fn(K) -> bool,
        send_namespaced: bool,
    ) -> Self {
        let mut this = Self::start(tree, Origin::Copy(NodeId::ROOT));
        let mut converted = HashMap::from([(this.tree_class[NodeId::ROOT.index()], 0)]);
        this.fill(NodeId::ROOT, 0, &can_use, send_namespaced, &mut converted);
        this
    }

    /// The whole tree, as `new ClientboundCommandsPacket(dispatcher.getRoot(), ...)` sends it.
    pub fn whole_tree(tree: &'t CommandTree<K>) -> Self {
        let mut this = Self::start(tree, Origin::Raw(NodeId::ROOT));
        this.raw.insert(NodeId::ROOT.index(), 0);
        this.make_raw_children(0, NodeId::ROOT);
        this
    }

    fn start(tree: &'t CommandTree<K>, root: Origin) -> Self {
        let mut classes = Classes {
            keys: HashMap::new(),
        };
        let tree_class = classes.of_tree(tree);
        Self {
            tree,
            nodes: vec![SentNode {
                origin: root,
                children: Vec::new(),
                redirect: None,
            }],
            raw: HashMap::new(),
            classes,
            tree_class,
        }
    }

    fn push(&mut self, origin: Origin, redirect: Option<u32>) -> u32 {
        self.nodes.push(SentNode {
            origin,
            children: Vec::new(),
            redirect,
        });
        (self.nodes.len() - 1) as u32
    }

    /// `fillUsableCommands` with Paper's patch. `converted` maps a server node's class to its copy
    /// (`HashMap.put`: the latest of equal nodes).
    fn fill(
        &mut self,
        source: NodeId,
        target: u32,
        can_use: &impl Fn(K) -> bool,
        send_namespaced: bool,
        converted: &mut HashMap<u32, u32>,
    ) {
        let tree = self.tree;
        for &child in tree.node(source).children() {
            let node = tree.node(child);
            if (!send_namespaced && node.name().contains(':'))
                || !node.requirement().is_none_or(can_use)
            {
                continue;
            }
            let sent = match node.redirect() {
                None => self.push(Origin::Copy(child), None),
                Some(to) => match converted.get(&self.tree_class[to.index()]) {
                    Some(&copy) => self.push(Origin::Copy(child), Some(copy)),
                    None => {
                        let origin = Origin::Flattened {
                            alias: child,
                            target: to,
                        };
                        let sent = self.push(origin, None);
                        self.make_raw_children(sent, to);
                        sent
                    }
                },
            };
            converted.insert(self.tree_class[child.index()], sent);
            self.nodes[target as usize].children.push(sent);
            if !node.children().is_empty() {
                self.fill(child, sent, can_use, send_namespaced, converted);
            }
        }
    }

    /// Gives `sent` the server node `of`'s children as raw nodes, and makes everything they reach
    /// (children and redirects) raw too, each server node once.
    fn make_raw_children(&mut self, sent: u32, of: NodeId) {
        let tree = self.tree;
        let mut work = vec![(sent, of, false)];
        while let Some((sent, of, with_redirect)) = work.pop() {
            let mut raw = |this: &mut Self, id: NodeId| -> u32 {
                if let Some(&raw) = this.raw.get(&id.index()) {
                    return raw;
                }
                let raw = this.push(Origin::Raw(id), None);
                this.raw.insert(id.index(), raw);
                work.push((raw, id, true));
                raw
            };
            let node = tree.node(of);
            let children = node.children().iter().map(|&c| raw(self, c)).collect();
            let redirect = match node.redirect() {
                Some(to) if with_redirect => Some(raw(self, to)),
                _ => None,
            };
            let sent = &mut self.nodes[sent as usize];
            sent.children = children;
            sent.redirect = redirect;
        }
    }

    /// The equality class of every sent node. Raw nodes are their server node's class; a copy or
    /// flattened node comes after its parent, so a reverse pass sees its children first.
    fn classes(&mut self) -> Vec<u32> {
        let tree = self.tree;
        let mut class = vec![0; self.nodes.len()];
        for (i, node) in self.nodes.iter().enumerate().rev() {
            class[i] = match node.origin {
                Origin::Raw(id) => self.tree_class[id.index()],
                origin => {
                    let kind = match origin {
                        Origin::Flattened { alias, .. } => {
                            KindKey::Literal(tree.node(alias).name())
                        }
                        _ => kind_key(tree.node(origin.behaviour()).kind(), origin.behaviour()),
                    };
                    let command = tree.node(origin.behaviour()).command();
                    let children = node.children.iter().map(|&c| class[c as usize]).collect();
                    self.classes.intern(kind, command, children)
                }
            };
        }
        class
    }

    /// `new ClientboundCommandsPacket(root, COMMAND_NODE_INSPECTOR)`. `is_restricted` tells whether a
    /// requirement fails for a source with no permissions; `suggestions` names a suggestion key's
    /// provider.
    pub fn to_packet(
        mut self,
        is_restricted: impl Fn(K) -> bool,
        suggestions: impl Fn(K) -> SuggestionProvider,
    ) -> CommandsPacket {
        let tree = self.tree;
        let class = self.classes();
        // enumerateNodes: breadth-first, children then the redirect; a node equal to one already
        // numbered is skipped with everything below it.
        let mut ids: HashMap<u32, u32> = HashMap::new();
        let mut order = Vec::new();
        let mut queue = VecDeque::from([0u32]);
        while let Some(n) = queue.pop_front() {
            if let MapEntry::Vacant(slot) = ids.entry(class[n as usize]) {
                slot.insert(order.len() as u32);
                order.push(n);
                let node = &self.nodes[n as usize];
                queue.extend(&node.children);
                queue.extend(node.redirect);
            }
        }
        // `Object2IntMap.getInt` gives 0 for a node never numbered.
        let id = |n: u32| ids.get(&class[n as usize]).copied().unwrap_or(0);
        let entries = order
            .iter()
            .map(|&n| {
                let node = &self.nodes[n as usize];
                let stub = match node.origin {
                    Origin::Flattened { alias, .. } => {
                        Stub::Literal(tree.node(alias).name().into())
                    }
                    Origin::Copy(id) | Origin::Raw(id) => match tree.node(id).kind() {
                        NodeKind::Root => Stub::Root,
                        NodeKind::Literal(literal) => Stub::Literal(literal.clone()),
                        NodeKind::Argument { name, ty } => Stub::Argument {
                            name: name.clone(),
                            ty: *ty,
                            suggestions: tree
                                .node(id)
                                .custom_suggestions()
                                .map(|k| suggestions(k).id().into()),
                        },
                    },
                };
                let behaviour = tree.node(node.origin.behaviour());
                Entry {
                    stub,
                    executable: behaviour.command().is_some(),
                    restricted: behaviour.requirement().is_some_and(&is_restricted),
                    children: node.children.iter().map(|&c| id(c)).collect(),
                    redirect: node.redirect.map(id),
                }
            })
            .collect();
        CommandsPacket {
            entries,
            root: id(0),
        }
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
