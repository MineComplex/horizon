//! `/tick` (`TickCommand`) over `TickRateManager`'s `command_*` methods, which hold its logic.
//! The manager queues what Java does to the world as it goes (packets to every player, the sprint
//! report from the server's own source); [`Server::drain_tick_rate_events`] carries those out, before
//! the command's own feedback, as the manager's calls do in Java.

use super::source::{string_arg, translatable};
use super::tree::Key;
use super::{CommandSource, Server};
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::{CommandContext, CommandError};
use crate::network::play::clientbound as cb;
use crate::server::tick_rate::{CommandOutcome, FeedbackArg, TickCommandError, TickRateEvent};

impl Server {
    /// Carries out the events of the tick rate manager: `ClientboundTickingState/StepPacket` to every
    /// player and the sprint report from the server's own source.
    pub fn drain_tick_rate_events(&mut self) {
        for event in self.rate.take_events() {
            match event {
                TickRateEvent::TickingState { tick_rate, frozen } => {
                    let packet = cb::ticking_state(tick_rate, frozen);
                    for j in &self.players {
                        j.peer.conn.send(packet.clone());
                    }
                }
                TickRateEvent::TickingStep {
                    frozen_ticks_to_run,
                } => {
                    let packet = cb::ticking_step(frozen_ticks_to_run);
                    for j in &self.players {
                        j.peer.conn.send(packet.clone());
                    }
                }
                TickRateEvent::SprintReport {
                    ticks_per_second,
                    milliseconds_per_tick,
                } => {
                    let source = self.console_source();
                    self.send_success(
                        &source,
                        translatable(
                            "commands.tick.sprint.report",
                            vec![
                                PrimitiveOrComponent::Integer(ticks_per_second),
                                string_arg(milliseconds_per_tick),
                            ],
                        ),
                        true,
                    );
                }
                // The autosave countdown it would shorten is not kept (see the runtime's docs).
                TickRateEvent::TickRateChanged => {}
            }
        }
    }
}

/// The events first, then the feedback in order, then the result.
fn finish(
    server: &mut Server,
    source: &CommandSource,
    outcome: CommandOutcome,
) -> Result<i32, CommandError> {
    server.drain_tick_rate_events();
    for feedback in outcome.feedback {
        let args = feedback
            .args
            .into_iter()
            .map(|arg| match arg {
                FeedbackArg::Int(n) => PrimitiveOrComponent::Integer(n),
                FeedbackArg::Text(text) => string_arg(text),
            })
            .collect();
        server.send_success(
            source,
            translatable(feedback.key, args),
            feedback.broadcast_to_ops,
        );
    }
    Ok(outcome.result)
}

fn failed(error: TickCommandError) -> CommandError {
    CommandError::new(error.key(), Vec::new())
}

pub(super) fn query(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    let average = server.tick_times.average_nanos(server.tick_count);
    let outcome = server
        .rate
        .command_query(average, server.tick_times.samples());
    finish(server, source, outcome)
}

pub(super) fn rate(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let outcome = server
        .rate
        .command_rate(context.float("rate").unwrap_or(20.0));
    finish(server, source, outcome)
}

pub(super) fn step(
    server: &mut Server,
    source: &CommandSource,
    ticks: i32,
) -> Result<i32, CommandError> {
    let outcome = server.rate.command_step(ticks).map_err(failed)?;
    finish(server, source, outcome)
}

pub(super) fn step_stop(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    let outcome = server.rate.command_step_stop().map_err(failed)?;
    finish(server, source, outcome)
}

pub(super) fn sprint(
    server: &mut Server,
    source: &CommandSource,
    ticks: i32,
) -> Result<i32, CommandError> {
    let outcome = server.rate.command_sprint(ticks);
    finish(server, source, outcome)
}

pub(super) fn sprint_stop(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    let outcome = server.rate.command_sprint_stop().map_err(failed)?;
    finish(server, source, outcome)
}

pub(super) fn freeze(
    server: &mut Server,
    source: &CommandSource,
    freeze: bool,
) -> Result<i32, CommandError> {
    let outcome = server.rate.command_freeze(freeze);
    finish(server, source, outcome)
}
