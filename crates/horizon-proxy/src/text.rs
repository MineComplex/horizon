//! Chat components: built as JSON (the status and login wire form) and turned into NBT (the
//! configuration and play form), and the MiniMessage subset the MOTD uses.
use horizon_world::nbt::{JavaString, Tag};
use serde_json::{Map, Value, json};

/// A literal with a named or `#rrggbb` color.
pub fn colored(text: &str, color: &str) -> Tag {
    from_json(&json!({"text": text, "color": color}))
}

/// Components one after another.
pub fn join(parts: Vec<Tag>) -> Tag {
    Tag::compound([
        ("text", Tag::String(JavaString::from(""))),
        (
            "extra",
            Tag::List(parts.into_iter().map(as_compound).collect()),
        ),
    ])
}

/// A string component as `{"text": ...}`, so it can share a list with compounds.
fn as_compound(tag: Tag) -> Tag {
    match tag {
        Tag::Compound(_) => tag,
        other => Tag::compound([("text", other)]),
    }
}

/// A JSON component as NBT (ComponentSerialization over NbtOps).
pub fn from_json(value: &Value) -> Tag {
    match value {
        Value::Null => Tag::String(JavaString::from("")),
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Tag::Int(i as i32),
            None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
        },
        Value::String(s) => Tag::String(JavaString::from(s.as_str())),
        Value::Array(items) => {
            let tags: Vec<Tag> = items.iter().map(from_json).collect();
            if tags.windows(2).all(|w| w[0].id() == w[1].id()) {
                Tag::List(tags)
            } else {
                Tag::List(tags.into_iter().map(as_compound).collect())
            }
        }
        Value::Object(map) => Tag::compound(map.iter().map(|(k, v)| (k.as_str(), from_json(v)))),
    }
}

/// An NBT component as JSON (the login state's form).
pub fn to_json(tag: &Tag) -> Value {
    match tag {
        Tag::String(s) => Value::String(s.to_string_lossy()),
        // Components only hold booleans in bytes.
        Tag::Byte(b) => Value::Bool(*b != 0),
        Tag::Short(v) => json!(v),
        Tag::Int(v) => json!(v),
        Tag::Long(v) => json!(v),
        Tag::Float(bits) => json!(f32::from_bits(*bits)),
        Tag::Double(bits) => json!(f64::from_bits(*bits)),
        Tag::List(items) => Value::Array(items.iter().map(to_json).collect()),
        Tag::Compound(c) => Value::Object(
            c.iter()
                .map(|(k, v)| (k.to_string_lossy(), to_json(v)))
                .collect(),
        ),
        _ => Value::String(String::new()),
    }
}

/// The text of a component, for logs: literals and translation keys.
pub fn plain(tag: &Tag) -> String {
    let mut out = String::new();
    collect(tag, &mut out);
    out
}
fn collect(tag: &Tag, out: &mut String) {
    match tag {
        Tag::String(s) => out.push_str(&s.to_string_lossy()),
        Tag::Compound(_) => {
            for key in ["text", "translate"] {
                if let Some(Tag::String(s)) = tag.get(key) {
                    out.push_str(&s.to_string_lossy());
                }
            }
            if let Some(Tag::List(with)) = tag.get("with") {
                let args: Vec<String> = with.iter().map(plain).collect();
                if !args.is_empty() {
                    out.push_str(&format!(" [{}]", args.join(", ")));
                }
            }
            if let Some(Tag::List(extra)) = tag.get("extra") {
                extra.iter().for_each(|t| collect(t, out));
            }
        }
        Tag::List(items) => items.iter().for_each(|t| collect(t, out)),
        _ => {}
    }
}

/// The plain text of a JSON component (login disconnects of backends).
pub fn plain_json(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .map(|v| plain(&from_json(&v)))
        .unwrap_or_else(|_| json.to_owned())
}

