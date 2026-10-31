//! The command executor: `net.minecraft.commands.execution` (`ExecutionContext`, `Frame`,
//! `CommandQueueEntry`, `ChainModifiers`, `BuildContexts`, `ContinuationTask`, `ExecuteCommand`,
//! `FallthroughTask`) and `Commands.executeCommandInContext`, as plain data.
//!
//! A command line is parsed, flattened into a [`ContextChain`] (modifier stages, then the stage that
//! runs a command) and queued as one entry. The queue runs entry by entry; an entry may queue more, and
//! what an entry queues runs before what was queued earlier (`pushNewCommands` puts the batch in front),
//! so a chain of redirects runs depth first, in vanilla's order. Each executed command and each
//! modifier call costs one of the `commandLimit` commands of the whole line
//! (`maxCommandSequenceLength`); a modifier may not yield `forkLimit` sources (`maxCommandForks`).
//!
//! Nothing here stores a closure: what Brigadier keeps as lambdas is the tree's key `K`, and the
//! server's [`Runner`] runs it. Result callbacks (`CommandResultCallback`) are [`ResultCallback`]
//! values the context writes into when they fire.

use std::collections::VecDeque;

use super::{CommandContext, CommandError, ContextChain};
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;

/// `MAX_QUEUE_DEPTH`.
const MAX_QUEUE_DEPTH: usize = 10_000_000;

/// `CommandResultCallback`: what to tell when a command completes. `Slot` records the outcome in the
/// context's results (what `/execute store` and the caller of a function read); the chain calls
/// both in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ResultCallback {
    #[default]
    Empty,
    Slot(usize),
    Chain(Box<ResultCallback>, Box<ResultCallback>),
}

impl ResultCallback {
    /// `CommandResultCallback.chain`.
    pub fn chain(first: Self, second: Self) -> Self {
        match (first, second) {
            (Self::Empty, second) => second,
            (first, Self::Empty) => first,
            (first, second) => Self::Chain(Box::new(first), Box::new(second)),
        }
    }
}

/// A source of commands: what the executor needs of `CommandSourceStack` (`ExecutionCommandSource`).
pub trait ExecSource: Clone {
    fn callback(&self) -> &ResultCallback;
    fn with_callback(self, callback: ResultCallback) -> Self;
}

/// What the server does for the executor: run a command, apply a redirect's modifier, report an
/// error. `K` is the tree's key.
pub trait Runner<'p, K: Copy, S: ExecSource> {
    /// `Command.run`: the command's result, or its error.
    fn run(
        &mut self,
        key: K,
        context: &CommandContext<'p, K>,
        source: &S,
    ) -> Result<i32, CommandError>;

    /// `RedirectModifier.apply`: the sources the next stage runs for.
    fn modify(
        &mut self,
        key: K,
        context: &CommandContext<'p, K>,
        source: &S,
    ) -> Result<Vec<S>, CommandError>;

    /// `ExecutionCommandSource.handleError` (called for every error; `forked` is vanilla's argument, and
    /// a forked chain does not tell the source).
    fn handle_error(&mut self, source: &S, error: &CommandError, forked: bool);
}

/// `ChainModifiers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainModifiers {
    flags: u8,
}

impl ChainModifiers {
    const FORKED: u8 = 1;
    const RETURN: u8 = 2;

    pub fn is_forked(self) -> bool {
        self.flags & Self::FORKED != 0
    }
    pub fn set_forked(self) -> Self {
        Self {
            flags: self.flags | Self::FORKED,
        }
    }
    pub fn is_return(self) -> bool {
        self.flags & Self::RETURN != 0
    }
    pub fn set_return(self) -> Self {
        Self {
            flags: self.flags | Self::RETURN,
        }
    }
}

/// `Frame`: how deep the entry is (a function call is one deeper) and what a `return` tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub depth: usize,
    pub return_value: ResultCallback,
}

/// `CommandQueueEntry`'s action, with the data each task of vanilla holds.
enum Action<'p, K: Copy, S> {
    /// `BuildContexts.TopLevel` (one source, default modifiers) and `.Continuation`: run the modifier
    /// stages from `stage`, then queue the command for each source left.
    Build {
        chain: &'p ContextChain<'p, 'p, K>,
        input: &'p str,
        stage: usize,
        modifiers: ChainModifiers,
        original: S,
        sources: Vec<S>,
    },
    /// `ContinuationTask` over `ExecuteCommand`: the command for `sources[index]`, then the rest.
    ExecuteEach {
        input: &'p str,
        modifiers: ChainModifiers,
        context: &'p CommandContext<'p, K>,
        sources: Vec<S>,
        index: usize,
    },
    /// `FallthroughTask`: a `return` whose sources ran out.
    Fallthrough,
}

