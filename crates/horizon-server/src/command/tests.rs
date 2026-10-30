//! Cases from Brigadier's own test suite (`CommandDispatcherTest`, `CommandSuggestionsTest`,
//! `CommandDispatcherUsagesTest`, `StringReaderTest`, `CommandSyntaxExceptionTest`), with the values
//! those tests expect, plus Minecraft's error keys.

use super::*;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Command,
    SubCommand,
    Denied,
    Modifier,
    Numbers,
}

fn everyone(key: Key) -> bool {
    key != Key::Denied
}

fn parse_error(tree: &CommandTree<Key>, input: &str) -> CommandError {
    let parse = tree.parse(input, everyone);
    match parse.context_chain() {
        Ok(_) => panic!("{input:?} parsed"),
        Err(error) => error,
    }
}

fn command_of(tree: &CommandTree<Key>, input: &str) -> Key {
    let parse = tree.parse(input, everyone);
    parse
        .context_chain()
        .map(|chain| chain.command)
        .unwrap_or_else(|e| panic!("{input:?}: {e:?}"))
}

fn texts(suggestions: &Suggestions) -> Vec<&str> {
    suggestions.list.iter().map(|s| s.text.as_str()).collect()
}

fn suggest(tree: &CommandTree<Key>, input: &str, cursor: usize) -> Suggestions {
    let parse = tree.parse(input, everyone);
    tree.completion_suggestions(&parse, cursor, |_, _, builder| {
        for n in [10, 9, 100] {
            if n.to_string().starts_with(builder.remaining()) {
                builder.suggest_int(n);
            }
        }
        Ok(())
    })
}

#[test]
fn create_and_execute_command() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(literal("foo").executes(Key::Command));
    assert_eq!(command_of(&tree, "foo"), Key::Command);
}

#[test]
fn unknown_impermissible_and_empty_commands() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(literal("bar"));
    tree.register(literal("baz"));
    tree.register(
        literal("hidden")
            .requires(Key::Denied)
            .executes(Key::Command),
    );
    for input in ["foo", "hidden", ""] {
        let error = parse_error(&tree, input);
        assert_eq!(error.key(), "command.unknown.command", "{input:?}");
        assert_eq!(error.cursor(), 0);
    }
}

#[test]
fn unknown_and_incorrect_subcommands() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("foo")
            .executes(Key::Command)
            .then(literal("bar"))
            .then(literal("baz")),
    );
    for input in ["foo unknown", "foo baz_", "foo bar baz"] {
        let error = parse_error(&tree, input);
        assert_eq!(error.key(), "command.unknown.argument", "{input:?}");
    }
    assert_eq!(parse_error(&tree, "foo unknown").cursor(), 4);
}

#[test]
fn subcommand_and_ambiguous_parent() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("foo")
            .then(literal("a"))
            .then(literal("=").executes(Key::SubCommand))
            .then(literal("c"))
            .executes(Key::Command),
    );
    assert_eq!(command_of(&tree, "foo ="), Key::SubCommand);

    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("test")
            .then(argument("incorrect", ArgumentType::integer()).executes(Key::Command))
            .then(
                argument("right", ArgumentType::integer())
                    .then(argument("sub", ArgumentType::integer()).executes(Key::SubCommand)),
            ),
    );
    assert_eq!(command_of(&tree, "test 1 2"), Key::SubCommand);
    let parse = tree.parse("test 1 2", everyone);
    assert_eq!(parse.context.integer("right"), Some(1));
    assert_eq!(parse.context.integer("sub"), Some(2));
    assert_eq!(parse.context.integer("incorrect"), None);
}

#[test]
fn incomplete_input_stops_before_the_space() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(literal("foo").then(literal("bar").executes(Key::Command)));
    tree.register(
        literal("num").then(argument("bar", ArgumentType::integer()).executes(Key::Command)),
    );
    for input in ["foo ", "num "] {
        let parse = tree.parse(input, everyone);
        assert_eq!(parse.reader().remaining(), " ");
        assert_eq!(parse.context.nodes().len(), 1);
    }
}

#[test]
fn errors_of_the_only_child_and_missing_separators() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("foo")
            .executes(Key::Command)
            .then(argument("bar", ArgumentType::integer()).executes(Key::Command)),
    );
    let error = parse_error(&tree, "foo bar");
    assert_eq!(error.key(), "parsing.int.expected");
    assert_eq!(error.cursor(), 4);
    // The literal's word is "foo$", which matches no literal, so nothing parses.
    let error = parse_error(&tree, "foo$");
    assert_eq!(
        (error.key(), error.cursor()),
        ("command.unknown.command", 0)
    );
    let error = parse_error(&tree, "foo 5x");
    assert_eq!(
        (error.key(), error.cursor()),
        ("command.expected.separator", 5)
    );
}

