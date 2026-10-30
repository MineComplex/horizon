//! Expected bytes were produced by the real 26.3 classes: `ClientboundCommandsPacket.STREAM_CODEC` on
//! Brigadier 1.3.11 trees, pruned by `Commands.fillUsableCommands` with Paper's patch applied (a
//! single-file Java program run on the server jar; not committed).

use std::io::Cursor;

use super::*;
use crate::command::{argument, literal};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Run(u8),
    Op,
    Custom,
    Summon,
}
use Key::*;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn encode(packet: &CommandsPacket) -> Vec<u8> {
    let mut out = Vec::new();
    packet.azalea_write(&mut out).unwrap();
    out
}

fn packet(commands: PlayerCommands<Key>) -> CommandsPacket {
    commands.to_packet(
        |k| k == Op,
        |k| match k {
            Summon => SuggestionProvider::SummonableEntities,
            _ => SuggestionProvider::AskServer,
        },
    )
}

/// The packet for a player (who cannot use `Op` nodes) and for the whole tree match Java's bytes,
/// and both decode back to the same packet.
fn check(tree: &CommandTree<Key>, player: &str, whole: &str) {
    for (commands, expected) in [
        (PlayerCommands::new(tree, |k| k != Op, true), player),
        (PlayerCommands::whole_tree(tree), whole),
    ] {
        let packet = packet(commands);
        let bytes = encode(&packet);
        assert_eq!(bytes, hex(expected), "{packet:#?}");
        let decoded = CommandsPacket::azalea_read(&mut Cursor::new(&bytes[..])).unwrap();
        assert_eq!(decoded, packet);
    }
}

#[test]
fn literal_argument_redirect() {
    let mut tree = CommandTree::new();
    let foo = tree.register(
        literal("foo")
            .executes(Run(1))
            .then(argument("bar", ArgumentType::integer_min(1)).executes(Run(2))),
    );
    tree.register(literal("baz").redirect(foo));
    let bytes = "040002010205010303666f6f0900010362617a06000362617203010000000100";
    check(&tree, bytes, bytes);
}

#[test]
fn brigadier_types_and_suggestions() {
    let mut tree = CommandTree::new();
    let word = ArgumentType::word;
    tree.register(
        literal("t")
            .then(argument("b", ArgumentType::Bool).executes(Run(1)))
            .then(argument("i", ArgumentType::integer()))
            .then(argument("i2", ArgumentType::Integer { min: -5, max: 5 }))
            .then(argument(
                "l",
                ArgumentType::Long {
                    min: 0,
                    max: i64::MAX,
                },
            ))
            .then(argument(
                "lm",
                ArgumentType::Long {
                    min: i64::MIN,
                    max: 300_000_000_000,
                },
            ))
            .then(argument(
                "f",
                ArgumentType::Float {
                    min: -f32::MAX,
                    max: 1.5,
                },
            ))
            .then(argument(
                "d",
                ArgumentType::Double {
                    min: -2.5,
                    max: 2.5,
                },
            ))
            .then(argument("w", word()))
            .then(argument("s", ArgumentType::string()))
            .then(argument("g", ArgumentType::greedy_string()))
            .then(argument("x", word()).suggests(Custom))
            .then(argument("y", word()).suggests(Summon)),
    );
    let bytes = "0e000101010c02030405060708090a0b0c0d0174060001620002000169030002000269320303ffff\
        fffb000000050200016c040100000000000000000200026c6d040200000045d964b8000200016601023fc000000\
        20001640203c0040000000000004004000000000000020001770500020001730501020001670502120001780500\
        146d696e6563726166743a61736b5f7365727665721200017905001d6d696e6563726166743a73756d6d6f6e6162\
        6c655f656e74697469657300";
    check(&tree, bytes, bytes);
}

#[test]
fn nodes_are_numbered_breadth_first() {
    let mut tree = CommandTree::new();
    tree.register(
        literal("a")
            .then(literal("a1").then(literal("a11").executes(Run(1))))
            .then(literal("a2").executes(Run(1))),
    );
    tree.register(
        literal("b")
            .then(argument("b1", ArgumentType::integer()).then(literal("b11").executes(Run(1)))),
    );
    tree.register(literal("c").redirect(NodeId::ROOT));
    let bytes = "090003010203010204050161010106016209000001630101070261310500026132020108026231030005\
        000361313105000362313100";
    check(&tree, bytes, bytes);
}

#[test]
fn requirements_prune_and_restrict() {
    let mut tree = CommandTree::new();
    tree.register(literal("op").requires(Op).executes(Run(1)));
    tree.register(
        literal("all")
            .executes(Run(2))
            .then(literal("secret").requires(Op).executes(Run(3)))
            .then(argument("v", ArgumentType::word()).executes(Run(4))),
    );
    let whole = "05000201022500026f700502030403616c6c25000673656372657406000176050000";
    check(&tree, "0300010105010203616c6c06000176050000", whole);
    // An operator keeps everything, still flagged restricted.
    let op = packet(PlayerCommands::new(&tree, |_| true, true));
    assert_eq!(encode(&op), hex(whole));
}

#[test]
fn equal_nodes_share_an_entry() {
    let mut tree = CommandTree::new();
    tree.register(literal("execute").then(literal("run").redirect(NodeId::ROOT)));
    tree.register(literal("return").then(literal("run").redirect(NodeId::ROOT)));
    let n = || argument("n", ArgumentType::Integer { min: 0, max: 5 });
    tree.register(literal("p").then(n().executes(Run(1))));
    tree.register(literal("q").then(n().executes(Run(1))));
    tree.register(literal("r").then(n().executes(Run(2))));
    let bytes = "090005010203040501010607657865637574650101060672657475726e010107017001010701710101\
        0801720900000372756e0600016e030300000000000000050600016e0303000000000000000500";
    check(&tree, bytes, bytes);
}