struct QueueEntry<'p, K: Copy, S> {
    frame: Frame,
    action: Action<'p, K, S>,
}

/// `ExecutionContext`.
pub struct ExecutionContext<'p, K: Copy, S> {
    command_limit: usize,
    fork_limit: usize,
    command_quota: isize,
    queue_overflow: bool,
    queue: VecDeque<QueueEntry<'p, K, S>>,
    new_top_commands: Vec<QueueEntry<'p, K, S>>,
    current_frame_depth: usize,
    results: Vec<Option<(bool, i32)>>,
}

impl<'p, K: Copy, S: ExecSource> ExecutionContext<'p, K, S> {
    /// `new ExecutionContext(commandLimit, forkLimit)`.
    pub fn new(command_limit: usize, fork_limit: usize) -> Self {
        Self {
            command_limit,
            fork_limit,
            command_quota: isize::try_from(command_limit).unwrap_or(isize::MAX),
            queue_overflow: false,
            queue: VecDeque::new(),
            new_top_commands: Vec::new(),
            current_frame_depth: 0,
            results: Vec::new(),
        }
    }

    /// A callback that records its outcome; read it with [`Self::outcome`].
    pub fn new_slot(&mut self) -> ResultCallback {
        self.results.push(None);
        ResultCallback::Slot(self.results.len() - 1)
    }

    /// `(success, result)` the slot was told last, if it was.
    pub fn outcome(&self, callback: &ResultCallback) -> Option<(bool, i32)> {
        match callback {
            ResultCallback::Slot(i) => self.results[*i],
            _ => None,
        }
    }

    fn fire(&mut self, callback: &ResultCallback, success: bool, result: i32) {
        match callback {
            ResultCallback::Empty => {}
            ResultCallback::Slot(i) => self.results[*i] = Some((success, result)),
            ResultCallback::Chain(first, second) => {
                self.fire(first, success, result);
                self.fire(second, success, result);
            }
        }
    }

    /// `createTopFrame`: depth 0, or one deeper when called from inside a running command.
    fn top_frame(&self, return_value: ResultCallback) -> Frame {
        Frame {
            depth: if self.current_frame_depth == 0 {
                0
            } else {
                self.current_frame_depth + 1
            },
            return_value,
        }
    }

    /// `queueInitialCommandExecution`.
    pub fn queue_initial_command(
        &mut self,
        input: &'p str,
        chain: &'p ContextChain<'p, 'p, K>,
        source: S,
        command_return: ResultCallback,
    ) {
        let frame = self.top_frame(command_return);
        self.queue_next(QueueEntry {
            frame,
            action: Action::Build {
                chain,
                input,
                stage: 0,
                modifiers: ChainModifiers::default(),
                original: source.clone(),
                sources: vec![source],
            },
        });
    }

    fn handle_queue_overflow(&mut self) {
        self.queue_overflow = true;
        self.new_top_commands.clear();
        self.queue.clear();
    }

    fn queue_next(&mut self, entry: QueueEntry<'p, K, S>) {
        if self.new_top_commands.len() + self.queue.len() > MAX_QUEUE_DEPTH {
            self.handle_queue_overflow();
        }
        if !self.queue_overflow {
            self.new_top_commands.push(entry);
        }
    }

    /// `discardAtDepthOrHigher`.
    fn discard_at_depth_or_higher(&mut self, depth: usize) {
        while self.queue.front().is_some_and(|e| e.frame.depth >= depth) {
            self.queue.pop_front();
        }
    }

    fn push_new_commands(&mut self) {
        for entry in self.new_top_commands.drain(..).rev() {
            self.queue.push_front(entry);
        }
    }

    /// `incrementCost`.
    fn increment_cost(&mut self) {
        self.command_quota -= 1;
    }