#[test]
fn orphaned_subcommand_is_unknown() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("foo")
            .then(argument("bar", ArgumentType::integer()))
            .executes(Key::Command),
    );
    let error = parse_error(&tree, "foo 5");
    assert_eq!(
        (error.key(), error.cursor()),
        ("command.unknown.command", 5)
    );
}

#[test]
fn redirects_start_new_stages() {
    let mut tree = CommandTree::<Key>::new();
    let actual = tree.register(literal("actual").executes(Key::Command));
    tree.register(literal("redirected").redirect(NodeId::ROOT));
    let input = "redirected redirected actual";
    let parse = tree.parse(input, everyone);
    let first = &parse.context;
    assert_eq!(first.range().get(input), "redirected");
    assert_eq!(first.nodes().len(), 1);
    assert_eq!(first.root(), NodeId::ROOT);
    let second = first.child().unwrap();
    assert_eq!(second.range().get(input), "redirected");
    let third = second.child().unwrap();
    assert_eq!(third.range().get(input), "actual");
    assert_eq!(third.nodes()[0].node, actual);
    let chain = parse.context_chain().unwrap();
    assert_eq!(chain.modifiers.len(), 2);
    assert_eq!(chain.command, Key::Command);
}

#[test]
fn forks_carry_their_modifier() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(literal("actual").executes(Key::Command));
    tree.register(literal("redirected").fork(NodeId::ROOT, Key::Modifier));
    let parse = tree.parse("redirected actual", everyone);
    let chain = parse.context_chain().unwrap();
    assert_eq!(chain.modifiers.len(), 1);
    assert_eq!(chain.modifiers[0].modifier(), Some(Key::Modifier));
    assert!(chain.modifiers[0].is_forked());
    assert_eq!(chain.executable.modifier(), None);
}

#[test]
fn registering_twice_merges() {
    let mut tree = CommandTree::<Key>::new();
    let first = tree.register(literal("base").then(literal("foo").executes(Key::Command)));
    let second = tree.register(
        literal("base")
            .requires(Key::Denied)
            .then(literal("bar").executes(Key::SubCommand)),
    );
    assert_eq!(first, second);
    // The merged node keeps its own requirement and gains the new children.
    assert_eq!(tree.node(first).requirement(), None);
    assert_eq!(command_of(&tree, "base foo"), Key::Command);
    assert_eq!(command_of(&tree, "base bar"), Key::SubCommand);
}

#[test]
fn paths_and_find_node() {
    let mut tree = CommandTree::<Key>::new();
    let bar = literal("bar");
    tree.register(literal("foo").then(bar));
    let node = tree.find_node(["foo", "bar"]).unwrap();
    assert_eq!(tree.path(node), ["foo", "bar"]);
    assert_eq!(tree.find_node(["foo", "baz"]), None);
}

#[test]
fn root_suggestions() {
    let mut tree = CommandTree::<Key>::new();
    for name in ["foo", "bar", "baz"] {
        tree.register(literal(name));
    }
    let all = suggest(&tree, "", 0);
    assert_eq!(all.range, StringRange::at(0));
    assert_eq!(texts(&all), ["bar", "baz", "foo"]);
    let partial = suggest(&tree, "b", 1);
    assert_eq!(partial.range, StringRange::between(0, 1));
    assert_eq!(texts(&partial), ["bar", "baz"]);
}

#[test]
fn suggestions_follow_the_cursor() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("parent_one")
            .then(literal("faz"))
            .then(literal("fbz"))
            .then(literal("gaz")),
    );
    tree.register(literal("parent_two"));
    let input = "parent_one faz ";
    let cases: &[(usize, (usize, usize), &[&str])] = &[
        (0, (0, 0), &["parent_one", "parent_two"]),
        (1, (0, 1), &["parent_one", "parent_two"]),
        (7, (0, 7), &["parent_one", "parent_two"]),
        (8, (0, 8), &["parent_one"]),
        (10, (0, 0), &[]),
        (11, (11, 11), &["faz", "fbz", "gaz"]),
        (12, (11, 12), &["faz", "fbz"]),
        (13, (11, 13), &["faz"]),
        (14, (0, 0), &[]),
        (15, (0, 0), &[]),
    ];
    for &(cursor, (start, end), expected) in cases {
        let suggestions = suggest(&tree, input, cursor);
        assert_eq!(texts(&suggestions), expected, "cursor {cursor}");
        assert_eq!(
            suggestions.range,
            StringRange::between(start, end),
            "cursor {cursor}"
        );
    }
}

