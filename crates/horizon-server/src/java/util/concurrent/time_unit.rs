//! Port of `java.util.concurrent.TimeUnit`.
use crate::java::lang::Throwable;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
pub enum TimeUnit {
    NANOSECONDS,
    MICROSECONDS,
    MILLISECONDS,
    SECONDS,
    MINUTES,
    HOURS,
    DAYS,
}

const NANO_SCALE: i64 = 1;
const MICRO_SCALE: i64 = 1000 * NANO_SCALE;
const MILLI_SCALE: i64 = 1000 * MICRO_SCALE;
const SECOND_SCALE: i64 = 1000 * MILLI_SCALE;
const MINUTE_SCALE: i64 = 60 * SECOND_SCALE;
const HOUR_SCALE: i64 = 60 * MINUTE_SCALE;
const DAY_SCALE: i64 = 24 * HOUR_SCALE;

impl TimeUnit {
    pub const VALUES: [TimeUnit; 7] = [
        TimeUnit::NANOSECONDS,
        TimeUnit::MICROSECONDS,
        TimeUnit::MILLISECONDS,
        TimeUnit::SECONDS,
        TimeUnit::MINUTES,
        TimeUnit::HOURS,
        TimeUnit::DAYS,
    ];
    pub fn values() -> Vec<TimeUnit> {
        Self::VALUES.to_vec()
    }
    pub fn ordinal(self) -> i32 {
        self as i32
    }
    pub fn name(self) -> &'static str {
        match self {
            TimeUnit::NANOSECONDS => "NANOSECONDS",
            TimeUnit::MICROSECONDS => "MICROSECONDS",
            TimeUnit::MILLISECONDS => "MILLISECONDS",
            TimeUnit::SECONDS => "SECONDS",
            TimeUnit::MINUTES => "MINUTES",
            TimeUnit::HOURS => "HOURS",
            TimeUnit::DAYS => "DAYS",
        }
    }
    pub fn value_of(name: &str) -> Option<TimeUnit> {
        Self::VALUES.into_iter().find(|v| v.name() == name)
    }

    fn scale(self) -> i64 {
        match self {
            TimeUnit::NANOSECONDS => NANO_SCALE,
            TimeUnit::MICROSECONDS => MICRO_SCALE,
            TimeUnit::MILLISECONDS => MILLI_SCALE,
            TimeUnit::SECONDS => SECOND_SCALE,
            TimeUnit::MINUTES => MINUTE_SCALE,
            TimeUnit::HOURS => HOUR_SCALE,
            TimeUnit::DAYS => DAY_SCALE,
        }
    }

    /// Java's saturating conversion from `d` in `from` scale to `to` scale.
    fn cvt(d: i64, dst: i64, src: i64) -> i64 {
        if src == dst {
            d
        } else if src < dst {
            d / (dst / src)
        } else {
            let r = src / dst;
            let m = i64::MAX / r;
            if d > m {
                i64::MAX
            } else if d < -m {
                i64::MIN
            } else {
                d * r
            }
        }
    }

    /// `convert(long sourceDuration, TimeUnit sourceUnit)`.
    pub fn convert(self, source_duration: i64, source_unit: TimeUnit) -> i64 {
        Self::cvt(source_duration, self.scale(), source_unit.scale())
    }
    /// `convert(Duration)`.
    pub fn convert_1(self, duration: Duration) -> i64 {
        let nanos = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
        self.convert(nanos, TimeUnit::NANOSECONDS)
    }
    pub fn to_nanos(self, duration: i64) -> i64 {
        Self::cvt(duration, NANO_SCALE, self.scale())
    }
    pub fn to_micros(self, duration: i64) -> i64 {
        Self::cvt(duration, MICRO_SCALE, self.scale())
    }
    pub fn to_millis(self, duration: i64) -> i64 {
        Self::cvt(duration, MILLI_SCALE, self.scale())
    }
    pub fn to_seconds(self, duration: i64) -> i64 {
        Self::cvt(duration, SECOND_SCALE, self.scale())
    }
    pub fn to_minutes(self, duration: i64) -> i64 {
        Self::cvt(duration, MINUTE_SCALE, self.scale())
    }
    pub fn to_hours(self, duration: i64) -> i64 {
        Self::cvt(duration, HOUR_SCALE, self.scale())
    }
    pub fn to_days(self, duration: i64) -> i64 {
        Self::cvt(duration, DAY_SCALE, self.scale())
    }
    /// The duration as a `std::time::Duration` (negative durations are zero).
    pub fn to_duration(self, duration: i64) -> Duration {
        Duration::from_nanos(self.to_nanos(duration).max(0) as u64)
    }
    /// `Thread.sleep` for `timeout` of this unit (`throws InterruptedException`).
    pub fn sleep(self, timeout: i64) -> Result<(), Throwable> {
        if timeout > 0 {
            std::thread::sleep(self.to_duration(timeout));
        }
        Ok(())
    }
}
