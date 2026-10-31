//! `LegacyComponentSerializer.legacySection().deserialize(text)` as Paper builds it
//! (`LegacyComponentSerializerProviderImpl`: `§`, hex colours on, BungeeCord's `§x§r§r§g§g§b§b`
//! form on): the text of `messages.whitelist` with `§` codes becomes a styled component.
//!
//! Derived from Adventure 5.2.0's sources (`LegacyComponentSerializerImpl.deserialize`,
//! `CharacterAndFormat.defaults`); Adventure's classes are not in the 26.3 jar or its libraries, so
//! there is no oracle. Not ported: the URL replacement (Paper's serializer has none).
//!
//! The text is read from the end: each code styles the text after it, up to the code that was read
//! before. A colour (or `§r`) ends the run it styles ("reset": what comes before it is a sibling, not
//! inside it); a decoration keeps going, so what comes before is nested around it.

use crate::azalea::chat::FormattedText;
use crate::azalea::chat::style::{ChatFormatting, Style, TextColor};
use crate::azalea::chat::text_component::TextComponent;

const SECTION: char = '§';
const HEX: char = '#';
/// `CharacterAndFormat.defaults()` in order; every letter is also read in upper case.
const CODES: &str = "0123456789abcdefklmnor";

/// An Adventure `TextComponent`: its content, style and children.
#[derive(Default)]
struct Node {
    content: String,
    color: Option<TextColor>,
    /// `TextDecoration.State.TRUE`: obfuscated, bold, strikethrough, underlined, italic.
    decorations: [bool; 5],
    children: Vec<Node>,
}

#[derive(Clone)]
enum Format {
    Color(TextColor),
    /// An index of `decorations`.
    Decoration(usize),
    Reset,
}

struct Decoded {
    format: Format,
    /// The code is `§#rrggbb`, which is 8 characters where the others are 2.
    hex: bool,
    /// Bungee form: 14 characters, seen from the last one.
    bungee: bool,
}

/// What Adventure's `deserialize` throws: a `§#rrggbb` code that overlaps a later code makes
/// `String.substring(from, to)` fail with a `StringIndexOutOfBoundsException`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("java.lang.StringIndexOutOfBoundsException: begin {begin}, end {end}, length {length}")]
pub struct StringIndexOutOfBounds {
    pub begin: usize,
    pub end: usize,
    pub length: usize,
}

/// `LegacyComponentSerializer.legacySection().deserialize(input)` as a vanilla component.
pub fn deserialize(input: &str) -> Result<FormattedText, StringIndexOutOfBounds> {
    parse(input).map(|node| node_to_text(&node))
}

fn parse(input: &str) -> Result<Node, StringIndexOutOfBounds> {
    let s: Vec<char> = input.chars().collect();
    // `input.lastIndexOf('§', input.length() - 2)`.
    let last_section = |from: isize| -> Option<usize> {
        let from = from.min(s.len() as isize - 1);
        (0..=from)
            .rev()
            .find(|&i| s[i as usize] == SECTION)
            .map(|i| i as usize)
    };
    let Some(mut next) = last_section(s.len() as isize - 2) else {
        return Ok(Node {
            content: input.to_owned(),
            ..Node::default()
        });
    };
    let mut parts: Vec<Node> = Vec::new();
    let mut current: Option<Node> = None;
    let mut reset = false;
    let mut pos = s.len();
    loop {
        if let Some(decoded) = decode(s[next + 1], &s, next + 2) {
            let from = next + if decoded.hex { 8 } else { 2 };
            if from != pos {
                current = Some(match current.take() {
                    Some(done) if reset => {
                        parts.push(done);
                        reset = false;
                        Node::default()
                    }
                    Some(inner) => Node {
                        children: vec![inner],
                        ..Node::default()
                    },
                    None => Node::default(),
                });
                // Java's `substring` throws when a hex code overlaps a later one.
                let text = s.get(from..pos).ok_or(StringIndexOutOfBounds {
                    begin: from,
                    end: pos,
                    length: s.len(),
                })?;
                current.as_mut().expect("just made").content = text.iter().collect();
            } else if current.is_none() {
                current = Some(Node::default());
            }
            if !reset {
                reset = apply(current.as_mut().expect("made above"), decoded.format);
            }
            if decoded.bungee {
                next = next.saturating_sub(12);
            }
            pos = next;
        }
        match next.checked_sub(1).and_then(|n| last_section(n as isize)) {
            Some(n) => next = n,
            None => break,
        }
    }
    parts.extend(current);
    let remaining: String = s[..pos].iter().collect();
    if parts.len() == 1 && remaining.is_empty() {
        return Ok(parts.remove(0));
    }
    parts.reverse();
    Ok(Node {
        content: remaining,
        children: parts,
        ..Node::default()
    })
}

