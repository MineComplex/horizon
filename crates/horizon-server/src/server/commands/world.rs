//! `/difficulty`, `/time` and `/weather` (`DifficultyCommand`, `TimeCommand`, `WeatherCommand`).
//!
//! The commands act on the source's level (`source.getLevel()`): Paper keeps difficulty, clocks and
//! weather per level. Time markers (`/time set day`) come from the `minecraft:timeline` registry as
//! `ServerClockManager.init` registers them. Not ported: `/time of <clock>` and the timeline
//! queries (registry arguments), Paper's `ClockTimeSkipEvent` (no listeners).

use super::source::{string_arg, text_arg, translatable};
use super::tree::Key;
use super::{CommandSource, Server};
use crate::azalea::chat::FormattedText;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::{CommandContext, CommandError};
use crate::network::play::clientbound as cb;
use crate::world::level_storage::Difficulty;

/// `Difficulty.getDisplayName`.
fn difficulty_name(difficulty: Difficulty) -> FormattedText {
    translatable(
        &format!("options.difficulty.{}", difficulty.name()),
        Vec::new(),
    )
}

fn difficulty_by_id(id: u8) -> Difficulty {
    [
        Difficulty::Peaceful,
        Difficulty::Easy,
        Difficulty::Normal,
        Difficulty::Hard,
    ][usize::from(id.min(3))]
}

pub(super) fn difficulty_query(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    let difficulty = server.world.levels[source.level].overrides.difficulty;
    server.send_success(
        source,
        translatable(
            "commands.difficulty.query",
            vec![text_arg(difficulty_name(difficulty))],
        ),
        false,
    );
    Ok(difficulty as i32)
}

/// `DifficultyCommand.setDifficulty`: `MinecraftServer.setDifficulty(level, difficulty, source, true)`
/// (Paper: a hardcore level stays hard; the overworld's change is also `level.dat`'s), then the
/// level's players are told.
pub(super) fn difficulty_set(
    server: &mut Server,
    source: &CommandSource,
    id: u8,
) -> Result<i32, CommandError> {
    let difficulty = difficulty_by_id(id);
    let current = server.world.levels[source.level].overrides.difficulty;
    if current == difficulty {
        return Err(CommandError::new(
            "commands.difficulty.failure",
            vec![text_arg(difficulty_name(difficulty))],
        ));
    }
    let index = source.level;
    let level = &mut server.world.levels[index];
    let effective = if level.overrides.hardcore {
        Difficulty::Hard
    } else {
        difficulty
    };
    level.overrides.set_difficulty(effective);
    let locked = level.overrides.difficulty_locked;
    if index == 0 {
        server.world.data.difficulty = effective;
    }
    let packet = cb::change_difficulty(effective as u8, locked);
    for j in server.players.iter().filter(|j| j.player.level == index) {
        j.peer.conn.send(packet.clone());
    }
    server.send_success(
        source,
        translatable(
            "commands.difficulty.success",
            vec![text_arg(difficulty_name(difficulty))],
        ),
        true,
    );
    Ok(0)
}

/// A time marker as `ClockTimeMarker`: the ticks in the period (`period_ticks` of the timeline, if
/// any), and whether `/time set` suggests it.
pub(super) struct TimeMarker {
    id: String,
    ticks: i64,
    period: Option<i64>,
    show_in_commands: bool,
}

impl TimeMarker {
    #[cfg(test)]
    pub(super) fn for_test(ticks: i64, period: Option<i64>) -> Self {
        Self {
            id: "minecraft:day".into(),
            ticks,
            period,
            show_in_commands: true,
        }
    }

    /// `occursAt`.
    pub(super) fn occurs_at(&self, total: i64) -> bool {
        match self.period {
            None => self.ticks == total,
            Some(period) => self.ticks == total % period,
        }
    }

    /// `resolveTimeToMoveTo`.
    pub(super) fn resolve(&self, total: i64) -> i64 {
        match self.period {
            None => self.ticks,
            Some(period) => {
                let duration = self.ticks - total % period;
                total
                    + if duration > 0 {
                        duration
                    } else {
                        period + duration
                    }
            }
        }
    }
}