#[test]
fn suggestions_through_redirects() {
    let mut tree = CommandTree::<Key>::new();
    let actual = tree.register(literal("actual").then(literal("sub")));
    tree.register(literal("redirect").redirect(actual));
    let suggestions = suggest(&tree, "redirect ", 9);
    assert_eq!(
        (suggestions.range, texts(&suggestions)),
        (StringRange::at(9), vec!["sub"])
    );
    let suggestions = suggest(&tree, "redirect s", 10);
    assert_eq!(
        (suggestions.range, texts(&suggestions)),
        (StringRange::between(9, 10), vec!["sub"])
    );
}

#[test]
fn provider_and_type_suggestions_merge_and_sort() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("set")
            .then(argument("n", ArgumentType::integer()).suggests(Key::Numbers))
            .then(argument("flag", ArgumentType::Bool))
            .then(literal("Other")),
    );
    let suggestions = suggest(&tree, "set ", 4);
    // Integers among themselves numerically; mixed with text, by text, ignoring case.
    assert_eq!(
        texts(&suggestions),
        ["9", "10", "100", "false", "Other", "true"]
    );
    let suggestions = suggest(&tree, "set t", 5);
    assert_eq!(texts(&suggestions), ["true"]);
}

#[test]
fn smart_and_all_usage() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("a")
            .then(
                literal("1")
                    .then(literal("i").executes(Key::Command))
                    .then(literal("ii").executes(Key::Command)),
            )
            .then(
                literal("2")
                    .then(literal("i").executes(Key::Command))
                    .then(literal("ii").executes(Key::Command)),
            ),
    );
    tree.register(literal("b").then(literal("1").executes(Key::Command)));
    tree.register(literal("c").executes(Key::Command));
    tree.register(literal("d").requires(Key::Denied).executes(Key::Command));
    tree.register(
        literal("e").executes(Key::Command).then(
            literal("1")
                .executes(Key::Command)
                .then(literal("i").executes(Key::Command)),
        ),
    );
    let h = tree.register(
        literal("h")
            .executes(Key::Command)
            .then(literal("1").then(literal("i").executes(Key::Command))),
    );
    tree.register(
        literal("i")
            .executes(Key::Command)
            .then(literal("1").executes(Key::Command))
            .then(literal("2").executes(Key::Command)),
    );
    tree.register(literal("j").redirect(NodeId::ROOT));
    tree.register(literal("k").redirect(h));

    let smart: Vec<String> = tree
        .smart_usage(NodeId::ROOT, everyone)
        .into_iter()
        .map(|(_, u)| u)
        .collect();
    assert_eq!(
        smart,
        [
            "a (1|2)", "b 1", "c", "e [1]", "h [1]", "i [1|2]", "j ...", "k -> h",
        ]
    );
    let all = tree.all_usage(NodeId::ROOT, everyone, true);
    assert_eq!(
        all,
        [
            "a 1 i", "a 1 ii", "a 2 i", "a 2 ii", "b 1", "c", "e", "e 1", "e 1 i", "h", "h 1 i",
            "i", "i 1", "i 2", "j ...", "k -> h",
        ]
    );
    assert!(
        tree.all_usage(NodeId::ROOT, everyone, false)
            .contains(&"d".to_owned())
    );
}

#[test]
fn ambiguities_between_arguments() {
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("test")
            .then(argument("incorrect", ArgumentType::integer()).executes(Key::Command))
            .then(argument("right", ArgumentType::integer()).executes(Key::Command)),
    );
    let mut found = Vec::new();
    tree.find_ambiguities(|_, child, sibling, inputs| {
        found.push((
            tree.node(child).name().to_owned(),
            tree.node(sibling).name().to_owned(),
            inputs.to_vec().join(","),
        ));
    });
    assert_eq!(
        found,
        [
            (
                "incorrect".to_owned(),
                "right".to_owned(),
                "0,123,-123".to_owned()
            ),
            (
                "right".to_owned(),
                "incorrect".to_owned(),
                "0,123,-123".to_owned()
            ),
        ]
    );
}