const COLORS: [&str; 16] = [
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
];
const DECORATIONS: [(&str, &str); 10] = [
    ("bold", "bold"),
    ("b", "bold"),
    ("italic", "italic"),
    ("i", "italic"),
    ("em", "italic"),
    ("underlined", "underlined"),
    ("u", "underlined"),
    ("strikethrough", "strikethrough"),
    ("st", "strikethrough"),
    ("obfuscated", "obfuscated"),
];

/// The style a tag opens, or `None` for a tag this subset doesn't know (kept as text).
fn style(tag: &str) -> Option<(&'static str, Value)> {
    let tag = tag
        .strip_prefix("color:")
        .or_else(|| tag.strip_prefix("c:"))
        .unwrap_or(tag);
    let tag = tag.to_ascii_lowercase();
    let hex = |s: &str| s.len() == 7 && s[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if tag.starts_with('#') && hex(&tag) {
        return Some(("color", Value::String(tag)));
    }
    let tag = if tag == "grey" {
        "gray".to_owned()
    } else {
        tag.replace("dark_grey", "dark_gray")
    };
    if let Some(c) = COLORS.iter().find(|c| **c == tag) {
        return Some(("color", Value::String((*c).to_owned())));
    }
    DECORATIONS
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, key)| (*key, Value::Bool(true)))
}

/// MiniMessage's common tags: colors (`<red>`, `<#09add3>`, `<color:...>`), decorations,
/// `<reset>`, `<newline>`/`<br>` and closing tags. Other tags stay as text.
pub fn mini_message(input: &str) -> Value {
    let mut stack: Vec<(String, &'static str, Value)> = Vec::new();
    let mut parts: Vec<Value> = Vec::new();
    let mut text = String::new();
    let flush =
        |text: &mut String, stack: &[(String, &'static str, Value)], parts: &mut Vec<Value>| {
            if text.is_empty() {
                return;
            }
            let mut part = Map::new();
            part.insert("text".into(), Value::String(std::mem::take(text)));
            for (_, key, value) in stack {
                part.insert((*key).into(), value.clone());
            }
            parts.push(Value::Object(part));
        };
    let mut rest = input;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            text.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &after[..end];
        rest = &after[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            if let Some(i) = stack
                .iter()
                .rposition(|(n, ..)| n.eq_ignore_ascii_case(name))
            {
                flush(&mut text, &stack, &mut parts);
                stack.truncate(i);
                continue;
            }
        } else if tag.eq_ignore_ascii_case("reset") {
            flush(&mut text, &stack, &mut parts);
            stack.clear();
            continue;
        } else if tag.eq_ignore_ascii_case("newline") || tag.eq_ignore_ascii_case("br") {
            text.push('\n');
            continue;
        } else if let Some((key, value)) = style(tag) {
            flush(&mut text, &stack, &mut parts);
            stack.push((tag.to_owned(), key, value));
            continue;
        }
        text.push('<');
        text.push_str(tag);
        text.push('>');
    }
    text.push_str(rest);
    flush(&mut text, &stack, &mut parts);
    match parts.len() {
        0 => json!({"text": ""}),
        1 => parts.pop().unwrap(),
        _ => json!({"text": "", "extra": parts}),
    }
}

/// Shared.ERROR, SUCCESS and SOMETHING of the Network plugin: prefixes of its messages.
pub const ERROR: &str = "#FF3333[✘] &c";
pub const SUCCESS: &str = "#99FF33[✔] &f";
pub const SOMETHING: &str = "#99FF33[&l!#99FF33] &f";
/// Colors.CUBE.
pub const CUBE: &str = " &e▪ &f";

/// MessageUtil.toComponent: legacy text when it has color codes, MiniMessage otherwise.
pub fn message(text: &str) -> Tag {
    if text.contains('§') || text.contains('&') {
        legacy(text)
    } else {
        from_json(&mini_message(text))
    }
}

/// Lines joined with newlines (Component.join(newlines())).
pub fn lines<S: AsRef<str>>(lines: &[S]) -> Tag {
    let parts: Vec<Tag> = lines
        .iter()
        .enumerate()
        .flat_map(|(i, l)| {
            let line = message(l.as_ref());
            if i == 0 {
                vec![line]
            } else {
                vec![from_json(&json!({"text": "\n"})), line]
            }
        })
        .collect();
    join(parts)
}

fn legacy_color(code: char) -> Option<&'static str> {
    let i = code.to_digit(16)? as usize;
    Some(COLORS[i])
}