#[test]
fn paper_flattens_dead_redirects() {
    let mut tree = CommandTree::new();
    let a = tree.register(literal("a"));
    let teleport = tree.register(
        literal("teleport")
            .executes(Run(1))
            .then(argument("x", ArgumentType::double()).executes(Run(2))),
    );
    tree.add_child(teleport, literal("again").redirect(teleport));
    // `go` is visited before `teleport` is kept; `alias` points at a node the player may not use.
    tree.add_child(a, literal("go").redirect(teleport));
    let secret = tree.register(literal("secret").requires(Op).executes(Run(3)));
    tree.register(literal("alias").redirect(secret));
    check(
        &tree,
        "0700030102030101040161050205060874656c65706f7274250005616c6961730502050602676f060001780200\
         09000205616761696e00",
        "080004010203040101050161050206070874656c65706f727425000673656372657409000305616c69617309\
         000202676f06000178020009000205616761696e00",
    );
}

#[test]
fn namespaced_commands_can_be_left_out() {
    let mut tree = CommandTree::new();
    tree.register(literal("minecraft:say").executes(Run(1)));
    tree.register(literal("say").executes(Run(1)));
    let sent = packet(PlayerCommands::new(&tree, |_| true, false));
    assert_eq!(sent.entries.len(), 2);
    assert_eq!(sent.entries[1].stub, Stub::Literal("say".into()));
    assert_eq!(
        packet(PlayerCommands::new(&tree, |_| true, true))
            .entries
            .len(),
        3
    );
}

#[test]
fn empty_tree_and_impossible_trees() {
    assert_eq!(encode(&CommandsPacket::empty()), [1, 0, 0, 0]);
    let read = |bytes: &[u8]| CommandsPacket::azalea_read(&mut Cursor::new(bytes));
    assert_eq!(read(&[1, 0, 0, 0]).unwrap(), CommandsPacket::empty());
    // A root that is its own child, a literal redirecting to itself, an index out of range.
    assert!(read(&[1, 0, 1, 0, 0]).is_err());
    assert!(read(&[2, 0, 1, 1, 9, 0, 1, 1, b'a', 0]).is_err());
    assert!(read(&[1, 0, 1, 5, 0]).is_err());
}

#[test]
fn parser_ids_follow_the_registry() {
    for (id, name) in [
        (parser_id::BOOL, "brigadier:bool"),
        (parser_id::FLOAT, "brigadier:float"),
        (parser_id::DOUBLE, "brigadier:double"),
        (parser_id::INTEGER, "brigadier:integer"),
        (parser_id::LONG, "brigadier:long"),
        (parser_id::STRING, "brigadier:string"),
    ] {
        assert_eq!(ARGUMENT_PARSERS[id as usize], name);
    }
    assert_eq!(ARGUMENT_PARSERS[61], "minecraft:uuid");
    for (id, name) in [
        (parser_id::BLOCK_POS, "minecraft:block_pos"),
        (parser_id::COLUMN_POS, "minecraft:column_pos"),
        (parser_id::VEC3, "minecraft:vec3"),
        (parser_id::VEC2, "minecraft:vec2"),
        (parser_id::TEAM_COLOR, "minecraft:team_color"),
        (parser_id::OPERATION, "minecraft:operation"),
        (parser_id::ANGLE, "minecraft:angle"),
        (parser_id::ROTATION, "minecraft:rotation"),
        (parser_id::SWIZZLE, "minecraft:swizzle"),
        (parser_id::INT_RANGE, "minecraft:int_range"),
        (parser_id::FLOAT_RANGE, "minecraft:float_range"),
        (parser_id::TIME, "minecraft:time"),
        (parser_id::UUID, "minecraft:uuid"),
    ] {
        assert_eq!(ARGUMENT_PARSERS[id as usize], name);
    }
}

/// Minecraft's types: their parser ids and `time`'s minimum. Two `vec3` nodes made apart are not
/// equal (Java compares the type instances), so `t pos` and `u pos` get one entry each.
#[test]
fn minecraft_types() {
    let mut tree = CommandTree::new();
    tree.register(
        literal("t").then(
            argument("pos", ArgumentType::vec3())
                .executes(Run(1))
                .then(argument("r", ArgumentType::Rotation).executes(Run(1))),
        ),
    );
    tree.register(literal("u").then(argument("pos", ArgumentType::vec3()).executes(Run(1))));
    tree.register(
        literal("w")
            .then(argument("d", ArgumentType::Time { min: 5 }).executes(Run(1)))
            .then(argument("e", ArgumentType::time()).executes(Run(1))),
    );
    let mut m = literal("m");
    for (name, ty) in [
        ("a", ArgumentType::BlockPos),
        ("b", ArgumentType::ColumnPos),
        ("c", ArgumentType::vec2()),
        ("d", ArgumentType::Angle),
        ("e", ArgumentType::Swizzle),
        ("f", ArgumentType::IntRange),
        ("g", ArgumentType::FloatRange),
        ("h", ArgumentType::Uuid),
        ("i", ArgumentType::Operation),
        ("j", ArgumentType::TeamColor),
    ] {
        m = m.then(argument(name, ty));
    }
    tree.register(m);
    let bytes = "1400040102030401010501740101060175010207080177010a090a0b0c0d0e0f101112016d06011303706f\
        730a060003706f730a060001642b00000005060001652b0000000002000161080200016209020001630b0200016\
        41c020001652002000166270200016728020001683d020001691a0200016a10060001721d00";
    check(&tree, bytes, bytes);
}
