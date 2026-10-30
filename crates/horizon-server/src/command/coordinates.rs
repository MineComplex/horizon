//! Positions and rotations typed in commands (`net.minecraft.commands.arguments.coordinates`):
//! world coordinates (`1 ~2 ~`), local ones (`^ ^1 ^`), how each argument type reads them, the
//! coordinate suggestions, and their resolution against where a command runs.
//!
//! Resolution is pure: [`CommandOrigin`] holds what `CommandSourceStack` supplies (position,
//! anchor position, rotation) as plain values, so these functions need no source or level.

use super::{
    ArgumentType, CommandError, StringReader, SuggestionsBuilder, values::suggest_matching,
};
use crate::core::geometry::{Vec3, trig};
use crate::core::position::BlockPos;

/// One axis of [`Coordinates::World`] (`WorldCoordinate`): a value, or an offset after `~`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldCoordinate {
    pub relative: bool,
    pub value: f64,
}

impl WorldCoordinate {
    const RELATIVE_ZERO: Self = Self {
        relative: true,
        value: 0.0,
    };

    /// `get`: the coordinate from the source's value on this axis.
    pub fn get(self, original: f64) -> f64 {
        if self.relative {
            self.value + original
        } else {
            self.value
        }
    }

    /// `parseDouble`: with `center`, a whole absolute number is the block's centre (`+0.5`).
    pub(super) fn parse_double(
        reader: &mut StringReader,
        center: bool,
    ) -> Result<Self, CommandError> {
        let relative = Self::start(reader, CommandError::pos_missing_double)?;
        let start = reader.cursor();
        let mut value = if reader.can_read() && reader.peek() != ' ' {
            reader.read_double()?
        } else {
            0.0
        };
        let number = &reader.string()[start..reader.cursor()];
        if relative && number.is_empty() {
            return Ok(Self::RELATIVE_ZERO);
        }
        if !number.contains('.') && !relative && center {
            value += 0.5;
        }
        Ok(Self { relative, value })
    }

    /// `parseInt`: an absolute value is an integer, an offset may have a fraction.
    pub(super) fn parse_int(reader: &mut StringReader) -> Result<Self, CommandError> {
        let relative = Self::start(reader, CommandError::pos_missing_int)?;
        let value = if !reader.can_read() || reader.peek() == ' ' {
            0.0
        } else if relative {
            reader.read_double()?
        } else {
            f64::from(reader.read_int()?)
        };
        Ok(Self { relative, value })
    }

    /// The checks both parsers start with, then `isRelative` (skips a `~`).
    fn start(
        reader: &mut StringReader,
        missing: fn() -> CommandError,
    ) -> Result<bool, CommandError> {
        if reader.can_read() && reader.peek() == '^' {
            return Err(CommandError::pos_mixed().at(reader));
        }
        if !reader.can_read() {
            return Err(missing().at(reader));
        }
        Ok(is_relative(reader))
    }
}

/// `WorldCoordinate.isRelative(reader)`: skips a leading `~`.
pub(super) fn is_relative(reader: &mut StringReader) -> bool {
    let relative = reader.peek() == '~';
    if relative {
        reader.skip();
    }
    relative
}

/// What `CommandSourceStack` gives coordinates: its position, the position of its anchor (feet or
/// eyes, which local coordinates start from) and its rotation `[x_rot, y_rot]` (pitch, yaw).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandOrigin {
    pub position: Vec3,
    pub anchor: Vec3,
    pub rotation: [f32; 2],
}

/// The `Coordinates` an argument parsed. Every coordinate type parses to this (as in vanilla): a
/// column, a 2D vector and a rotation are world coordinates with an unused `~` axis
/// (`y` for columns and 2D vectors, `z` for rotations, which keep the pitch in `x` and the yaw in `y`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coordinates {
    World {
        x: WorldCoordinate,
        y: WorldCoordinate,
        z: WorldCoordinate,
    },
    /// `^left ^up ^forwards`, along the source's rotation from its anchor.
    Local { left: f64, up: f64, forwards: f64 },
}

impl Coordinates {
    /// `getPosition`.
    pub fn position(&self, origin: &CommandOrigin) -> Vec3 {
        match *self {
            Self::World { x, y, z } => {
                let pos = origin.position;
                Vec3::new(x.get(pos.x), y.get(pos.y), z.get(pos.z))
            }
            Self::Local { left, up, forwards } => {
                local_position(origin.anchor, origin.rotation, left, up, forwards)
            }
        }
    }

