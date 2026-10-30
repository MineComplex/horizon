//! Minecraft's argument values that need no registry or world: angles, swizzles, number ranges,
//! durations, UUIDs, scoreboard operations and team colours (`net.minecraft.commands.arguments`,
//! `MinMaxBounds`), with their parsers.

use uuid::Uuid;

use super::{CommandError, StringReader, SuggestionsBuilder, coordinates::is_relative};

/// `AngleArgument.SingleAngle`: degrees, or an offset from the source's yaw after `~`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Angle {
    pub angle: f32,
    pub relative: bool,
}

impl Angle {
    /// `getAngle`: wrapped to `[-180, 180)`.
    pub fn resolve(self, y_rot: f32) -> f32 {
        wrap_degrees(if self.relative {
            self.angle + y_rot
        } else {
            self.angle
        })
    }

    /// `AngleArgument.parse`: NaN and infinities are refused after the number.
    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        if !reader.can_read() {
            return Err(CommandError::new("argument.angle.incomplete", Vec::new()).at(reader));
        }
        let relative = is_relative(reader);
        let angle = if reader.can_read() && reader.peek() != ' ' {
            reader.read_float()?
        } else {
            0.0
        };
        if !angle.is_finite() {
            return Err(CommandError::new("argument.angle.invalid", Vec::new()).at(reader));
        }
        Ok(Self { angle, relative })
    }
}

/// `Mth.wrapDegrees(float)`.
pub fn wrap_degrees(angle: f32) -> f32 {
    let mut normalized = angle % 360.0;
    if normalized >= 180.0 {
        normalized -= 360.0;
    }
    if normalized < -180.0 {
        normalized += 360.0;
    }
    normalized
}

/// `SwizzleArgument`'s set of axes, indexed `[x, y, z]` (for `Vec3::align`). It may be empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Swizzle(pub [bool; 3]);

impl Swizzle {
    /// `SwizzleArgument.parse`: `x`, `y` and `z` at most once each, up to a space.
    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        let mut axes = [false; 3];
        while reader.can_read() && reader.peek() != ' ' {
            let axis = match reader.read() {
                'x' => 0,
                'y' => 1,
                'z' => 2,
                _ => {
                    return Err(
                        CommandError::new("arguments.swizzle.invalid", Vec::new()).at(reader)
                    );
                }
            };
            if axes[axis] {
                return Err(CommandError::new("arguments.swizzle.invalid", Vec::new()).at(reader));
            }
            axes[axis] = true;
        }
        Ok(Self(axes))
    }
}

/// `MinMaxBounds.Ints`: `min..max`, either side optional, or one number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IntRange {
    pub min: Option<i32>,
    pub max: Option<i32>,
}

impl IntRange {
    pub fn matches(self, value: i32) -> bool {
        !self.min.is_some_and(|min| min > value) && self.max.is_none_or(|max| max >= value)
    }

    /// `matchesSqr`: against the squared bounds.
    pub fn matches_sqr(self, value_sqr: i64) -> bool {
        let sqr = |v: i32| i64::from(v) * i64::from(v);
        !self.min.is_some_and(|min| sqr(min) > value_sqr)
            && self.max.is_none_or(|max| sqr(max) >= value_sqr)
    }

    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        let (min, max) = read_bounds(reader, CommandError::reader_invalid_int, |a: &i32, b| {
            a.cmp(b)
        })?;
        Ok(Self { min, max })
    }
}

/// `MinMaxBounds.Doubles`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DoubleRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl DoubleRange {
    /// `matches`: NaN matches any range, as in Java (`min > v` and `!(max < v)`).
    pub fn matches(self, value: f64) -> bool {
        Self::within(self.min, self.max, value)
    }

    /// `matchesSqr`: against the squared bounds (`Mth.square`, so a negative minimum squares up).
    pub fn matches_sqr(self, value_sqr: f64) -> bool {
        Self::within(self.min.map(|v| v * v), self.max.map(|v| v * v), value_sqr)
    }

    fn within(min: Option<f64>, max: Option<f64>, value: f64) -> bool {
        use std::cmp::Ordering::Less;
        !min.is_some_and(|min| min > value)
            && max.is_none_or(|max| max.partial_cmp(&value) != Some(Less))
    }

    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        // `Double.compareTo`: `0..-0` is swapped.
        let (min, max) = read_bounds(reader, CommandError::reader_invalid_double, f64::total_cmp)?;
        Ok(Self { min, max })
    }
}