/// `determineFormatType` then `decodeTextFormat`; `pos` is just after the code character.
fn decode(code: char, s: &[char], pos: usize) -> Option<Decoded> {
    if pos >= 14 && s[pos - 14] == SECTION && s[pos - 13] == 'x' {
        // BungeeCord: the six digits sit in every other character, ending at `pos - 1`.
        let hex: String = (0..6).rev().map(|i| s[pos - 1 - 2 * i]).collect();
        return parse_hex(&hex).map(|format| Decoded {
            format,
            hex: false,
            bungee: true,
        });
    }
    if code == HEX && s.len() - pos >= 6 {
        let hex: String = s[pos..pos + 6].iter().collect();
        return parse_hex(&hex).map(|format| Decoded {
            format,
            hex: true,
            bungee: false,
        });
    }
    let index = CODES
        .find(code.to_ascii_lowercase())
        .filter(|_| code.is_ascii())?;
    let format = match index {
        0..=15 => {
            let formatting = ChatFormatting::from_code(CODES.as_bytes()[index] as char)?;
            Format::Color(TextColor::try_from(formatting).ok()?)
        }
        16..=20 => Format::Decoration(index - 16),
        _ => Format::Reset,
    };
    Some(Decoded {
        format,
        hex: false,
        bungee: false,
    })
}

/// `Integer.parseInt(digits, 16)` then `TextColor.color`.
fn parse_hex(digits: &str) -> Option<Format> {
    let value = i32::from_str_radix(digits, 16).ok()?;
    Some(Format::Color(TextColor::from_rgb(value as u32 & 0xff_ffff)))
}

/// `applyFormat`: true when the format ends the run (a colour or a reset).
fn apply(node: &mut Node, format: Format) -> bool {
    match format {
        Format::Color(color) => {
            node.color.get_or_insert(color);
            true
        }
        Format::Decoration(i) => {
            node.decorations[i] = true;
            false
        }
        Format::Reset => true,
    }
}

fn node_to_text(node: &Node) -> FormattedText {
    let mut style = Style::new().color(node.color.clone());
    let [obfuscated, bold, strikethrough, underlined, italic] = node.decorations;
    style.obfuscated = obfuscated.then_some(true);
    style.bold = bold.then_some(true);
    style.strikethrough = strikethrough.then_some(true);
    style.underlined = underlined.then_some(true);
    style.italic = italic.then_some(true);
    let mut text = TextComponent::new(node.content.clone()).with_style(style);
    text.base.siblings = node.children.iter().map(node_to_text).collect();
    FormattedText::Text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The component as vanilla's JSON, which is what a client reads.
    fn json(input: &str) -> String {
        deserialize(input).unwrap().to_json().to_string()
    }

    #[test]
    fn random_text_never_fails_where_java_cannot() {
        let alphabet = ['§', '§', '#', 'x', 'a', '1', 'f', '+', 'Z'];
        let mut state = 0x9e3779b97f4a7c15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..300_000 {
            let len = (next() % 24) as usize;
            let s: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            assert!(deserialize(&s).is_ok(), "{s:?}");
        }
    }

    // Expected values are worked out by hand from Adventure 5.2.0's `deserialize` (see the module doc).

    #[test]
    fn text_without_a_code_is_one_literal() {
        assert_eq!(json("plain"), r#""plain""#);
        // A `§` as the last character is not a code.
        assert_eq!(json("odd §"), r#""odd §""#);
        assert_eq!(json("§zUnknown"), r#""§zUnknown""#);
        assert_eq!(json(""), r#""""#);
    }

    #[test]
    fn a_colour_styles_what_follows_and_ends_the_run() {
        assert_eq!(json("§aGo"), r#"{"text":"Go","color":"green"}"#);
        assert_eq!(json("§AGo"), r#"{"text":"Go","color":"green"}"#);
        // Text before the first code stays in front, the runs are its children.
        assert_eq!(
            json("Hi §cred §9blue"),
            r#"{"text":"Hi ","extra":[{"text":"red ","color":"red"},{"text":"blue","color":"blue"}]}"#
        );
    }

    #[test]
    fn decorations_nest_around_what_follows() {
        assert_eq!(json("§lBold"), r#"{"text":"Bold","bold":true}"#);
        // Codes in a row style the same run.
        assert_eq!(
            json("§l§oBoth"),
            r#"{"text":"Both","bold":true,"italic":true}"#
        );
        // Text between codes: what comes before a decoration wraps the run it styles.
        assert_eq!(
            json("§lA§oB"),
            r#"{"text":"A","extra":[{"text":"B","italic":true}],"bold":true}"#
        );
    }

    #[test]
    fn reset_ends_the_styles_before_it() {
        assert_eq!(
            json("§cred§rplain"),
            r#"{"text":"","extra":[{"text":"red","color":"red"},"plain"]}"#
        );
    }

    #[test]
    fn hex_colours_in_both_forms() {
        assert_eq!(
            json("§#ff8800Orange"),
            r##"{"text":"Orange","color":"#FF8800"}"##
        );
        assert_eq!(
            json("§x§f§f§8§8§0§0Orange"),
            r##"{"text":"Orange","color":"#FF8800"}"##
        );
        // Too short for a hex colour, and not a colour: no code at all.
        assert_eq!(json("§#ff88"), r##""§#ff88""##);
    }
}
