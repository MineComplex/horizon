//! Moonrise's chunk sending (`RegionizedPlayerChunkLoader`): the distances and the rate limiter,
//! ported as pure functions and values. The queues and tickets around them belong to the chunk
//! system; the rules are written down in the module docs of [`super`].

/// `MoonriseConstants.MAX_VIEW_DISTANCE` (the `Paper.MaxViewDistance` property's default).
pub const MAX_VIEW_DISTANCE: i32 = 32;
/// `PlayerChunkLoaderData.MAX_RATE`: chunks per second when a rate is unlimited.
pub const MAX_RATE: f64 = 10_000.0;

/// A level's or a player's `ViewDistances` (-1: unset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewDistances {
    pub tick: i32,
    /// "View distance + 1".
    pub load: i32,
    pub send: i32,
}

impl ViewDistances {
    pub const UNSET: Self = Self {
        tick: -1,
        load: -1,
        send: -1,
    };

    /// A level's distances from `server.properties` (`view-distance`, `simulation-distance`):
    /// `ChunkMap.setServerViewDistance` clamps the view distance to 2..=32 and loads one more,
    /// `DistanceManager.updateSimulationDistance` clamps the tick distance to 0..=32; the send
    /// distance is only set through the API.
    pub fn for_level(view_distance: i32, simulation_distance: i32) -> Self {
        Self {
            tick: simulation_distance.clamp(0, MAX_VIEW_DISTANCE),
            load: view_distance.clamp(2, MAX_VIEW_DISTANCE) + 1,
            send: -1,
        }
    }
}

/// The distances a player's loader uses (`PlayerChunkLoaderData.add`/`update`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerDistances {
    pub tick: i32,
    pub load: i32,
    pub send: i32,
}

/// `getTickDistance`, `getLoadViewDistance`, `getSendViewDistance`. `client_view_distance` is the
/// client information's view distance (`max(0, vd)`, -1 when unknown); `auto_send` is Paper's
/// `chunk-loading-advanced.auto-config-send-distance` (default true).
pub fn player_distances(
    player: ViewDistances,
    level: ViewDistances,
    client_view_distance: i32,
    auto_send: bool,
) -> PlayerDistances {
    let pick = |p: i32, l: i32| if p < 0 { l } else { p };
    let tick = pick(player.tick, level.tick).min(pick(player.load, level.load) - 1);
    let load = (tick + 1).max(pick(player.load, level.load));
    let wanted = if player.send >= 0 {
        player.send
    } else if !auto_send || client_view_distance < 0 {
        if level.send < 0 { load - 1 } else { level.send }
    } else {
        client_view_distance + 1
    };
    PlayerDistances {
        tick,
        load,
        send: (load - 1).min(wanted),
    }
}

/// What the login packet and the API report for a level (`getAPIViewDistance`,
/// `getAPITickDistance`): `(view distance, simulation distance)`.
pub fn api_distances(level: ViewDistances) -> (i32, i32) {
    let d = player_distances(ViewDistances::UNSET, level, -1, false);
    (d.load - 1, d.tick)
}

/// `ChunkTrackingView.isWithinDistance(..., includeNeighbors = true)`.
pub fn is_within_distance(center: (i32, i32), view_distance: i32, chunk: (i32, i32)) -> bool {
    let dx = i64::from(((chunk.0 - center.0).abs() - 2).max(0));
    let dz = i64::from(((chunk.1 - center.1).abs() - 2).max(0));
    dx * dx + dz * dz < i64::from(view_distance) * i64::from(view_distance)
}

/// `wantChunkSent`: within the square of `send + 1` and the vanilla view circle.
pub fn want_chunk_sent(center: (i32, i32), send: i32, chunk: (i32, i32)) -> bool {
    let square = (center.0 - chunk.0).abs().max((center.1 - chunk.1).abs());
    square <= send + 1 && is_within_distance(center, send, chunk)
}

/// `getMaxChunkSendRate` (and the load/generate rates): a configured rate `<= 0` or above
/// [`MAX_RATE`] is [`MAX_RATE`], otherwise at least 1. Paper's defaults: send 75, load 100,
/// generate -1 (unlimited).
pub fn effective_rate(configured: f64) -> f64 {
    if configured <= 0.0 || configured > MAX_RATE {
        MAX_RATE
    } else {
        configured.max(1.0)
    }
}