/// `MinMaxBounds.Bounds.fromReader`, then the swapped check of `Ints`/`Doubles.fromReader`. Every
/// error of the bounds is moved to their start.
fn read_bounds<T: std::str::FromStr + Copy>(
    reader: &mut StringReader,
    invalid: fn(&str) -> CommandError,
    compare: impl Fn(&T, &T) -> std::cmp::Ordering,
) -> Result<(Option<T>, Option<T>), CommandError> {
    let start = reader.cursor();
    let at_start = |reader: &mut StringReader, error: CommandError| {
        reader.set_cursor(start);
        Err(error.at(reader))
    };
    if !reader.can_read() {
        return at_start(
            reader,
            CommandError::new("argument.range.empty", Vec::new()),
        );
    }
    let number = |reader: &mut StringReader| -> Result<Option<T>, CommandError> {
        let from = reader.cursor();
        // Digits, `-`, and a `.` not followed by another.
        while reader.can_read()
            && (matches!(reader.peek(), '0'..='9' | '-')
                || (reader.peek() == '.' && reader.peek_at(1) != '.'))
        {
            reader.skip();
        }
        let text = &reader.string()[from..reader.cursor()];
        if text.is_empty() {
            return Ok(None);
        }
        text.parse().map(Some).map_err(|_| invalid(text))
    };
    let min = match number(reader) {
        Ok(min) => min,
        Err(error) => return at_start(reader, error),
    };
    let max = if reader.can_read_n(2) && reader.peek() == '.' && reader.peek_at(1) == '.' {
        reader.skip();
        reader.skip();
        match number(reader) {
            Ok(max) => max,
            Err(error) => return at_start(reader, error),
        }
    } else {
        min
    };
    match (min, max) {
        (None, None) => at_start(
            reader,
            CommandError::new("argument.range.empty", Vec::new()),
        ),
        (Some(a), Some(b)) if compare(&a, &b).is_gt() => at_start(
            reader,
            CommandError::new("argument.range.swapped", Vec::new()),
        ),
        _ => Ok((min, max)),
    }
}

/// `TimeArgument`'s units (`d`, `s`, `t` or none) and their ticks.
const TIME_UNITS: [(&str, i32); 4] = [("d", 24000), ("s", 20), ("t", 1), ("", 1)];

/// `TimeArgument.parse`: a float and a unit, rounded to ticks, at least `min`.
pub(super) fn parse_time(reader: &mut StringReader, min: i32) -> Result<i32, CommandError> {
    let value = reader.read_float()?;
    let unit = reader.read_unquoted_string();
    let Some(&(_, factor)) = TIME_UNITS.iter().find(|(name, _)| *name == unit) else {
        return Err(CommandError::new("argument.time.invalid_unit", Vec::new()).at(reader));
    };
    let ticks = java_round(value * factor as f32);
    if ticks < min {
        return Err(CommandError::time_tick_count_too_low(ticks, min).at(reader));
    }
    Ok(ticks)
}

/// `TimeArgument.listSuggestions`: the units, after a number.
pub(super) fn suggest_time_units(builder: &mut SuggestionsBuilder) {
    let mut reader = StringReader::new(builder.remaining());
    if reader.read_float().is_ok() {
        let mut units = builder.create_offset(builder.start() + reader.cursor());
        suggest_matching(&mut units, TIME_UNITS.iter().map(|(name, _)| *name));
        builder.add(units);
    }
}

/// `Math.round(float)`: the nearest `int`, halves up, saturating (NaN is 0).
fn java_round(value: f32) -> i32 {
    let bits = value.to_bits() as i32;
    let biased_exp = (bits & 0x7f80_0000) >> 23;
    let shift = (24 - 2 + 127) - biased_exp;
    if shift & -32 == 0 {
        let mut r = (bits & 0x007f_ffff) | 0x0080_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        value as i32
    }
}

/// `UuidArgument.parse`: the longest run of hex digits and `-`, read as `UUID.fromString` does
/// (five non-empty dash-separated hex fields, each below 2^63, truncated to 8-4-4-4-12 digits).
pub(super) fn parse_uuid(reader: &mut StringReader) -> Result<Uuid, CommandError> {
    let remaining = reader.remaining();
    let len = remaining
        .find(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .unwrap_or(remaining.len());
    let text = &remaining[..len];
    let field = |text: &str| {
        u128::from_str_radix(text, 16)
            .ok()
            .filter(|&v| v <= i64::MAX as u128)
    };
    let fields: Vec<&str> = text.split('-').collect();
    let parsed = (len > 0 && len <= 36 && fields.len() == 5)
        .then(|| {
            fields
                .iter()
                .map(|f| field(f))
                .collect::<Option<Vec<u128>>>()
        })
        .flatten();
    let Some(v) = parsed else {
        return Err(CommandError::new("argument.uuid.invalid", Vec::new()).at(reader));
    };
    let most = (v[0] & 0xffff_ffff) << 32 | (v[1] & 0xffff) << 16 | (v[2] & 0xffff);
    let least = (v[3] & 0xffff) << 48 | (v[4] & 0xffff_ffff_ffff);
    reader.set_cursor(reader.cursor() + len);
    Ok(Uuid::from_u64_pair(most as u64, least as u64))
}

/// `OperationArgument.Operation`: how `/scoreboard players operation` combines two scores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScoreOperation {
    Assign,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Min,
    Max,
    Swap,
}