    /// `runCommandQueue`.
    pub fn run(&mut self, runner: &mut impl Runner<'p, K, S>) {
        self.push_new_commands();
        loop {
            if self.command_quota <= 0 {
                tracing::info!(
                    "Command execution stopped due to limit (executed {} commands)",
                    self.command_limit
                );
                break;
            }
            let Some(entry) = self.queue.pop_front() else {
                break;
            };
            self.current_frame_depth = entry.frame.depth;
            self.execute(entry, runner);
            if self.queue_overflow {
                tracing::error!(
                    "Command execution stopped due to command queue overflow (max {MAX_QUEUE_DEPTH})"
                );
                break;
            }
            self.push_new_commands();
        }
        self.current_frame_depth = 0;
    }

    fn execute(&mut self, entry: QueueEntry<'p, K, S>, runner: &mut impl Runner<'p, K, S>) {
        let QueueEntry { frame, action } = entry;
        match action {
            Action::Build {
                chain,
                input,
                stage,
                modifiers,
                original,
                sources,
            } => self.build(
                runner, frame, chain, input, stage, modifiers, original, sources,
            ),
            Action::ExecuteEach {
                input,
                modifiers,
                context,
                sources,
                index,
            } => {
                let source = sources[index].clone();
                let last = index + 1 >= sources.len();
                // The next source is queued behind this command's own entries (`ContinuationTask`).
                let next = (!last).then(|| QueueEntry {
                    frame: frame.clone(),
                    action: Action::ExecuteEach {
                        input,
                        modifiers,
                        context,
                        sources,
                        index: index + 1,
                    },
                });
                self.execute_command(runner, modifiers, context, &source);
                if let Some(next) = next {
                    self.queue_next(next);
                }
            }
            Action::Fallthrough => {
                // `FallthroughTask`: `frame.returnFailure(); frame.discard()`.
                let callback = frame.return_value.clone();
                self.fire(&callback, false, 0);
                self.discard_at_depth_or_higher(frame.depth);
            }
        }
    }

    /// `BuildContexts.execute`.
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        runner: &mut impl Runner<'p, K, S>,
        frame: Frame,
        chain: &'p ContextChain<'p, 'p, K>,
        input: &'p str,
        mut stage: usize,
        mut modifiers: ChainModifiers,
        original: S,
        mut sources: Vec<S>,
    ) {
        while stage < chain.modifiers.len() {
            let context = chain.modifiers[stage];
            if context.is_forked() {
                modifiers = modifiers.set_forked();
            }
            if let Some(key) = context.modifier() {
                self.increment_cost();
                let forked = modifiers.is_forked();
                let mut next = Vec::new();
                for source in &sources {
                    match runner.modify(key, context, source) {
                        Ok(new_sources) => {
                            if next.len() + new_sources.len() >= self.fork_limit {
                                let error = CommandError::new(
                                    "command.forkLimit",
                                    vec![PrimitiveOrComponent::Integer(
                                        i32::try_from(self.fork_limit).unwrap_or(i32::MAX),
                                    )],
                                );
                                runner.handle_error(&original, &error, forked);
                                return;
                            }
                            next.extend(new_sources);
                        }
                        Err(error) => {
                            // `runModifier`: the callback hears of the failure, a forked chain goes on.
                            let callback = source.callback().clone();
                            self.fire(&callback, false, 0);
                            runner.handle_error(source, &error, forked);
                            if !forked {
                                return;
                            }
                        }
                    }
                }
                sources = next;
            }
            stage += 1;
        }
        if sources.is_empty() {
            if modifiers.is_return() {
                self.queue_next(QueueEntry {
                    frame,
                    action: Action::Fallthrough,
                });
            }
            return;
        }
        if modifiers.is_return() {
            // The first source reports to the frame's `return` as well.
            let first = sources.swap_remove(0);
            let callback =
                ResultCallback::chain(first.callback().clone(), frame.return_value.clone());
            sources = vec![first.with_callback(callback)];
        }
        self.queue_next(QueueEntry {
            frame,
            action: Action::ExecuteEach {
                input,
                modifiers,
                context: chain.executable,
                sources,
                index: 0,
            },
        });
    }

    /// `ExecuteCommand.execute`: one command for one source.
    fn execute_command(
        &mut self,
        runner: &mut impl Runner<'p, K, S>,
        modifiers: ChainModifiers,
        context: &'p CommandContext<'p, K>,
        source: &S,
    ) {
        self.increment_cost();
        let Some(key) = context.command() else { return };
        let callback = source.callback().clone();
        match runner.run(key, context, source) {
            Ok(result) => self.fire(&callback, true, result),
            Err(error) => {
                self.fire(&callback, false, 0);
                runner.handle_error(source, &error, modifiers.is_forked());
            }
        }
    }
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
