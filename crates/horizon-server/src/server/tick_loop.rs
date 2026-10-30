//! The scheduling arithmetic of Leaf's `MinecraftServer.runServer` (Paper's "improve tick loop": a
//! fixed-period schedule with bounded catch-up, TPS/MSPT from windows of tick records; Purpur/Gale's
//! lagging flag over the 5 s TPS), vanilla's 100-sample tick times (`/tick query`, autosave while
//! sprinting) and `pause-when-empty-seconds`.
//!
//! No clock is read here: every call takes `now`, so the tick thread is a thin loop:
//!
//! ```text
//! loop {
//!     let sprinting = clock.start_tick(Instant::now(), &mut rate, false);
//!     run queued tasks and packets, then tick the server (see EmptyPause, TickTimeSamples)
//!     clock.end_tick(Instant::now());
//!     while let TickAction::Wait(d) = clock.next_action(Instant::now()) {
//!         run tasks until at most `d` has passed (clock.add_task_time(spent)) or park for `d`
//!     }
//!     if sprinting { rate.end_tick_work(Instant::now()) }
//! }
//! ```
//!
//! Paper removed vanilla's "Can't keep up! Is the server overloaded?" warning with this loop: Leaf logs
//! no overload warning. Being behind shows as [`TickClock::is_lagging`], `/tps` and `/mspt`.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::tick_rate::TickRateManager;

const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// The settings the loop reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickLoopConfig {
    /// Paper's `misc.catch-up-ticks` (`paper-global.yml`, `default` = 5; values below 1 count as 1): how
    /// many late ticks may still run back to back; older ones are skipped.
    pub catchup_ticks: i32,
    /// Purpur's `settings.lagging-threshold` (`purpur.yml`, 19.0): lagging below this 5 s TPS.
    pub lagging_threshold: f64,
}

impl Default for TickLoopConfig {
    fn default() -> Self {
        Self {
            catchup_ticks: 5,
            lagging_threshold: 19.0,
        }
    }
}

/// What the tick thread does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickAction {
    /// The next tick is due in this long: run tasks or sleep, then ask again.
    Wait(Duration),
    /// Start the next tick.
    Tick,
}

/// One finished tick (Spottedleaf's `TickTime`), times in ns since the clock's origin.
#[derive(Debug, Clone, Copy)]
struct TickRecord {
    previous_start: Option<i64>,
    start: i64,
    end: i64,
    /// Tasks run while waiting for this tick (added to its MSPT).
    task_time: i64,
}

impl TickRecord {
    fn time_from_last_tick(&self, interval: i64) -> i64 {
        match self.previous_start {
            Some(previous) => self.start - previous,
            None => interval.max(self.end - self.start),
        }
    }
}

/// The ticks of the last `span` ns (`TickData`): a record leaves once a newer tick starts more than
/// `span` after it ended.
#[derive(Debug, Clone)]
struct TickWindow {
    span: i64,
    records: VecDeque<TickRecord>,
}

impl TickWindow {
    fn new(seconds: i64) -> Self {
        Self {
            span: seconds * NANOS_PER_SECOND,
            records: VecDeque::new(),
        }
    }

