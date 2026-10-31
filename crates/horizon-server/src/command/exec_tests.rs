//! The executor over a small tree: order, forks, limits, errors and callbacks.

use super::*;
use crate::command::{ArgumentType, CommandTree, NodeId, argument, literal};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Fork,
    Redirect,
    Run,
    Fail,
}

#[derive(Clone, Debug)]
struct Src {
    id: i32,
    callback: ResultCallback,
}

impl ExecSource for Src {
    fn callback(&self) -> &ResultCallback {
        &self.callback
    }
    fn with_callback(mut self, callback: ResultCallback) -> Self {
        self.callback = callback;
        self
    }
}

#[derive(Default)]
struct Host {
    ran: Vec<i32>,
    errors: Vec<(i32, String, bool)>,
    fork_width: i32,
}

impl<'p> Runner<'p, Key, Src> for Host {
    fn run(
        &mut self,
        key: Key,
        _: &CommandContext<'p, Key>,
        source: &Src,
    ) -> Result<i32, CommandError> {
        match key {
            Key::Fail => Err(CommandError::new("test.failed", vec![])),
            _ => {
                self.ran.push(source.id);
                Ok(source.id)
            }
        }
    }

    fn modify(
        &mut self,
        key: Key,
        _: &CommandContext<'p, Key>,
        source: &Src,
    ) -> Result<Vec<Src>, CommandError> {
        match key {
            // Each source becomes `fork_width` sources: id * 10 + 1, id * 10 + 2...
            Key::Fork => Ok((1..=self.fork_width)
                .map(|n| Src {
                    id: source.id * 10 + n,
                    callback: source.callback.clone(),
                })
                .collect()),
            Key::Redirect => Err(CommandError::new("test.modifier", vec![])),
            _ => Ok(vec![source.clone()]),
        }
    }

    fn handle_error(&mut self, source: &Src, error: &CommandError, forked: bool) {
        self.errors
            .push((source.id, error.key().to_owned(), forked));
    }
}

fn tree() -> CommandTree<Key> {
    let mut tree = CommandTree::new();
    tree.register(literal("fork").fork(NodeId::ROOT, Key::Fork));
    tree.register(literal("redirect").redirect_with(NodeId::ROOT, Key::Redirect));
    tree.register(literal("fork_bad").fork(NodeId::ROOT, Key::Redirect));
    tree.register(literal("run").executes(Key::Run));
    tree.register(literal("fail").executes(Key::Fail));
    tree.register(literal("n").then(argument("x", ArgumentType::integer()).executes(Key::Run)));
    tree
}

fn run(input: &str, limit: usize, forks: usize, width: i32) -> (Host, Option<(bool, i32)>) {
    let tree = tree();
    let parse = tree.parse(input, |_| true);
    let chain = parse.context_chain().expect("parses");
    let mut context = ExecutionContext::new(limit, forks);
    let slot = context.new_slot();
    let source = Src {
        id: 1,
        callback: slot.clone(),
    };
    let mut host = Host {
        fork_width: width,
        ..Host::default()
    };
    context.queue_initial_command(input, &chain, source, ResultCallback::Empty);
    context.run(&mut host);
    let outcome = context.outcome(&slot);
    (host, outcome)
}

#[test]
fn a_plain_command_runs_once_and_reports() {
    let (host, outcome) = run("run", 100, 100, 1);
    assert_eq!(host.ran, [1]);
    assert_eq!(outcome, Some((true, 1)));
}

#[test]
fn forks_multiply_the_sources_in_order() {
    let (host, _) = run("fork fork run", 100, 100, 2);
    assert_eq!(host.ran, [111, 112, 121, 122]);
}

#[test]
fn a_failing_command_reports_false_and_tells_the_source() {
    let (host, outcome) = run("fail", 100, 100, 1);
    assert_eq!(outcome, Some((false, 0)));
    assert_eq!(host.errors, [(1, "test.failed".to_owned(), false)]);
}

#[test]
fn a_failing_modifier_stops_a_redirect_but_not_a_fork() {
    let (host, outcome) = run("redirect run", 100, 100, 1);
    assert!(host.ran.is_empty());
    assert_eq!(outcome, Some((false, 0)));
    assert_eq!(host.errors, [(1, "test.modifier".to_owned(), false)]);
    // A forked chain tells nobody, and goes on with the sources it has (none here).
    let (host, _) = run("fork_bad run", 100, 100, 1);
    assert!(host.ran.is_empty());
    assert_eq!(host.errors, [(1, "test.modifier".to_owned(), true)]);
}

#[test]
fn the_command_limit_stops_the_line() {
    // Two forks (cost 1 each) and then 4 commands: a limit of 5 runs three of them.
    let (host, _) = run("fork fork run", 5, 100, 2);
    assert_eq!(host.ran, [111, 112, 121]);
}

#[test]
fn the_fork_limit_is_an_error_for_the_original_source() {
    let (host, _) = run("fork fork run", 100, 4, 3);
    assert!(host.ran.is_empty());
    assert_eq!(host.errors, [(1, "command.forkLimit".to_owned(), true)]);
}

#[test]
fn an_argument_command_runs() {
    let (host, outcome) = run("n 5", 10, 10, 1);
    assert_eq!(host.ran, [1]);
    assert_eq!(outcome, Some((true, 1)));
}

#[test]
fn callbacks_chain() {
    let a = ResultCallback::Slot(0);
    let b = ResultCallback::Slot(1);
    assert_eq!(ResultCallback::chain(ResultCallback::Empty, b.clone()), b);
    assert_eq!(ResultCallback::chain(a.clone(), ResultCallback::Empty), a);
    assert!(matches!(
        ResultCallback::chain(a, b),
        ResultCallback::Chain(..)
    ));
}
