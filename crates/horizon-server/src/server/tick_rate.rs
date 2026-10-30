//! The tick rate and the `/tick` state machine: vanilla's `TickRateManager` + `ServerTickRateManager`
//! (with Paper's silent sprint) and `TickCommand`'s results.
//!
//! Where Java sends packets or chat from inside the manager, this one queues [`TickRateEvent`]s that the
//! server drains ([`TickRateManager::take_events`]) and acts on, in order. Time is passed in, never read.
use std::time::Instant;

use crate::util::number_text::format_fixed;

/// `TickRateManager.MIN_TICKRATE`.
pub const MIN_TICK_RATE: f32 = 1.0;
/// `TickCommand.MAX_TICKRATE`: the `rate` argument's bounds are `1.0..=10000.0`.
pub const MAX_COMMAND_TICK_RATE: f32 = 10000.0;
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MILLI: i64 = 1_000_000;

/// What the Java manager does to the world outside it.
#[derive(Debug, Clone, PartialEq)]
pub enum TickRateEvent {
    /// Broadcast `ClientboundTickingStatePacket` (rate and frozen) to every player.
    TickingState { tick_rate: f32, frozen: bool },
    /// Broadcast `ClientboundTickingStepPacket` to every player.
    TickingStep { frozen_ticks_to_run: i32 },
    /// The server's own command source sends `commands.tick.sprint.report` as a success, broadcast to
    /// operators (`sendSuccess(..., true)`). Not queued for a silent (API) sprint (Paper).
    SprintReport {
        ticks_per_second: i32,
        milliseconds_per_tick: String,
    },
    /// `MinecraftServer.onTickRateChanged`: shorten the autosave countdown if
    /// [`super::tick_loop::next_autosave_interval`] is now smaller.
    TickRateChanged,
}

/// The server's tick rate, freeze/step state and sprint. One value, owned by the tick thread.
#[derive(Debug, Clone)]
pub struct TickRateManager {
    tick_rate: f32,
    nanos_per_tick: i64,
    frozen_ticks_to_run: i32,
    run_game_elements: bool,
    frozen: bool,
    remaining_sprint_ticks: i64,
    sprint_tick_start: Option<Instant>,
    sprint_time_spent: i64,
    scheduled_sprint_ticks: i64,
    previous_frozen: bool,
    silent: bool,
    events: Vec<TickRateEvent>,
}

impl Default for TickRateManager {
    fn default() -> Self {
        Self {
            tick_rate: 20.0,
            nanos_per_tick: NANOS_PER_SECOND / 20,
            frozen_ticks_to_run: 0,
            run_game_elements: true,
            frozen: false,
            remaining_sprint_ticks: 0,
            sprint_tick_start: None,
            sprint_time_spent: 0,
            scheduled_sprint_ticks: 0,
            previous_frozen: false,
            silent: false,
            events: Vec::new(),
        }
    }
}