    fn add(&mut self, record: TickRecord) {
        while self
            .records
            .front()
            .is_some_and(|first| record.start - first.end > self.span)
        {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    /// `MinecraftServer.getTPS(TickData, interval)`: ticks over the time between tick starts; with no
    /// data, the target rate.
    fn tps(&self, interval: i64) -> f64 {
        if self.records.is_empty() {
            return NANOS_PER_SECOND as f64 / interval as f64;
        }
        let total: i64 = self
            .records
            .iter()
            .map(|r| r.time_from_last_tick(interval))
            .sum();
        self.records.len() as f64 / (total as f64 / 1.0e9)
    }

    /// `generateTickReport(...).timePerTickData().segmentAll()` in ms.
    fn mspt(&self) -> Option<MsptStats> {
        let times = self.records.iter().map(|r| r.end - r.start + r.task_time);
        let (mut sum, mut min, mut max) = (0i64, i64::MAX, i64::MIN);
        for t in times {
            sum += t;
            min = min.min(t);
            max = max.max(t);
        }
        let n = self.records.len();
        (n > 0).then(|| MsptStats {
            average: sum as f64 / n as f64 * 1.0e-6,
            min: min as f64 * 1.0e-6,
            max: max as f64 * 1.0e-6,
        })
    }
}

/// Milliseconds per tick over a window: the time ticking plus the tasks run while waiting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MsptStats {
    pub average: f64,
    pub min: f64,
    pub max: f64,
}

/// When ticks start, and the TPS/MSPT they add up to.
#[derive(Debug, Clone)]
pub struct TickClock {
    config: TickLoopConfig,
    origin: Instant,
    /// `Schedule.lastPeriod`: when the latest tick was due.
    last_period: i64,
    next_tick_time: i64,
    this_tick_nanos: i64,
    last_tick_start: Option<i64>,
    current_tick_start: i64,
    task_time: i64,
    delayed_tasks_deadline: i64,
    lagging: bool,
    current_tick: i32,
    tps_5s: TickWindow,
    tps_10s: TickWindow,
    tps_1m: TickWindow,
    tps_5m: TickWindow,
    tps_15m: TickWindow,
}

impl TickClock {
    /// `initTickSchedule` at `start` (right after the levels are prepared): the first tick is due now.
    pub fn new(
        start: Instant,
        config: TickLoopConfig,
        rate: &TickRateManager,
        paused: bool,
    ) -> Self {
        let interval = if paused || !rate.is_sprinting() {
            rate.nanoseconds_per_tick()
        } else {
            0
        };
        Self {
            config,
            origin: start,
            last_period: -interval,
            next_tick_time: 0,
            this_tick_nanos: interval,
            last_tick_start: None,
            current_tick_start: 0,
            task_time: 0,
            delayed_tasks_deadline: 0,
            lagging: false,
            current_tick: 0,
            tps_5s: TickWindow::new(5),
            tps_10s: TickWindow::new(10),
            tps_1m: TickWindow::new(60),
            tps_5m: TickWindow::new(300),
            tps_15m: TickWindow::new(900),
        }
    }

    fn nanos(&self, t: Instant) -> i64 {
        i64::try_from(t.saturating_duration_since(self.origin).as_nanos()).unwrap_or(i64::MAX)
    }

    /// The top of a loop iteration at `now`: schedules this tick and returns whether it is a sprint tick
    /// (no wait after it; ticks run with no time budget). `paused` is the integrated server's pause,
    /// always false on a dedicated server.
    pub fn start_tick(&mut self, now: Instant, rate: &mut TickRateManager, paused: bool) -> bool {
        let start = self.nanos(now);
        let sprinting = !paused && rate.is_sprinting() && rate.check_should_sprint_this_tick(now);
        if sprinting {
            self.this_tick_nanos = 0;
            self.last_period = start;
        } else {
            let interval = rate.nanoseconds_per_tick();
            self.this_tick_nanos = interval;
            let behind = periods_ahead(self.last_period, interval, start).max(1);
            let catchup = i64::from(self.config.catchup_ticks.max(1));
            self.lagging = self.tps_including_5s(rate)[0] < self.config.lagging_threshold;
            if behind > catchup {
                self.last_period += (behind - catchup) * interval;
            }
            self.last_period += interval;
        }
        self.next_tick_time = self.last_period + self.this_tick_nanos;
        self.current_tick_start = start;
        self.current_tick = self.current_tick.wrapping_add(1);
        sprinting
    }

    /// `recordEndOfTick`, once the tick's work is done at `now`.
    pub fn end_tick(&mut self, now: Instant) {
        let end = self.nanos(now);
        let record = TickRecord {
            previous_start: self.last_tick_start,
            start: self.current_tick_start,
            end,
            task_time: self.task_time,
        };
        self.last_tick_start = Some(self.current_tick_start);
        self.task_time = 0;
        for window in [
            &mut self.tps_5s,
            &mut self.tps_10s,
            &mut self.tps_1m,
            &mut self.tps_5m,
            &mut self.tps_15m,
        ] {
            window.add(record);
        }
        self.delayed_tasks_deadline = (end + self.this_tick_nanos).max(self.next_tick_time);
    }

    /// Between ticks: how long until the next one is due (Paper never sleeps past it).
    pub fn next_action(&self, now: Instant) -> TickAction {
        let left = self.next_tick_time - self.nanos(now);
        if left <= 0 {
            TickAction::Tick
        } else {
            TickAction::Wait(Duration::from_nanos(left as u64))
        }
    }

    /// Time spent running tasks while waiting; it counts towards the next tick's MSPT.
    pub fn add_task_time(&mut self, spent: Duration) {
        self.task_time += i64::try_from(spent.as_nanos()).unwrap_or(i64::MAX);
    }

