//! `Rotation` and `Mirror` (`net.minecraft.world.level.block`): how structure templates turn and
//! flip. What they do to a state comes from the block tables ([`BlockStateData::rotate`],
//! [`BlockStateData::mirror`]); here is what they do to directions and names.
//!
//! [`BlockStateData::rotate`]: super::BlockStateData::rotate
//! [`BlockStateData::mirror`]: super::BlockStateData::mirror

use crate::core::geometry::Direction;
use crate::util::random::RandomSource;

/// `Rotation`, in vanilla's order (the ordinal is the number of quarter turns clockwise).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rotation {
    #[default]
    None,
    Clockwise90,
    Clockwise180,
    Counterclockwise90,
}

/// `Mirror`, in vanilla's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mirror {
    #[default]
    None,
    /// Flips Z.
    LeftRight,
    /// Flips X.
    FrontBack,
}

impl Rotation {
    pub const ALL: [Rotation; 4] = [
        Self::None,
        Self::Clockwise90,
        Self::Clockwise180,
        Self::Counterclockwise90,
    ];

    /// The serialized name (`StringRepresentable`), as piece tags save it.
    pub fn name(self) -> &'static str {
        ["none", "clockwise_90", "180", "counterclockwise_90"][self as usize]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }

    /// `Rotation.getRotated`: this rotation followed by `other`.
    pub fn then(self, other: Rotation) -> Self {
        Self::ALL[(self as usize + other as usize) % 4]
    }

    /// `Rotation.rotate(Direction)`: vertical directions stay.
    pub fn rotate(self, direction: Direction) -> Direction {
        if matches!(direction, Direction::Up | Direction::Down) {
            return direction;
        }
        let clockwise = |d| match d {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            _ => Direction::North,
        };
        (0..self as usize).fold(direction, |d, _| clockwise(d))
    }

    /// `Rotation.getRandom(random)`: one `nextInt(4)`.
    pub fn random(random: &mut impl RandomSource) -> Self {
        Self::ALL[random.next_int_bound(4).unwrap_or(0) as usize]
    }
}

impl Mirror {
    pub const ALL: [Mirror; 3] = [Self::None, Self::LeftRight, Self::FrontBack];

    pub fn name(self) -> &'static str {
        ["none", "left_right", "front_back"][self as usize]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }

    /// `Mirror.mirror(Direction)`: the flipped axis's directions swap.
    pub fn mirror(self, direction: Direction) -> Direction {
        match (self, direction) {
            (Self::FrontBack, Direction::West | Direction::East)
            | (Self::LeftRight, Direction::North | Direction::South) => direction.opposite(),
            _ => direction,
        }
    }
}