/// Moonrise's `AllocatingRateLimiter`.
#[derive(Clone, Debug)]
struct Allocating {
    rate: f64,
    interval_ns: i64,
    max_allocation: f64,
    take_carry: f64,
    allocation: f64,
    last_update: i64,
}

impl Allocating {
    fn max_allocation(rate: f64, interval_ns: i64) -> f64 {
        rate * (interval_ns as f64 / 1.0E9)
    }
    fn reset(&mut self, rate: f64, allocation: f64) {
        self.rate = rate;
        self.max_allocation = Self::max_allocation(rate, self.interval_ns);
        self.take_carry = 0.0;
        self.allocation = allocation.min(self.max_allocation);
    }
    fn tick(&mut self, time: i64) {
        let diff = (time - self.last_update).max(0);
        self.last_update = time;
        self.allocation = self
            .max_allocation
            .min(self.allocation + self.rate * (diff as f64 / 1.0E9));
    }
    fn preview(&self, max_take: i64) -> i64 {
        if max_take < 0 {
            return 0;
        }
        (max_take as f64)
            .min(self.allocation + self.take_carry)
            .floor() as i64
    }
    fn take(&mut self, max_take: i64) -> i64 {
        if max_take < 0 {
            return 0;
        }
        let mut ret = self.take_carry;
        let from_allocation = (max_take as f64 - ret).min(self.allocation);
        self.allocation -= from_allocation;
        ret += from_allocation;
        let whole = ret.floor();
        self.take_carry = ret - whole;
        whole as i64
    }
    fn return_unused(&mut self, unused: i64) {
        if unused <= 0 {
            return;
        }
        let new =
            (self.max_allocation + 1.0).min(self.allocation + self.take_carry + unused as f64);
        self.allocation = (new - 1.0).min(self.max_allocation);
        self.take_carry = 0.999_999_999_999_9;
    }
}

/// Moonrise's `StaggeredRateLimiter` with the chunk loader's windows (`createChunkLimiter`):
/// 4x the rate over 100 ms, 2x over 500 ms, 1.25x over 1 s, 1x over 3 s. Times are nanoseconds
/// from any fixed origin (`System.nanoTime`).
#[derive(Clone, Debug)]
pub struct ChunkRateLimiter {
    limiters: [(f64, Allocating); 4],
    rate: f64,
}

impl ChunkRateLimiter {
    pub fn new(now_ns: i64) -> Self {
        let window = |ms: i64, factor: f64| {
            (
                factor,
                Allocating {
                    rate: 0.0,
                    interval_ns: ms * 1_000_000,
                    max_allocation: 0.0,
                    take_carry: 0.0,
                    allocation: 0.0,
                    last_update: now_ns,
                },
            )
        };
        Self {
            limiters: [
                window(100, 4.0),
                window(500, 2.0),
                window(1_000, 1.25),
                window(3_000, 1.0),
            ],
            rate: 0.0,
        }
    }

    /// `tick(time, rate, 0.0)` (the loader's `INITIAL_ALLOCATION_FACTOR`): a changed rate
    /// empties every window, then each window accrues the time since its last tick.
    pub fn tick(&mut self, now_ns: i64, rate: f64) {
        if rate != self.rate {
            self.rate = rate;
            for (factor, l) in &mut self.limiters {
                l.reset(*factor * rate, 0.0);
            }
        }
        for (_, l) in &mut self.limiters {
            l.tick(now_ns);
        }
    }

    /// `takeAllocation`: how many of `max_take` may go now (the smallest window's allowance).
    pub fn take(&mut self, max_take: i64) -> i64 {
        let take = self
            .limiters
            .iter()
            .map(|(_, l)| l.preview(max_take))
            .min()
            .unwrap_or(0);
        for (_, l) in &mut self.limiters {
            let to_take = if take == l.preview(take + 1) {
                take + 1
            } else {
                take
            };
            let taken = l.take(to_take);
            debug_assert_eq!(taken, take, "a window gave a different allocation");
        }
        take
    }

    /// `returnUnused` (generation only: sends are never returned).
    pub fn return_unused(&mut self, unused: i64) {
        for (_, l) in &mut self.limiters {
            l.return_unused(unused);
        }
    }
}