impl Server {
    /// The markers `ServerClockManager.init` registers for `clock`: those of every timeline that
    /// runs on it (`Timeline.registerTimeMarkers`).
    fn time_markers(&self, clock: &str) -> Vec<TimeMarker> {
        let mut markers: Vec<TimeMarker> = Vec::new();
        let Some(timelines) = self.registries.synced("minecraft:timeline") else {
            return markers;
        };
        for entry in &timelines.entries {
            let json = &entry.json;
            if json["clock"].as_str() != Some(clock) {
                continue;
            }
            let period = json["period_ticks"].as_i64().filter(|p| *p > 0);
            let Some(map) = json["time_markers"].as_object() else {
                continue;
            };
            for (key, info) in map {
                let (ticks, show) = match info {
                    serde_json::Value::Number(n) => (n.as_i64().unwrap_or(0), false),
                    other => (
                        other["ticks"].as_i64().unwrap_or(0),
                        other["show_in_commands"].as_bool().unwrap_or(false),
                    ),
                };
                let id = if key.contains(':') {
                    key.clone()
                } else {
                    format!("minecraft:{key}")
                };
                markers.retain(|m| m.id != id);
                markers.push(TimeMarker {
                    id,
                    ticks,
                    period,
                    show_in_commands: show,
                });
            }
        }
        markers
    }

    /// `commandTimeMarkersForClock`: what `/time set` suggests.
    pub fn command_time_markers(&self, level: usize) -> Vec<String> {
        let Some(clock) = self.world.levels[level].default_clock.clone() else {
            return Vec::new();
        };
        self.time_markers(&clock)
            .into_iter()
            .filter(|m| m.show_in_commands)
            .map(|m| m.id)
            .collect()
    }

    /// `ServerClockManager.broadcastUpdates`: the clock's new state to the level's players.
    fn broadcast_clock(&self, index: usize, clock: &str) {
        let level = &self.world.levels[index];
        // The `advance_time` game rule is not ported: on.
        let Some(update) = level.clocks.network_state(clock, true) else {
            return;
        };
        let packet = cb::set_time(level.overrides.game_time, &[update]);
        for j in self.players.iter().filter(|j| j.player.level == index) {
            j.peer.conn.send(packet.clone());
        }
    }
}

/// `TimeCommand.getDefaultClock`: the clock of the source level's dimension type.
fn default_clock(server: &Server, source: &CommandSource) -> Result<String, CommandError> {
    let level = &server.world.levels[source.level];
    level.default_clock.clone().ok_or_else(|| {
        let name = server
            .registries
            .synced("minecraft:dimension_type")
            .and_then(|r| {
                usize::try_from(level.dimension_type)
                    .ok()
                    .and_then(|i| r.entries.get(i))
            })
            .map_or_else(|| level.dimension.clone(), |e| e.id.clone());
        CommandError::new("commands.time.no_default_clock", vec![string_arg(name)])
    })
}

/// `TimeCommand.wrapTime`.
fn wrap_time(ticks: i64) -> i32 {
    (ticks % 2_147_483_647) as i32
}

pub(super) fn time_query_game_time(
    server: &mut Server,
    source: &CommandSource,
) -> Result<i32, CommandError> {
    let game_time = server.world.levels[source.level].overrides.game_time;
    server.send_success(
        source,
        translatable(
            "commands.time.query.gametime",
            vec![PrimitiveOrComponent::Long(game_time)],
        ),
        false,
    );
    Ok(wrap_time(game_time))
}

pub(super) fn time_query(server: &mut Server, source: &CommandSource) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let total = server.world.levels[source.level]
        .clocks
        .state(&clock)
        .map_or(0, |s| s.total_ticks);
    server.send_success(
        source,
        translatable(
            "commands.time.query.absolute",
            vec![string_arg(clock), PrimitiveOrComponent::Long(total)],
        ),
        false,
    );
    Ok(wrap_time(total))
}

pub(super) fn time_set(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let new_total = i64::from(context.integer("time").unwrap_or(0));
    let clocks = &mut server.world.levels[source.level].clocks;
    let current = clocks.state(&clock).map_or(0, |s| s.total_ticks);
    if current == new_total {
        return Err(CommandError::new(
            "commands.time.set.already_at_time",
            vec![string_arg(clock), PrimitiveOrComponent::Long(new_total)],
        ));
    }
    clocks.set_total_ticks(&clock, new_total);
    server.broadcast_clock(source.level, &clock);
    server.send_success(
        source,
        translatable(
            "commands.time.set.absolute",
            vec![string_arg(clock), PrimitiveOrComponent::Long(new_total)],
        ),
        true,
    );
    Ok(wrap_time(new_total))
}

pub(super) fn time_add(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let time = i64::from(context.integer("time").unwrap_or(0));
    let clocks = &mut server.world.levels[source.level].clocks;
    clocks.add_ticks(&clock, time);
    let total = clocks.state(&clock).map_or(0, |s| s.total_ticks);
    server.broadcast_clock(source.level, &clock);
    server.send_success(
        source,
        translatable(
            "commands.time.set.absolute",
            vec![string_arg(clock), PrimitiveOrComponent::Long(total)],
        ),
        true,
    );
    Ok(wrap_time(total))
}