    /// `getBlockPos` (`BlockPos.containing`).
    pub fn block_pos(&self, origin: &CommandOrigin) -> BlockPos {
        BlockPos::containing(self.position(origin))
    }

    /// `ColumnPosArgument.getColumnPos`: the block position's `[x, z]`.
    pub fn column_pos(&self, origin: &CommandOrigin) -> [i32; 2] {
        let pos = self.block_pos(origin);
        [pos.x, pos.z]
    }

    /// `Vec2Argument.getVec2`: the position's `[x, z]` as floats.
    pub fn vec2(&self, origin: &CommandOrigin) -> [f32; 2] {
        let pos = self.position(origin);
        [pos.x as f32, pos.z as f32]
    }

    /// `getRotation`: `[x_rot, y_rot]`; local coordinates give zero.
    pub fn rotation(&self, origin: &CommandOrigin) -> [f32; 2] {
        match *self {
            Self::World { x, y, .. } => {
                let [x_rot, y_rot] = origin.rotation;
                [
                    x.get(f64::from(x_rot)) as f32,
                    y.get(f64::from(y_rot)) as f32,
                ]
            }
            Self::Local { .. } => [0.0, 0.0],
        }
    }

    /// `isXRelative`, `isYRelative`, `isZRelative`: local coordinates are relative on every axis.
    pub fn relative_axes(&self) -> [bool; 3] {
        match self {
            Self::World { x, y, z } => [x.relative, y.relative, z.relative],
            Self::Local { .. } => [true; 3],
        }
    }

    /// `BlockPosArgument.parse`.
    pub(super) fn parse_block_pos(reader: &mut StringReader) -> Result<Self, CommandError> {
        if reader.can_read() && reader.peek() == '^' {
            Self::parse_local(reader)
        } else {
            Self::parse_world(reader, |r, _| WorldCoordinate::parse_int(r))
        }
    }

    /// `Vec3Argument.parse`: `center_correct` centres whole absolute `x` and `z` in their block.
    pub(super) fn parse_vec3(
        reader: &mut StringReader,
        center_correct: bool,
    ) -> Result<Self, CommandError> {
        if reader.can_read() && reader.peek() == '^' {
            Self::parse_local(reader)
        } else {
            Self::parse_world(reader, |r, axis| {
                WorldCoordinate::parse_double(r, center_correct && axis != 1)
            })
        }
    }

    /// `ColumnPosArgument.parse` and `Vec2Argument.parse`: `x z`, with `y` an unused `~`.
    pub(super) fn parse_column(
        reader: &mut StringReader,
        mut axis: impl FnMut(&mut StringReader) -> Result<WorldCoordinate, CommandError>,
    ) -> Result<Self, CommandError> {
        let [x, z] = Self::parse_pair(reader, &mut axis, CommandError::pos2d_incomplete)?;
        let y = WorldCoordinate::RELATIVE_ZERO;
        Ok(Self::World { x, y, z })
    }

    /// `RotationArgument.parse`: `yaw pitch`.
    pub(super) fn parse_rotation(reader: &mut StringReader) -> Result<Self, CommandError> {
        let mut axis = |r: &mut StringReader| WorldCoordinate::parse_double(r, false);
        let [y, x] = Self::parse_pair(reader, &mut axis, CommandError::rotation_incomplete)?;
        let z = WorldCoordinate::RELATIVE_ZERO;
        Ok(Self::World { x, y, z })
    }

    fn parse_pair(
        reader: &mut StringReader,
        axis: &mut impl FnMut(&mut StringReader) -> Result<WorldCoordinate, CommandError>,
        incomplete: fn() -> CommandError,
    ) -> Result<[WorldCoordinate; 2], CommandError> {
        let start = reader.cursor();
        if !reader.can_read() {
            return Err(incomplete().at(reader));
        }
        let first = axis(reader)?;
        if !(reader.can_read() && reader.peek() == ' ') {
            reader.set_cursor(start);
            return Err(incomplete().at(reader));
        }
        reader.skip();
        Ok([first, axis(reader)?])
    }

    /// `WorldCoordinates.parseInt`/`parseDouble`: three axes separated by single spaces. `axis`
    /// reads one axis, given its index.
    fn parse_world(
        reader: &mut StringReader,
        mut axis: impl FnMut(&mut StringReader, usize) -> Result<WorldCoordinate, CommandError>,
    ) -> Result<Self, CommandError> {
        let start = reader.cursor();
        let mut axes = [WorldCoordinate::RELATIVE_ZERO; 3];
        for (i, slot) in axes.iter_mut().enumerate() {
            if i > 0 {
                if !(reader.can_read() && reader.peek() == ' ') {
                    reader.set_cursor(start);
                    return Err(CommandError::pos3d_incomplete().at(reader));
                }
                reader.skip();
            }
            *slot = axis(reader, i)?;
        }
        let [x, y, z] = axes;
        Ok(Self::World { x, y, z })
    }