impl TickRateManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The events queued since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<TickRateEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn tick_rate(&self) -> f32 {
        self.tick_rate
    }

    pub fn nanoseconds_per_tick(&self) -> i64 {
        self.nanos_per_tick
    }

    pub fn milliseconds_per_tick(&self) -> f32 {
        self.nanos_per_tick as f32 / NANOS_PER_MILLI as f32
    }

    /// Whether this tick runs game elements (entities, block ticks...): false while frozen and not stepping.
    pub fn runs_normally(&self) -> bool {
        self.run_game_elements
    }

    pub fn is_stepping_forward(&self) -> bool {
        self.frozen_ticks_to_run > 0
    }

    pub fn frozen_ticks_to_run(&self) -> i32 {
        self.frozen_ticks_to_run
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn is_sprinting(&self) -> bool {
        self.scheduled_sprint_ticks > 0
    }

    /// `TickRateManager.isEntityFrozen`: players and entities carrying a player keep moving.
    pub fn is_entity_frozen(&self, is_player: bool, player_passengers: usize) -> bool {
        !self.runs_normally() && !is_player && player_passengers == 0
    }

    /// `setTickRate`: at least [`MIN_TICK_RATE`] (`Math.max`, so NaN stays NaN and gives 0 ns).
    pub fn set_tick_rate(&mut self, rate: f32) {
        self.tick_rate = if rate.is_nan() {
            rate
        } else {
            rate.max(MIN_TICK_RATE)
        };
        self.nanos_per_tick = (NANOS_PER_SECOND as f64 / f64::from(self.tick_rate)) as i64;
        self.events.push(TickRateEvent::TickRateChanged);
        self.push_state();
    }

    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.push_state();
    }

    fn push_state(&mut self) {
        self.events.push(TickRateEvent::TickingState {
            tick_rate: self.tick_rate,
            frozen: self.frozen,
        });
    }

    fn push_step(&mut self) {
        self.events.push(TickRateEvent::TickingStep {
            frozen_ticks_to_run: self.frozen_ticks_to_run,
        });
    }

    /// `TickRateManager.tick`, at the start of every server tick that is not paused for being empty.
    pub fn tick(&mut self) {
        self.run_game_elements = !self.frozen || self.frozen_ticks_to_run > 0;
        if self.frozen_ticks_to_run > 0 {
            self.frozen_ticks_to_run -= 1;
        }
    }

    /// Runs `ticks` more ticks while frozen; false (nothing changes) when not frozen.
    pub fn step_game_if_paused(&mut self, ticks: i32) -> bool {
        if !self.frozen {
            return false;
        }
        self.frozen_ticks_to_run = ticks;
        self.push_step();
        true
    }

    pub fn stop_stepping(&mut self) -> bool {
        if self.frozen_ticks_to_run <= 0 {
            return false;
        }
        self.frozen_ticks_to_run = 0;
        self.push_step();
        true
    }

    pub fn stop_sprinting(&mut self) -> bool {
        if self.remaining_sprint_ticks <= 0 {
            return false;
        }
        self.finish_sprint();
        true
    }

    /// Sprints `ticks` ticks (unfreezing for the sprint); true when it replaced a running sprint.
    /// `silent` (Paper's API sprint) drops the report, and only applies when no sprint is running.
    pub fn request_game_to_sprint(&mut self, ticks: i32, silent: bool) -> bool {
        if !self.is_sprinting() {
            self.silent = silent;
        }
        let interrupted = self.remaining_sprint_ticks > 0;
        self.sprint_time_spent = 0;
        self.scheduled_sprint_ticks = i64::from(ticks);
        self.remaining_sprint_ticks = i64::from(ticks);
        self.previous_frozen = self.frozen;
        self.set_frozen(false);
        interrupted
    }

    fn finish_sprint(&mut self) {
        let completed = self.scheduled_sprint_ticks - self.remaining_sprint_ticks;
        let millis = (self.sprint_time_spent as f64).max(1.0) / NANOS_PER_MILLI as f64;
        let ticks_per_second = (1000.0 * completed as f64 / millis) as i32;
        let mspt = if completed == 0 {
            f64::from(self.milliseconds_per_tick())
        } else {
            millis / completed as f64
        };
        self.scheduled_sprint_ticks = 0;
        self.sprint_time_spent = 0;
        if !self.silent {
            self.events.push(TickRateEvent::SprintReport {
                ticks_per_second,
                milliseconds_per_tick: format_fixed(mspt, 2),
            });
        }
        self.silent = false;
        self.remaining_sprint_ticks = 0;
        self.set_frozen(self.previous_frozen);
        self.events.push(TickRateEvent::TickRateChanged);
    }

    /// `checkShouldSprintThisTick`, asked by the loop at `now` while [`Self::is_sprinting`]: true runs
    /// this tick as a sprint tick (no waiting); the tick after the last one ends the sprint and reports.
    pub fn check_should_sprint_this_tick(&mut self, now: Instant) -> bool {
        if !self.run_game_elements {
            return false;
        }
        if self.remaining_sprint_ticks > 0 {
            self.sprint_tick_start = Some(now);
            self.remaining_sprint_ticks -= 1;
            return true;
        }
        self.finish_sprint();
        false
    }

    /// `endTickWork`, after a sprint tick: adds its time to the sprint's.
    pub fn end_tick_work(&mut self, now: Instant) {
        if let Some(start) = self.sprint_tick_start {
            let spent = now.saturating_duration_since(start);
            self.sprint_time_spent += i64::try_from(spent.as_nanos()).unwrap_or(i64::MAX);
        }
    }

    /// `updateJoiningPlayer`: the two packets a joining player gets, in order.
    pub fn joining_player_packets(&self) -> [TickRateEvent; 2] {
        [
            TickRateEvent::TickingState {
                tick_rate: self.tick_rate,
                frozen: self.frozen,
            },
            TickRateEvent::TickingStep {
                frozen_ticks_to_run: self.frozen_ticks_to_run,
            },
        ]
    }

    /// `/tick rate <rate>` (`rate` already within the argument's bounds).
    pub fn command_rate(&mut self, rate: f32) -> CommandOutcome {
        self.set_tick_rate(rate);
        let text = format_fixed(f64::from(rate), 1);
        CommandOutcome::one(
            rate as i32,
            Feedback::broadcast("commands.tick.rate.success", vec![FeedbackArg::Text(text)]),
        )
    }

    /// `/tick query`: `average_tick_nanos` and `samples` are the server's
    /// ([`super::tick_loop::TickTimeSamples`]).
    pub fn command_query(&self, average_tick_nanos: i64, samples: &[i64]) -> CommandOutcome {
        let busy = nanos_to_millis_string(average_tick_nanos);
        let rate = FeedbackArg::Text(format_fixed(f64::from(self.tick_rate), 1));
        let mut feedback = Vec::new();
        if self.is_sprinting() {
            feedback.push(Feedback::quiet("commands.tick.status.sprinting", vec![]));
            feedback.push(Feedback::quiet(
                "commands.tick.query.rate.sprinting",
                vec![rate, FeedbackArg::Text(busy)],
            ));
        } else {
            let status = if self.frozen {
                "commands.tick.status.frozen"
            } else if self.nanos_per_tick < average_tick_nanos {
                "commands.tick.status.lagging"
            } else {
                "commands.tick.status.running"
            };
            feedback.push(Feedback::quiet(status, vec![]));
            let target = nanos_to_millis_string(self.nanos_per_tick);
            let args = vec![rate, FeedbackArg::Text(busy), FeedbackArg::Text(target)];
            feedback.push(Feedback::quiet("commands.tick.query.rate.running", args));
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        if !sorted.is_empty() {
            let at = |fraction: f64| {
                nanos_to_millis_string(sorted[(sorted.len() as f64 * fraction) as usize])
            };
            let args = vec![
                FeedbackArg::Text(nanos_to_millis_string(sorted[sorted.len() / 2])),
                FeedbackArg::Text(at(0.95)),
                FeedbackArg::Text(at(0.99)),
                FeedbackArg::Int(sorted.len() as i32),
            ];
            feedback.push(Feedback::quiet("commands.tick.query.percentiles", args));
        }
        CommandOutcome {
            result: self.tick_rate as i32,
            feedback,
        }
    }

    /// `/tick sprint <ticks>`. Drain [`Self::take_events`] before showing the feedback.
    pub fn command_sprint(&mut self, ticks: i32) -> CommandOutcome {
        let mut feedback = Vec::new();
        if self.request_game_to_sprint(ticks, false) {
            feedback.push(Feedback::broadcast(
                "commands.tick.sprint.stop.success",
                vec![],
            ));
        }
        feedback.push(Feedback::broadcast(
            "commands.tick.status.sprinting",
            vec![],
        ));
        CommandOutcome {
            result: 1,
            feedback,
        }
    }

    /// `/tick freeze` and `/tick unfreeze`: freezing also ends a sprint (with its report) and stepping.
    pub fn command_freeze(&mut self, freeze: bool) -> CommandOutcome {
        if freeze {
            if self.is_sprinting() {
                self.stop_sprinting();
            }
            if self.is_stepping_forward() {
                self.stop_stepping();
            }
        }
        self.set_frozen(freeze);
        let key = if freeze {
            "commands.tick.status.frozen"
        } else {
            "commands.tick.status.running"
        };
        CommandOutcome::one(i32::from(freeze), Feedback::broadcast(key, vec![]))
    }

    /// `/tick step [<ticks>]` (1 without the argument).
    pub fn command_step(&mut self, ticks: i32) -> Result<CommandOutcome, TickCommandError> {
        if !self.step_game_if_paused(ticks) {
            return Err(TickCommandError::StepNotFrozen);
        }
        Ok(CommandOutcome::one(
            1,
            Feedback::broadcast("commands.tick.step.success", vec![FeedbackArg::Int(ticks)]),
        ))
    }

    /// `/tick step stop`.
    pub fn command_step_stop(&mut self) -> Result<CommandOutcome, TickCommandError> {
        if !self.stop_stepping() {
            return Err(TickCommandError::StepStopNotStepping);
        }
        Ok(CommandOutcome::one(
            1,
            Feedback::broadcast("commands.tick.step.stop.success", vec![]),
        ))
    }

    /// `/tick sprint stop`.
    pub fn command_sprint_stop(&mut self) -> Result<CommandOutcome, TickCommandError> {
        if !self.stop_sprinting() {
            return Err(TickCommandError::SprintStopNotSprinting);
        }
        Ok(CommandOutcome::one(
            1,
            Feedback::broadcast("commands.tick.sprint.stop.success", vec![]),
        ))
    }
}