impl ScoreOperation {
    pub const ALL: [Self; 9] = [
        Self::Assign,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Divide,
        Self::Modulo,
        Self::Min,
        Self::Max,
        Self::Swap,
    ];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Assign => "=",
            Self::Add => "+=",
            Self::Subtract => "-=",
            Self::Multiply => "*=",
            Self::Divide => "/=",
            Self::Modulo => "%=",
            Self::Min => "<",
            Self::Max => ">",
            Self::Swap => "><",
        }
    }

    /// `apply(a, b)`: the target `a` takes the result (both, for a swap). Division and modulo floor,
    /// overflow wraps as Java's `int`, and dividing by zero fails with no position.
    pub fn apply(self, a: &mut i32, b: &mut i32) -> Result<(), CommandError> {
        let div0 = || CommandError::new("arguments.operation.div0", Vec::new());
        *a = match self {
            Self::Assign => *b,
            Self::Add => a.wrapping_add(*b),
            Self::Subtract => a.wrapping_sub(*b),
            Self::Multiply => a.wrapping_mul(*b),
            // `Math.floorDiv`, `Math.floorMod`.
            Self::Divide if *b == 0 => return Err(div0()),
            Self::Divide => {
                let q = a.wrapping_div(*b);
                if (*a ^ *b) < 0 && q.wrapping_mul(*b) != *a {
                    q - 1
                } else {
                    q
                }
            }
            Self::Modulo if *b == 0 => return Err(div0()),
            Self::Modulo => {
                let r = a.wrapping_rem(*b);
                if (*a ^ *b) < 0 && r != 0 { r + *b } else { r }
            }
            Self::Min => (*a).min(*b),
            Self::Max => (*a).max(*b),
            Self::Swap => {
                std::mem::swap(a, b);
                return Ok(());
            }
        };
        Ok(())
    }

    /// `OperationArgument.parse`: everything up to a space. An unknown operation fails with no
    /// position (`create()`), the reader after it.
    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        let invalid = || CommandError::new("arguments.operation.invalid", Vec::new());
        if !reader.can_read() {
            return Err(invalid().at(reader));
        }
        let start = reader.cursor();
        while reader.can_read() && reader.peek() != ' ' {
            reader.skip();
        }
        let text = &reader.string()[start..reader.cursor()];
        Self::ALL
            .into_iter()
            .find(|op| op.symbol() == text)
            .ok_or_else(invalid)
    }
}

/// `TeamColor`: the sixteen chat colours a team, a bossbar or a scoreboard slot can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TeamColor {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
}

impl TeamColor {
    /// In id order.
    pub const ALL: [Self; 16] = [
        Self::Black,
        Self::DarkBlue,
        Self::DarkGreen,
        Self::DarkAqua,
        Self::DarkRed,
        Self::DarkPurple,
        Self::Gold,
        Self::Gray,
        Self::DarkGray,
        Self::Blue,
        Self::Green,
        Self::Aqua,
        Self::Red,
        Self::LightPurple,
        Self::Yellow,
        Self::White,
    ];

    /// The network id.
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// `getSerializedName`.
    pub const fn name(self) -> &'static str {
        [
            "black",
            "dark_blue",
            "dark_green",
            "dark_aqua",
            "dark_red",
            "dark_purple",
            "gold",
            "gray",
            "dark_gray",
            "blue",
            "green",
            "aqua",
            "red",
            "light_purple",
            "yellow",
            "white",
        ][self as usize]
    }

    /// `rgb`: the colour of its `TextColor`.
    pub const fn rgb(self) -> u32 {
        [
            0x000000, 0x0000aa, 0x00aa00, 0x00aaaa, 0xaa0000, 0xaa00aa, 0xffaa00, 0xaaaaaa,
            0x555555, 0x5555ff, 0x55ff55, 0x55ffff, 0xff5555, 0xff55ff, 0xffff55, 0xffffff,
        ][self as usize]
    }

    /// `byName` (case-sensitive).
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// `TeamColorArgument.parse`.
    pub(super) fn parse(reader: &mut StringReader) -> Result<Self, CommandError> {
        let name = reader.read_unquoted_string();
        Self::by_name(name).ok_or_else(|| CommandError::color_invalid(name).at(reader))
    }
}

/// `SharedSuggestionProvider.suggest(values, builder)`: the values the typed text matches from
/// their start or after a `.`, `_` or `/` (`matchesSubStr`), ignoring case.
pub(super) fn suggest_matching<'v>(
    builder: &mut SuggestionsBuilder,
    values: impl IntoIterator<Item = &'v str>,
) {
    let typed = builder.remaining_lowercase().to_owned();
    for value in values {
        if matches_sub_str(&typed, &value.to_lowercase()) {
            builder.suggest(value);
        }
    }
}

/// `SharedSuggestionProvider.matchesSubStr`.
fn matches_sub_str(pattern: &str, input: &str) -> bool {
    let mut index = 0;
    while !input[index..].starts_with(pattern) {
        match input[index..].find(['.', '_', '/']) {
            Some(at) => index += at + 1,
            None => return false,
        }
    }
    true
}