    /// `MinecraftServer.haveTime` (the budget levels and chunk tasks check): never during a sprint tick.
    pub fn have_time(&self, now: Instant, may_have_delayed_tasks: bool) -> bool {
        if self.this_tick_nanos == 0 {
            return false;
        }
        let deadline = if may_have_delayed_tasks {
            self.delayed_tasks_deadline
        } else {
            self.next_tick_time
        };
        self.nanos(now) < deadline
    }

    /// When the current tick was scheduled to end (Paper's `ServerTickEndEvent` time remaining).
    pub fn next_tick_time(&self) -> Instant {
        self.origin + Duration::from_nanos(self.next_tick_time.max(0) as u64)
    }

    /// Purpur's `MinecraftServer.lagging`, updated at the start of each non-sprint tick.
    pub fn is_lagging(&self) -> bool {
        self.lagging
    }

    /// Paper's `MinecraftServer.currentTick`: loop iterations, paused or not.
    pub fn current_tick(&self) -> i32 {
        self.current_tick
    }

    /// `getTPS()`: the 1 m, 5 m and 15 m averages.
    pub fn tps(&self, rate: &TickRateManager) -> [f64; 3] {
        let interval = rate.nanoseconds_per_tick();
        [
            self.tps_1m.tps(interval),
            self.tps_5m.tps(interval),
            self.tps_15m.tps(interval),
        ]
    }

    /// Gale's `getTPSIncluding5Seconds()`: 5 s, 1 m, 5 m, 15 m. What `/tps` shows, and Leaf's
    /// `getTPS()` while `misc.including-5s-in-get-tps` is on (its default).
    pub fn tps_including_5s(&self, rate: &TickRateManager) -> [f64; 4] {
        let interval = rate.nanoseconds_per_tick();
        let [one, five, fifteen] = self.tps(rate);
        [self.tps_5s.tps(interval), one, five, fifteen]
    }

    /// `/mspt`'s three windows: 5 s, 10 s and 1 m (None shows as 0.0).
    pub fn mspt(&self) -> [Option<MsptStats>; 3] {
        [self.tps_5s.mspt(), self.tps_10s.mspt(), self.tps_1m.mspt()]
    }
}

/// `Schedule.getPeriodsAhead`: whole periods from `last_period` to `time`, rounded towards zero.
fn periods_ahead(last_period: i64, period: i64, time: i64) -> i64 {
    let difference = time - last_period;
    let periods = difference.abs() / period;
    if difference >= 0 { periods } else { -periods }
}

/// Vanilla's last 100 `tickServer` durations (`tickTimesNanos`, `getAverageTickTimeNanos`).
#[derive(Debug, Clone)]
pub struct TickTimeSamples {
    times: [i64; 100],
    total: i64,
}

impl Default for TickTimeSamples {
    fn default() -> Self {
        Self {
            times: [0; 100],
            total: 0,
        }
    }
}

impl TickTimeSamples {
    /// Records the duration of tick number `tick_count` (already incremented for this tick).
    pub fn record(&mut self, tick_count: i32, nanos: i64) {
        let index = tick_count.rem_euclid(100) as usize;
        self.total = self.total - self.times[index] + nanos;
        self.times[index] = nanos;
    }

    pub fn average_nanos(&self, tick_count: i32) -> i64 {
        self.total / i64::from(tick_count.clamp(1, 100))
    }

    pub fn samples(&self) -> &[i64; 100] {
        &self.times
    }
}

/// `computeNextAutosaveInterval`: about 5 minutes of ticks, at least 100. Paper's autosave then counts
/// down from `autosavePeriod`; a tick rate change shortens the countdown to this if it is smaller.
pub fn next_autosave_interval(rate: &TickRateManager, average_tick_nanos: i64) -> i32 {
    let ticks_per_second = if rate.is_sprinting() {
        NANOS_PER_SECOND as f32 / (average_tick_nanos + 1) as f32
    } else {
        rate.tick_rate()
    };
    ((ticks_per_second * 300.0) as i32).max(100)
}

/// What `tickServer` does this tick under `pause-when-empty-seconds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyTick {
    /// Tick the game.
    Run,
    /// The server just became empty long enough: log `Server empty for {seconds} seconds, pausing`,
    /// autosave, then do the paused work.
    StartPause { seconds: i32 },
    /// Paused: only scheduler tasks, chunk unloads and connections; no game tick, tick count unchanged.
    Paused,
}