/// `TickCommand.nanosToMilisString`: `"%.1f"` of the milliseconds as a `float`.
fn nanos_to_millis_string(nanos: i64) -> String {
    format_fixed(f64::from(nanos as f32 / NANOS_PER_MILLI as f32), 1)
}

/// An argument of a translated feedback message.
#[derive(Debug, Clone, PartialEq)]
pub enum FeedbackArg {
    Int(i32),
    Text(String),
}

/// A `sendSuccess` of a translated message; `broadcast_to_ops` is its second argument.
#[derive(Debug, Clone, PartialEq)]
pub struct Feedback {
    pub key: &'static str,
    pub args: Vec<FeedbackArg>,
    pub broadcast_to_ops: bool,
}

impl Feedback {
    fn broadcast(key: &'static str, args: Vec<FeedbackArg>) -> Self {
        Self {
            key,
            args,
            broadcast_to_ops: true,
        }
    }

    fn quiet(key: &'static str, args: Vec<FeedbackArg>) -> Self {
        Self {
            key,
            args,
            broadcast_to_ops: false,
        }
    }
}

/// A `/tick` command's result value and its feedback, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutcome {
    pub result: i32,
    pub feedback: Vec<Feedback>,
}

impl CommandOutcome {
    fn one(result: i32, feedback: Feedback) -> Self {
        Self {
            result,
            feedback: vec![feedback],
        }
    }
}