#[test]
fn reader_matches_brigadier() {
    let mut reader = StringReader::new("12 -3.5 abc \"q\\\"x\" 'a b' true 9000000000");
    assert_eq!(reader.read_int().unwrap(), 12);
    reader.skip();
    assert_eq!(reader.read_double().unwrap(), -3.5);
    reader.skip();
    assert_eq!(reader.read_unquoted_string(), "abc");
    reader.skip();
    assert_eq!(reader.read_string().unwrap(), "q\"x");
    reader.skip();
    assert_eq!(reader.read_quoted_string().unwrap(), "a b");
    reader.skip();
    assert!(reader.read_boolean().unwrap());
    reader.skip();
    assert_eq!(reader.read_long().unwrap(), 9_000_000_000);

    // Numbers Java accepts and rejects over [0-9.-].
    assert_eq!(StringReader::new(".5").read_double().unwrap(), 0.5);
    assert_eq!(StringReader::new("5.").read_float().unwrap(), 5.0);
    let mut reader = StringReader::new("1.5");
    let error = reader.read_int().unwrap_err();
    assert_eq!(
        (error.key(), error.cursor(), reader.cursor()),
        ("parsing.int.invalid", 0, 0)
    );
    assert_eq!(error.args(), [PrimitiveOrComponent::String("1.5".into())]);
    assert_eq!(
        StringReader::new("-").read_double().unwrap_err().key(),
        "parsing.double.invalid"
    );
    assert_eq!(
        StringReader::new("x").read_long().unwrap_err().key(),
        "parsing.long.expected"
    );

    let error = StringReader::new("\"a\\b\"").read_string().unwrap_err();
    assert_eq!((error.key(), error.cursor()), ("parsing.quote.escape", 3));
    let error = StringReader::new("\"open").read_string().unwrap_err();
    assert_eq!(
        (error.key(), error.cursor()),
        ("parsing.quote.expected.end", 5)
    );
    let error = StringReader::new("abc").read_quoted_string().unwrap_err();
    assert_eq!(error.key(), "parsing.quote.expected.start");
    let error = StringReader::new("tuesday").read_boolean().unwrap_err();
    assert_eq!((error.key(), error.cursor()), ("parsing.bool.invalid", 0));
    let error = StringReader::new("").read_boolean().unwrap_err();
    assert_eq!(error.key(), "parsing.bool.expected");
    let error = StringReader::new("a").expect('b').unwrap_err();
    assert_eq!(error.args(), [PrimitiveOrComponent::String("b".into())]);
}

#[test]
fn bounds_report_min_then_found() {
    let mut reader = StringReader::new("-1");
    let error = ArgumentType::Integer { min: 0, max: 10 }
        .parse(&mut reader)
        .unwrap_err();
    assert_eq!(error.key(), "argument.integer.low");
    assert_eq!(
        error.args(),
        [
            PrimitiveOrComponent::Integer(0),
            PrimitiveOrComponent::Integer(-1)
        ]
    );
    assert_eq!(reader.cursor(), 0);
    let error = ArgumentType::Double { min: 0.0, max: 1.5 }
        .parse(&mut StringReader::new("2"))
        .unwrap_err();
    assert_eq!(error.key(), "argument.double.big");
    assert_eq!(
        error.args(),
        [
            PrimitiveOrComponent::Double(1.5),
            PrimitiveOrComponent::Double(2.0)
        ]
    );
    let mut reader = StringReader::new("all of it");
    assert_eq!(
        ArgumentType::greedy_string().parse(&mut reader).unwrap(),
        ArgumentValue::String("all of it".into())
    );
    assert!(!reader.can_read());
}

#[test]
fn error_context_and_message() {
    let input = "Hello world! This has an error in it. Oh dear!";
    let error = CommandError::reader_expected_int().at(&StringReader::at(input, 20));
    assert_eq!(error.context().unwrap(), "...d! This ha<--[HERE]");
    let error = CommandError::reader_expected_int().at(&StringReader::at("Hello", 5));
    assert_eq!(error.context().unwrap(), "Hello<--[HERE]");
    assert_eq!(CommandError::reader_expected_int().context(), None);
    // No language is installed in tests, so the key is the text.
    assert_eq!(
        error.to_string(),
        "parsing.int.expected at position 5: Hello<--[HERE]"
    );
}

#[test]
fn positions_convert_to_utf16() {
    let input = "say é😀 x";
    let mut tree = CommandTree::<Key>::new();
    tree.register(
        literal("say")
            .then(argument("message", ArgumentType::greedy_string()).executes(Key::Command)),
    );
    let parse = tree.parse(input, everyone);
    let parsed = &parse.context.nodes()[1];
    assert_eq!(parsed.range.get(input), "é😀 x");
    assert_eq!(parsed.range.to_utf16(input), (4, 9));
    assert_eq!(utf16_offset(input, input.len()), 9);
}

#[test]
fn tree_is_plain_data() {
    fn shareable<T: Send + Sync>() {}
    shareable::<CommandTree<Key>>();
}