    /// `LocalCoordinates.parse`.
    fn parse_local(reader: &mut StringReader) -> Result<Self, CommandError> {
        let start = reader.cursor();
        let mut axes = [0.0; 3];
        for (i, slot) in axes.iter_mut().enumerate() {
            if i > 0 {
                if !(reader.can_read() && reader.peek() == ' ') {
                    reader.set_cursor(start);
                    return Err(CommandError::pos3d_incomplete().at(reader));
                }
                reader.skip();
            }
            if !reader.can_read() {
                return Err(CommandError::pos_missing_double().at(reader));
            }
            if reader.peek() != '^' {
                reader.set_cursor(start);
                return Err(CommandError::pos_mixed().at(reader));
            }
            reader.skip();
            if reader.can_read() && reader.peek() != ' ' {
                *slot = reader.read_double()?;
            }
        }
        let [left, up, forwards] = axes;
        Ok(Self::Local { left, up, forwards })
    }
}

/// `LocalCoordinates.apply`: `left`, `up` and `forwards` along `rotation` (`[x_rot, y_rot]`) from
/// `source`, with vanilla's float table trigonometry and its operation order.
pub fn local_position(source: Vec3, rotation: [f32; 2], left: f64, up: f64, forwards: f64) -> Vec3 {
    const RAD: f32 = (std::f64::consts::PI / 180.0) as f32;
    let [x_rot, y_rot] = rotation;
    let y_cos = trig::cos(f64::from((y_rot + 90.0) * RAD));
    let y_sin = trig::sin(f64::from((y_rot + 90.0) * RAD));
    let x_cos = trig::cos(f64::from(-x_rot * RAD));
    let x_sin = trig::sin(f64::from(-x_rot * RAD));
    let x_cos_up = trig::cos(f64::from((-x_rot + 90.0) * RAD));
    let x_sin_up = trig::sin(f64::from((-x_rot + 90.0) * RAD));
    let forward = Vec3::new(
        f64::from(y_cos * x_cos),
        f64::from(x_sin),
        f64::from(y_sin * x_cos),
    );
    let upward = Vec3::new(
        f64::from(y_cos * x_cos_up),
        f64::from(x_sin_up),
        f64::from(y_sin * x_cos_up),
    );
    let leftward = forward.cross(upward).scale(-1.0);
    Vec3::new(
        source.x + (forward.x * forwards + upward.x * up + leftward.x * left),
        source.y + (forward.y * forwards + upward.y * up + leftward.y * left),
        source.z + (forward.z * forwards + upward.z * up + leftward.z * left),
    )
}

/// `SharedSuggestionProvider.suggestCoordinates` / `suggest2DCoordinates` for a server source:
/// `^ ^ ^` when the text starts with `^`, else `~ ~ ~` (`getRelevantCoordinates` and
/// `getAbsoluteCoordinates` of `CommandSourceStack`), completing the axes not typed yet, each
/// candidate kept when `ty` parses it.
pub(super) fn suggest_coordinates(
    ty: &ArgumentType,
    builder: &mut SuggestionsBuilder,
    three: bool,
) {
    let remainder = builder.remaining();
    let c = if remainder.starts_with('^') { "^" } else { "~" };
    // `String.split(" ")`: trailing empty fields are dropped.
    let mut fields: Vec<&str> = remainder.split(' ').collect();
    while fields.last() == Some(&"") {
        fields.pop();
    }
    let (full, partial) = match (remainder.is_empty(), fields.as_slice(), three) {
        (true, _, true) => (
            format!("{c} {c} {c}"),
            vec![c.to_owned(), format!("{c} {c}")],
        ),
        (true, _, false) => (format!("{c} {c}"), vec![c.to_owned()]),
        (false, [x], true) => (format!("{x} {c} {c}"), vec![format!("{x} {c}")]),
        (false, [x, y], true) => (format!("{x} {y} {c}"), Vec::new()),
        (false, [x], false) => (format!("{x} {c}"), Vec::new()),
        _ => return,
    };
    if ty.parse(&mut StringReader::new(&full)).is_ok() {
        let result = partial.iter().chain([&full]);
        suggest_matching(builder, result.map(String::as_str));
    }
}