pub(super) fn time_set_marker(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let marker_id = context
        .identifier("timemarker")
        .unwrap_or_default()
        .to_owned();
    let markers = server.time_markers(&clock);
    let current = server.world.levels[source.level]
        .clocks
        .state(&clock)
        .map_or(0, |s| s.total_ticks);
    let Some(marker) = markers.iter().find(|m| m.id == marker_id) else {
        return Err(CommandError::new(
            "commands.time.no_time_marker_found",
            vec![string_arg(marker_id), string_arg(clock)],
        ));
    };
    // Paper: a move of 0 ticks (also "already there") is not a move.
    let target = marker.resolve(current);
    if marker.occurs_at(current) || target - current == 0 {
        return Err(CommandError::new(
            "commands.time.set.already_at_time_marker",
            vec![string_arg(clock), string_arg(marker_id)],
        ));
    }
    server.world.levels[source.level]
        .clocks
        .set_total_ticks(&clock, target);
    server.broadcast_clock(source.level, &clock);
    server.send_success(
        source,
        translatable(
            "commands.time.set.time_marker",
            vec![string_arg(clock), string_arg(marker_id)],
        ),
        true,
    );
    Ok(wrap_time(target))
}

pub(super) fn time_pause(
    server: &mut Server,
    source: &CommandSource,
    paused: bool,
) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let clocks = &mut server.world.levels[source.level].clocks;
    if clocks.state(&clock).is_some_and(|s| s.paused) == paused {
        let key = if paused {
            "commands.time.pause.already_paused"
        } else {
            "commands.time.pause.already_running"
        };
        return Err(CommandError::new(key, vec![string_arg(clock)]));
    }
    clocks.set_paused(&clock, paused);
    server.broadcast_clock(source.level, &clock);
    let key = if paused {
        "commands.time.pause"
    } else {
        "commands.time.resume"
    };
    server.send_success(source, translatable(key, vec![string_arg(clock)]), true);
    Ok(1)
}

pub(super) fn time_rate(
    server: &mut Server,
    source: &CommandSource,
    context: &CommandContext<'_, Key>,
) -> Result<i32, CommandError> {
    let clock = default_clock(server, source)?;
    let rate = context.float("rate").unwrap_or(1.0);
    let clocks = &mut server.world.levels[source.level].clocks;
    if clocks.state(&clock).is_some_and(|s| s.rate == rate) {
        return Err(CommandError::new(
            "commands.time.rate.already_same",
            vec![string_arg(clock), PrimitiveOrComponent::Float(rate)],
        ));
    }
    clocks.set_rate(&clock, rate);
    server.broadcast_clock(source.level, &clock);
    server.send_success(
        source,
        translatable(
            "commands.time.rate",
            vec![string_arg(clock), PrimitiveOrComponent::Float(rate)],
        ),
        true,
    );
    Ok(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Weather {
    Clear,
    Rain,
    Thunder,
}

/// `WeatherCommand.getDuration`: the argument, or a draw from the type's `UniformInt`
/// (`ServerLevel.RAIN_DELAY` 12000..=180000, `RAIN_DURATION` 12000..=24000, `THUNDER_DURATION`
/// 3600..=15600).
fn duration_of(weather: Weather, input: i32) -> i32 {
    if input != -1 {
        return input;
    }
    let (min, max) = match weather {
        Weather::Clear => (12000, 180_000),
        Weather::Rain => (12000, 24000),
        Weather::Thunder => (3600, 15600),
    };
    rand::random_range(min..=max)
}

/// `WeatherCommand.setClear/setRain/setThunder` with `MinecraftServer.setWeatherParameters`: the
/// level's weather data changes; the weather cycle that acts on it is not ported (`weather.dat` is
/// not written either).
pub(super) fn weather(
    server: &mut Server,
    source: &CommandSource,
    weather: Weather,
    duration: i32,
) -> Result<i32, CommandError> {
    let time = duration_of(weather, duration);
    let data = &mut server.world.levels[source.level].weather;
    match weather {
        Weather::Clear => {
            data.clear_weather_time = time;
            data.rain_time = 0;
            data.thunder_time = 0;
            data.raining = false;
            data.thundering = false;
        }
        Weather::Rain => {
            data.clear_weather_time = 0;
            data.rain_time = time;
            data.thunder_time = time;
            data.raining = true;
            data.thundering = false;
        }
        Weather::Thunder => {
            data.clear_weather_time = 0;
            data.rain_time = time;
            data.thunder_time = time;
            data.raining = true;
            data.thundering = true;
        }
    }
    let key = match weather {
        Weather::Clear => "commands.weather.set.clear",
        Weather::Rain => "commands.weather.set.rain",
        Weather::Thunder => "commands.weather.set.thunder",
    };
    server.send_success(source, translatable(key, Vec::new()), true);
    Ok(duration)
}