/// `/tick`'s failures (`SimpleCommandExceptionType`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TickCommandError {
    #[error("commands.tick.step.fail")]
    StepNotFrozen,
    #[error("commands.tick.step.stop.fail")]
    StepStopNotStepping,
    #[error("commands.tick.sprint.stop.fail")]
    SprintStopNotSprinting,
}

impl TickCommandError {
    /// The message's translation key.
    pub fn key(self) -> &'static str {
        match self {
            Self::StepNotFrozen => "commands.tick.step.fail",
            Self::StepStopNotStepping => "commands.tick.step.stop.fail",
            Self::SprintStopNotSprinting => "commands.tick.sprint.stop.fail",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TickRateEvent::*;
    use super::*;
    use std::time::Duration;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn rate_and_nanos() {
        let mut m = TickRateManager::new();
        assert_eq!(
            (
                m.tick_rate(),
                m.nanoseconds_per_tick(),
                m.milliseconds_per_tick()
            ),
            (20.0, 50_000_000, 50.0)
        );
        m.set_tick_rate(0.5);
        assert_eq!(
            (m.tick_rate(), m.nanoseconds_per_tick()),
            (1.0, 1_000_000_000)
        );
        m.set_tick_rate(3.0);
        // (long)(1e9 / 3.0) truncates.
        assert_eq!(m.nanoseconds_per_tick(), 333_333_333);
        assert_eq!(
            m.take_events()[..2],
            [
                TickRateChanged,
                TickingState {
                    tick_rate: 1.0,
                    frozen: false
                }
            ]
        );
    }

    #[test]
    fn freeze_and_step() {
        let mut m = TickRateManager::new();
        assert!(!m.step_game_if_paused(3));
        m.set_frozen(true);
        m.tick();
        assert!(!m.runs_normally());
        assert!(m.is_entity_frozen(false, 0));
        assert!(!m.is_entity_frozen(true, 0));
        assert!(!m.is_entity_frozen(false, 1));
        assert!(m.step_game_if_paused(2));
        m.tick();
        assert!(m.runs_normally());
        assert_eq!(m.frozen_ticks_to_run(), 1);
        m.tick();
        assert!(m.runs_normally());
        m.tick();
        assert!(!m.runs_normally());
        assert!(!m.stop_stepping());
        assert_eq!(
            m.take_events(),
            [
                TickingState {
                    tick_rate: 20.0,
                    frozen: true
                },
                TickingStep {
                    frozen_ticks_to_run: 2
                }
            ]
        );
    }

    #[test]
    fn sprint_report() {
        let t0 = Instant::now();
        let mut m = TickRateManager::new();
        m.set_frozen(true);
        assert!(!m.request_game_to_sprint(3, false));
        assert!(m.is_sprinting() && !m.is_frozen());
        m.take_events();
        let mut now = t0;
        for _ in 0..3 {
            m.tick();
            assert!(m.check_should_sprint_this_tick(now));
            now += ms(4);
            m.end_tick_work(now);
            now += ms(1); // between end_tick_work and the next tick: not counted
        }
        m.tick();
        assert!(!m.check_should_sprint_this_tick(now));
        // 3 ticks in 12 ms: (int)(1000 * 3 / 12.0) = 250, 12 / 3 = 4.00.
        assert_eq!(
            m.take_events(),
            [
                SprintReport {
                    ticks_per_second: 250,
                    milliseconds_per_tick: "4.00".into()
                },
                TickingState {
                    tick_rate: 20.0,
                    frozen: true
                },
                TickRateChanged,
            ]
        );
        assert!(!m.is_sprinting() && m.is_frozen());
    }

    #[test]
    fn sprint_stopped_before_any_tick_reports_target_mspt() {
        let mut m = TickRateManager::new();
        m.request_game_to_sprint(100, false);
        m.take_events();
        assert!(m.stop_sprinting());
        // 0 completed ticks: (int)(1000 * 0 / 1e-6) = 0 and millisecondsPerTick().
        assert_eq!(
            m.take_events()[0],
            SprintReport {
                ticks_per_second: 0,
                milliseconds_per_tick: "50.00".into()
            }
        );
        assert!(!m.stop_sprinting());
    }

    #[test]
    fn silent_sprint_and_interruption() {
        let mut m = TickRateManager::new();
        assert!(!m.request_game_to_sprint(10, true));
        // A second request while sprinting keeps the first one's silence.
        assert!(m.request_game_to_sprint(5, false));
        m.take_events();
        m.stop_sprinting();
        assert!(
            !m.take_events()
                .iter()
                .any(|e| matches!(e, SprintReport { .. }))
        );
        m.request_game_to_sprint(5, false);
        m.stop_sprinting();
        assert!(
            m.take_events()
                .iter()
                .any(|e| matches!(e, SprintReport { .. }))
        );
    }

    #[test]
    fn commands() {
        let mut m = TickRateManager::new();
        assert_eq!(m.command_step(1), Err(TickCommandError::StepNotFrozen));
        assert_eq!(
            m.command_sprint_stop(),
            Err(TickCommandError::SprintStopNotSprinting)
        );
        let out = m.command_rate(33.35);
        assert_eq!(out.result, 33);
        assert_eq!(out.feedback[0].args, [FeedbackArg::Text("33.3".into())]);
        m.command_rate(20.0);
        let out = m.command_sprint(20);
        assert_eq!(out.feedback.len(), 1);
        let out = m.command_sprint(20);
        assert_eq!(out.feedback[0].key, "commands.tick.sprint.stop.success");
        m.take_events();
        let out = m.command_freeze(true);
        assert_eq!(
            (out.result, out.feedback[0].key),
            (1, "commands.tick.status.frozen")
        );
        assert!(matches!(m.take_events()[0], SprintReport { .. }));
        assert_eq!(
            m.command_step(5).unwrap().feedback[0].args,
            [FeedbackArg::Int(5)]
        );
        assert!(m.command_step_stop().is_ok());
        assert_eq!(
            m.command_step_stop(),
            Err(TickCommandError::StepStopNotStepping)
        );
        assert_eq!(
            TickCommandError::StepStopNotStepping.key(),
            "commands.tick.step.stop.fail"
        );
    }

    #[test]
    fn query() {
        let mut m = TickRateManager::new();
        let samples: Vec<i64> = (1..=100).map(|i| i * 1_000_000).collect();
        let out = m.command_query(60_000_000, &samples);
        assert_eq!(out.feedback[0].key, "commands.tick.status.lagging");
        let text = |s: &str| FeedbackArg::Text(s.into());
        assert_eq!(
            out.feedback[1].args,
            [text("20.0"), text("60.0"), text("50.0")]
        );
        // samples[50], samples[95], samples[99] of the sorted copy.
        assert_eq!(
            out.feedback[2].args,
            [
                text("51.0"),
                text("96.0"),
                text("100.0"),
                FeedbackArg::Int(100)
            ]
        );
        assert_eq!(
            m.command_query(50_000_000, &samples).feedback[0].key,
            "commands.tick.status.running"
        );
        m.set_frozen(true);
        assert_eq!(
            m.command_query(0, &samples).feedback[0].key,
            "commands.tick.status.frozen"
        );
    }
}