fn is_hex(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// RGBUtil.GradientPattern: `<#RRGGBB>text</#RRGGBB>` as one `§#RRGGBB` color per character.
fn gradients(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<#") {
        let open = &rest[start + 2..];
        let (Some(from), Some(close)) = (open.get(..6), open.get(6..7)) else {
            break;
        };
        let body_start = start + 9;
        let end_tag = rest[body_start.min(rest.len())..].find("</#");
        match (is_hex(from) && close == ">", end_tag) {
            (true, Some(len)) => {
                let body = &rest[body_start..body_start + len];
                let after = &rest[body_start + len + 3..];
                let Some(to) = after
                    .get(..6)
                    .filter(|t| is_hex(t) && after.get(6..7) == Some(">"))
                else {
                    out.push_str(&rest[..start + 2]);
                    rest = &rest[start + 2..];
                    continue;
                };
                out.push_str(&rest[..start]);
                out.push_str(&gradient(from, body, to));
                rest = &after[7..];
            }
            _ => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// RGBUtil.color.
fn gradient(from: &str, body: &str, to: &str) -> String {
    let rgb = |h: &str| u32::from_str_radix(h, 16).unwrap_or(0);
    let (a, b) = (rgb(from), rgb(to));
    let channel = |c: u32, shift: u32| ((c >> shift) & 0xff) as f32;
    let mut chars = Vec::new();
    let mut magic = String::new();
    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c == '§' {
            match it.peek().copied() {
                Some(m @ ('l' | 'o' | 'k' | 'm' | 'n')) => {
                    magic.push('§');
                    magic.push(m);
                    it.next();
                    continue;
                }
                Some('r') => {
                    magic.clear();
                    it.next();
                    continue;
                }
                _ => {}
            }
        }
        chars.push((c, magic.clone()));
    }
    if chars.len() == 1 {
        return format!("§#{from}{}", body);
    }
    let n = chars.len().max(2) - 1;
    let mut out = String::new();
    for (i, (c, magic)) in chars.into_iter().enumerate() {
        let mix = |shift| {
            let (x, y) = (channel(a, shift), channel(b, shift));
            (x + (y - x) / n as f32 * i as f32) as u32
        };
        out.push_str(&format!(
            "§#{:02X}{:02X}{:02X}{magic}{c}",
            mix(16),
            mix(8),
            mix(0)
        ));
    }
    out
}

/// MessageUtil.translate then LegacyComponentSerializer: `&`/`§` codes, `#RRGGBB` (also
/// `&#RRGGBB` and `§x§R§R§G§G§B§B`) and gradients, as a component.
pub fn legacy(text: &str) -> Tag {
    // ChatColor.translateAlternateColorCodes('&').
    let mut translated = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&'
            && it
                .peek()
                .is_some_and(|n| "0123456789AaBbCcDdEeFfKkLlMmNnOoRrXx".contains(*n))
        {
            translated.push('§');
            translated.push(it.next().unwrap().to_ascii_lowercase());
        } else {
            translated.push(c);
        }
    }
    let text = gradients(&translated);
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Value> = Vec::new();
    let mut style = Map::new();
    let mut current = String::new();
    let flush = |current: &mut String, style: &Map<String, Value>, spans: &mut Vec<Value>| {
        if !current.is_empty() {
            let mut span = style.clone();
            span.insert("text".into(), Value::String(std::mem::take(current)));
            spans.push(Value::Object(span));
        }
    };
    let hex_at = |i: usize| -> Option<String> {
        let s: String = chars.get(i..i + 6)?.iter().collect();
        is_hex(&s).then_some(s)
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let color = |hex: String, style: &mut Map<String, Value>| {
            style.clear();
            style.insert(
                "color".into(),
                Value::String(format!("#{}", hex.to_ascii_uppercase())),
            );
        };
        if c == '§' && i + 1 < chars.len() {
            let code = chars[i + 1];
            if code == '#' {
                if let Some(hex) = hex_at(i + 2) {
                    flush(&mut current, &style, &mut spans);
                    color(hex, &mut style);
                    i += 8;
                    continue;
                }
            }
            if code == 'x' {
                // §x§R§R§G§G§B§B.
                let digits: Option<String> = (0..6)
                    .map(
                        |k| match (chars.get(i + 2 + 2 * k), chars.get(i + 3 + 2 * k)) {
                            (Some('§'), Some(d)) if d.is_ascii_hexdigit() => Some(*d),
                            _ => None,
                        },
                    )
                    .collect();
                if let Some(hex) = digits {
                    flush(&mut current, &style, &mut spans);
                    color(hex, &mut style);
                    i += 14;
                    continue;
                }
            }
            let decoration = match code {
                'k' => Some("obfuscated"),
                'l' => Some("bold"),
                'm' => Some("strikethrough"),
                'n' => Some("underlined"),
                'o' => Some("italic"),
                _ => None,
            };
            if let Some(key) = decoration {
                flush(&mut current, &style, &mut spans);
                style.insert(key.into(), Value::Bool(true));
                i += 2;
                continue;
            }
            if code == 'r' {
                flush(&mut current, &style, &mut spans);
                style.clear();
                i += 2;
                continue;
            }
            if let Some(name) = legacy_color(code) {
                flush(&mut current, &style, &mut spans);
                style.clear();
                style.insert("color".into(), Value::String(name.into()));
                i += 2;
                continue;
            }
        }
        // RGBUtil.SolidPattern: #RRGGBB, also &#RRGGBB.
        if c == '#' || (c == '&' && chars.get(i + 1) == Some(&'#')) {
            let at = if c == '#' { i + 1 } else { i + 2 };
            if let Some(hex) = hex_at(at) {
                flush(&mut current, &style, &mut spans);
                color(hex, &mut style);
                i = at + 6;
                continue;
            }
        }
        current.push(c);
        i += 1;
    }
    flush(&mut current, &style, &mut spans);
    from_json(&match spans.len() {
        0 => json!({"text": ""}),
        _ => json!({"text": "", "extra": spans}),
    })
}

/// MessageUtil.translate as a string: `&` codes as `§`, `#RRGGBB` and gradients as
/// `§x§R§R§G§G§B§B` (the server list's sample names are plain strings).
pub fn translate(text: &str) -> String {
    let mut translated = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&'
            && it
                .peek()
                .is_some_and(|n| "0123456789AaBbCcDdEeFfKkLlMmNnOoRrXx".contains(*n))
        {
            translated.push('§');
            translated.push(it.next().unwrap().to_ascii_lowercase());
        } else {
            translated.push(c);
        }
    }
    let text = gradients(&translated);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let at = match (chars[i], chars.get(i + 1)) {
            ('§', Some('#')) => Some(i + 2),
            ('#', _) => Some(i + 1),
            _ => None,
        };
        let hex: Option<String> = at
            .and_then(|at| chars.get(at..at + 6))
            .map(|h| h.iter().collect())
            .filter(|h: &String| is_hex(h));
        if let (Some(at), Some(hex)) = (at, hex) {
            out.push_str("§x");
            for d in hex.chars() {
                out.push('§');
                out.push(d.to_ascii_lowercase());
            }
            i = at + 6;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// DefaultFontInfo: pixel width of a character of the default font.
fn char_width(c: char) -> usize {
    match c {
        '!' | '\'' | ',' | '.' | ':' | ';' | 'i' | 'l' | '|' => 1,
        '`' => 2,
        'I' | '[' | ']' | 't' | ' ' => 3,
        '(' | ')' | '*' | '<' | '>' | 'f' | 'k' | '{' | '}' => 4,
        '@' | '~' => 6,
        _ => 5,
    }
}

/// MessageUtil.center: spaces in front so the line sits in the middle of the chat.
pub fn center(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let (mut size, mut bold, mut code) = (0usize, false, false);
    for c in text.chars() {
        if c == '§' || c == '&' {
            code = true;
            continue;
        }
        if code {
            code = false;
            match c.to_ascii_lowercase() {
                'l' => bold = true,
                'r' => bold = false,
                c if c.is_ascii_hexdigit() => bold = false,
                _ => {}
            }
            continue;
        }
        size += char_width(c) + 1 + usize::from(bold);
    }
    let to_compensate = 154usize.saturating_sub(size / 2);
    let spaces = to_compensate.div_ceil(char_width(' ') + 1);
    format!("{}{text}", " ".repeat(spaces))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_default_motd() {
        assert_eq!(
            mini_message("<#09add3>A Velocity Server"),
            json!({"text": "A Velocity Server", "color": "#09add3"})
        );
    }

    #[test]
    fn nesting_closing_and_unknown_tags() {
        assert_eq!(
            mini_message("<red>a<bold>b</bold>c</red> <hover:x>d"),
            json!({"text": "", "extra": [
                {"text": "a", "color": "red"},
                {"text": "b", "color": "red", "bold": true},
                {"text": "c", "color": "red"},
                {"text": " <hover:x>d"},
            ]})
        );
        assert_eq!(mini_message("x<br>y"), json!({"text": "x\ny"}));
        assert_eq!(mini_message("a < b"), json!({"text": "a < b"}));
    }

    #[test]
    fn json_to_nbt_and_back_to_text() {
        let tag = from_json(&json!({"text": "", "extra": ["a", {"text": "b"}]}));
        assert_eq!(plain(&tag), "ab");
        assert_eq!(
            plain(&join(vec![
                colored("x: ", "red"),
                Tag::String(JavaString::from("y"))
            ])),
            "x: y"
        );
        assert_eq!(
            plain_json(r#"{"translate":"multiplayer.disconnect.banned"}"#),
            "multiplayer.disconnect.banned"
        );
    }

    #[test]
    fn legacy_codes_hex_and_gradients() {
        let tag = legacy("&cRed &lbold&r plain #FF0000hex");
        let json = to_json(&tag);
        assert_eq!(json["extra"][0], json!({"text": "Red ", "color": "red"}));
        assert_eq!(
            json["extra"][1],
            json!({"text": "bold", "color": "red", "bold": true})
        );
        assert_eq!(json["extra"][2], json!({"text": " plain "}));
        assert_eq!(json["extra"][3], json!({"text": "hex", "color": "#FF0000"}));
        assert_eq!(plain(&legacy(ERROR)), "[✘] ");
        let json = to_json(&legacy("<#FF0000>ab</#0000FF>"));
        assert_eq!(json["extra"][0]["color"], "#FF0000");
        assert_eq!(json["extra"][1]["color"], "#0000FF");
        let json = to_json(&legacy("§x§1§2§3§4§5§6x"));
        assert_eq!(json["extra"][0], json!({"text": "x", "color": "#123456"}));
        // No codes: MiniMessage.
        assert_eq!(
            to_json(&message("<red>hi")),
            json!({"text": "hi", "color": "red"})
        );
    }

    #[test]
    fn translated_strings() {
        assert_eq!(translate("&aHi #FF0000x"), "§aHi §x§f§f§0§0§0§0x");
        assert_eq!(plain(&legacy(&translate("&bA"))), "A");
    }

    #[test]
    fn centered_lines() {
        assert!(center("hi").starts_with(&" ".repeat(30)));
        assert_eq!(center(""), "");
    }
}
