//! Port of `net.kyori.adventure.text.format.NamedTextColor`.
//!
//! The 16 named colors are `TextColor` constants (`NamedTextColor::RED` is `TextColor::RED`): see
//! text_color.rs for why `NamedTextColor` is an alias.
use crate::prelude::*;
use crate::net::kyori::adventure::text::format::TextColor;
use crate::net::kyori::adventure::util::{HSVLike, Index};

pub type NamedTextColor = TextColor;

const BLACK_VALUE: i32 = 0x000000;
const DARK_BLUE_VALUE: i32 = 0x0000aa;
const DARK_GREEN_VALUE: i32 = 0x00aa00;
const DARK_AQUA_VALUE: i32 = 0x00aaaa;
const DARK_RED_VALUE: i32 = 0xaa0000;
const DARK_PURPLE_VALUE: i32 = 0xaa00aa;
const GOLD_VALUE: i32 = 0xffaa00;
const GRAY_VALUE: i32 = 0xaaaaaa;
const DARK_GRAY_VALUE: i32 = 0x555555;
const BLUE_VALUE: i32 = 0x5555ff;
const GREEN_VALUE: i32 = 0x55ff55;
const AQUA_VALUE: i32 = 0x55ffff;
const RED_VALUE: i32 = 0xff5555;
const LIGHT_PURPLE_VALUE: i32 = 0xff55ff;
const YELLOW_VALUE: i32 = 0xffff55;
const WHITE_VALUE: i32 = 0xffffff;

impl TextColor {
    pub const BLACK: TextColor = TextColor::named("black", BLACK_VALUE);
    pub const DARK_BLUE: TextColor = TextColor::named("dark_blue", DARK_BLUE_VALUE);
    pub const DARK_GREEN: TextColor = TextColor::named("dark_green", DARK_GREEN_VALUE);
    pub const DARK_AQUA: TextColor = TextColor::named("dark_aqua", DARK_AQUA_VALUE);
    pub const DARK_RED: TextColor = TextColor::named("dark_red", DARK_RED_VALUE);
    pub const DARK_PURPLE: TextColor = TextColor::named("dark_purple", DARK_PURPLE_VALUE);
    pub const GOLD: TextColor = TextColor::named("gold", GOLD_VALUE);
    pub const GRAY: TextColor = TextColor::named("gray", GRAY_VALUE);
    pub const DARK_GRAY: TextColor = TextColor::named("dark_gray", DARK_GRAY_VALUE);
    pub const BLUE: TextColor = TextColor::named("blue", BLUE_VALUE);
    pub const GREEN: TextColor = TextColor::named("green", GREEN_VALUE);
    pub const AQUA: TextColor = TextColor::named("aqua", AQUA_VALUE);
    pub const RED: TextColor = TextColor::named("red", RED_VALUE);
    pub const LIGHT_PURPLE: TextColor = TextColor::named("light_purple", LIGHT_PURPLE_VALUE);
    pub const YELLOW: TextColor = TextColor::named("yellow", YELLOW_VALUE);
    pub const WHITE: TextColor = TextColor::named("white", WHITE_VALUE);

    /// `NamedTextColor.VALUES` (declaration order).
    pub const NAMED_VALUES: [TextColor; 16] = [
        TextColor::BLACK,
        TextColor::DARK_BLUE,
        TextColor::DARK_GREEN,
        TextColor::DARK_AQUA,
        TextColor::DARK_RED,
        TextColor::DARK_PURPLE,
        TextColor::GOLD,
        TextColor::GRAY,
        TextColor::DARK_GRAY,
        TextColor::BLUE,
        TextColor::GREEN,
        TextColor::AQUA,
        TextColor::RED,
        TextColor::LIGHT_PURPLE,
        TextColor::YELLOW,
        TextColor::WHITE,
    ];

    /// `NamedTextColor.namedColor(int)`.
    pub fn named_color(value: i32) -> Option<TextColor> {
        named_color(value)
    }
    /// `NamedTextColor.ofExact(int)` (deprecated alias of `namedColor`).
    pub fn of_exact(value: i32) -> Option<TextColor> {
        named_color(value)
    }
    /// `NamedTextColor.nearestTo(TextColor)`.
    pub fn nearest_to(any: TextColor) -> TextColor {
        if any.is_named() {
            return any;
        }
        TextColor::nearest_color_to(&TextColor::NAMED_VALUES, any)
    }
    /// `NamedTextColor.NAMES.value(name)`.
    pub fn named_by_name(name: &str) -> Option<TextColor> {
        NAMES.value(&name.to_string())
    }
}

/// `NamedTextColor.NAMES`.
pub static NAMES: LazyLock<Index<String, TextColor>> =
    LazyLock::new(|| Index::create(TextColor::NAMED_VALUES.to_vec(), |c: &TextColor| c.name.unwrap_or("").to_string()));

pub fn named_color(value: i32) -> Option<TextColor> {
    match value {
        BLACK_VALUE => Some(TextColor::BLACK),
        DARK_BLUE_VALUE => Some(TextColor::DARK_BLUE),
        DARK_GREEN_VALUE => Some(TextColor::DARK_GREEN),
        DARK_AQUA_VALUE => Some(TextColor::DARK_AQUA),
        DARK_RED_VALUE => Some(TextColor::DARK_RED),
        DARK_PURPLE_VALUE => Some(TextColor::DARK_PURPLE),
        GOLD_VALUE => Some(TextColor::GOLD),
        GRAY_VALUE => Some(TextColor::GRAY),
        DARK_GRAY_VALUE => Some(TextColor::DARK_GRAY),
        BLUE_VALUE => Some(TextColor::BLUE),
        GREEN_VALUE => Some(TextColor::GREEN),
        AQUA_VALUE => Some(TextColor::AQUA),
        RED_VALUE => Some(TextColor::RED),
        LIGHT_PURPLE_VALUE => Some(TextColor::LIGHT_PURPLE),
        YELLOW_VALUE => Some(TextColor::YELLOW),
        WHITE_VALUE => Some(TextColor::WHITE),
        _ => None,
    }
}
#[allow(unused)]
fn _hsv(_: HSVLike) {}