/// `pause-when-empty-seconds` (Paper/Leaf default -1: off; 0 or less never pauses).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmptyPause {
    seconds: i32,
    empty_ticks: i32,
}

impl EmptyPause {
    pub fn new(seconds: i32) -> Self {
        Self {
            seconds,
            empty_ticks: 0,
        }
    }

    /// At the top of `tickServer`. `blocked` is true while something (Paper: a plugin) keeps it awake.
    pub fn update(&mut self, player_count: usize, sprinting: bool, blocked: bool) -> EmptyTick {
        let threshold = self.seconds.wrapping_mul(20);
        if threshold <= 0 {
            return EmptyTick::Run;
        }
        if player_count == 0 && !sprinting && !blocked {
            self.empty_ticks = self.empty_ticks.wrapping_add(1);
        } else {
            self.empty_ticks = 0;
        }
        match self.empty_ticks.cmp(&threshold) {
            std::cmp::Ordering::Less => EmptyTick::Run,
            std::cmp::Ordering::Equal => EmptyTick::StartPause {
                seconds: self.seconds,
            },
            std::cmp::Ordering::Greater => EmptyTick::Paused,
        }
    }

    /// Paper's `isTickPaused`.
    pub fn is_paused(&self) -> bool {
        self.empty_ticks > 0 && self.empty_ticks >= self.seconds.wrapping_mul(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    /// Paper's loop with catch-up 5 at 20 TPS. Expected values printed by a Java 25 run of the same
    /// scenario over Paper's loop and Spottedleaf's `Schedule`/`TickData` arithmetic (transcribed).
    #[test]
    fn catch_up_lagging_and_averages_match_java() {
        let t0 = Instant::now();
        let at = |ns: u64| t0 + Duration::from_nanos(ns);
        let mut rate = TickRateManager::new();
        let mut clock = TickClock::new(t0, TickLoopConfig::default(), &rate, false);
        let mut work = [10 * MS; 200];
        work[2] = 400 * MS;
        work[10] = 120 * MS;
        work[11] = 120 * MS;
        work[12] = 120 * MS;
        work[30] = 49 * MS;
        let mut tasks = [0; 200];
        tasks[1] = 3 * MS;
        tasks[20] = 45 * MS;
        // (tick start, next tick time) in ms, from Java.
        let expected: [(u64, u64); 22] = [
            (0, 50),
            (50, 100),
            (100, 150),
            (500, 350),
            (510, 400),
            (520, 450),
            (530, 500),
            (540, 550),
            (550, 600),
            (600, 650),
            (650, 700),
            (770, 750),
            (890, 800),
            (1010, 850),
            (1020, 900),
            (1030, 950),
            (1040, 1000),
            (1050, 1050),
            (1060, 1100),
            (1100, 1150),
            (1150, 1200),
            (1205, 1250),
        ];
        let mut now = 0;
        for i in 0..200 {
            assert_eq!(clock.next_action(at(now)), TickAction::Tick);
            assert!(!clock.start_tick(at(now), &mut rate, false));
            assert_eq!(
                clock.is_lagging(),
                (4..=56).contains(&i),
                "lagging at tick {i}"
            );
            let next = clock.nanos(clock.next_tick_time()) as u64;
            if let Some(&(start, deadline)) = expected.get(i) {
                assert_eq!((now / MS, next / MS), (start, deadline), "tick {i}");
            }
            now += work[i];
            clock.end_tick(at(now));
            if let TickAction::Wait(left) = clock.next_action(at(now)) {
                assert_eq!(left, Duration::from_nanos(next - now));
                if tasks[i] > 0 {
                    now += tasks[i];
                    clock.add_task_time(Duration::from_nanos(tasks[i]));
                }
                now = now.max(next);
            }
        }
        assert_eq!(
            clock.tps_including_5s(&rate),
            [
                20.0,
                19.704433497536947,
                19.704433497536947,
                19.704433497536947
            ]
        );
        let stats = |average, min, max| Some(MsptStats { average, min, max });
        assert_eq!(
            clock.mspt(),
            [
                stats(10.0, 10.0, 10.0),
                stats(14.075757575757576, 10.0, 403.0),
                stats(14.035, 10.0, 403.0)
            ]
        );
        assert_eq!(clock.tps_10s.records.len(), 198);
        assert_eq!(clock.current_tick(), 200);
    }

    #[test]
    fn empty_windows_report_the_target() {
        let mut rate = TickRateManager::new();
        let clock = TickClock::new(Instant::now(), TickLoopConfig::default(), &rate, false);
        assert_eq!(clock.tps_including_5s(&rate), [20.0; 4]);
        assert_eq!(clock.mspt(), [None; 3]);
        rate.set_tick_rate(3.0);
        assert_eq!(clock.tps(&rate)[0], 1.0e9 / 333_333_333.0);
    }

    #[test]
    fn sprint_ticks_do_not_wait() {
        let t0 = Instant::now();
        let mut rate = TickRateManager::new();
        let mut clock = TickClock::new(t0, TickLoopConfig::default(), &rate, false);
        rate.request_game_to_sprint(2, false);
        let mut now = t0;
        for _ in 0..2 {
            rate.tick();
            assert!(clock.start_tick(now, &mut rate, false));
            assert!(!clock.have_time(now, false));
            now += Duration::from_millis(7);
            clock.end_tick(now);
            assert_eq!(clock.next_action(now), TickAction::Tick);
            rate.end_tick_work(now);
        }
        rate.tick();
        // The sprint is over: this tick reports it and runs on the normal schedule again, one period
        // after the last sprint tick's due time (its start, 7 ms ago): due 107 ms after t0.
        assert!(!clock.start_tick(now, &mut rate, false));
        assert!(!rate.is_sprinting());
        assert_eq!(
            clock.next_action(now),
            TickAction::Wait(Duration::from_millis(93))
        );
        assert!(clock.have_time(now, false));
    }

    #[test]
    fn negative_catchup_counts_as_one() {
        let t0 = Instant::now();
        let mut rate = TickRateManager::new();
        let config = TickLoopConfig {
            catchup_ticks: 0,
            ..TickLoopConfig::default()
        };
        let mut clock = TickClock::new(t0, config, &rate, false);
        clock.start_tick(t0, &mut rate, false);
        let late = t0 + Duration::from_millis(1000);
        clock.end_tick(late);
        // 20 periods behind, at most 1 kept: this tick counts as due at 1000 ms, the next at 1050 ms.
        clock.start_tick(late, &mut rate, false);
        assert_eq!(clock.next_tick_time(), t0 + Duration::from_millis(1050));
        assert_eq!(periods_ahead(100, 50, 26), -1);
    }

    #[test]
    fn tick_time_samples_and_autosave() {
        let mut samples = TickTimeSamples::default();
        for tick in 1..=150 {
            samples.record(tick, i64::from(tick) * 1_000);
        }
        // Ticks 51..=150 remain.
        assert_eq!(
            samples.average_nanos(150),
            (51..=150).sum::<i64>() * 1_000 / 100
        );
        let mut fresh = TickTimeSamples::default();
        fresh.record(1, 30);
        assert_eq!(fresh.average_nanos(1), 30);
        assert_eq!(fresh.average_nanos(0), 30);
        let mut rate = TickRateManager::new();
        assert_eq!(next_autosave_interval(&rate, 0), 6000);
        rate.set_tick_rate(0.2);
        assert_eq!(next_autosave_interval(&rate, 0), 300);
        rate.request_game_to_sprint(10, true);
        // 1e9 / 5_000_001 ticks per second.
        assert_eq!(next_autosave_interval(&rate, 5_000_000), 59999);
        assert_eq!(next_autosave_interval(&rate, 1_000_000_000), 300);
        assert_eq!(next_autosave_interval(&rate, 10_000_000_000), 100);
    }

    #[test]
    fn pause_when_empty() {
        assert_eq!(EmptyPause::new(-1).update(0, false, false), EmptyTick::Run);
        let mut pause = EmptyPause::new(1);
        for _ in 0..19 {
            assert_eq!(pause.update(0, false, false), EmptyTick::Run);
        }
        assert!(!pause.is_paused());
        assert_eq!(
            pause.update(0, false, false),
            EmptyTick::StartPause { seconds: 1 }
        );
        assert!(pause.is_paused());
        assert_eq!(pause.update(0, false, false), EmptyTick::Paused);
        assert_eq!(pause.update(1, false, false), EmptyTick::Run);
        assert!(!pause.is_paused());
        assert_eq!(pause.update(0, true, false), EmptyTick::Run);
        assert_eq!(pause.update(0, false, true), EmptyTick::Run);
    }
}
