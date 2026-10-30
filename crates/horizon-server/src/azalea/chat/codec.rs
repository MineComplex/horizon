//! `ComponentSerialization.CODEC` for 26.3: JSON (serde over `serde_json::Value`: the login state,
//! config files) and NBT (`nbt::Tag`: configuration and play packets, as a nameless root tag). The
//! rules are written once over [`Format`], the part of DFU's `DynamicOps` the component codecs use.
//!
//! Not checked here, for want of the registries and parsers they need: entity selectors, NBT paths
//! and block coordinates are kept as text; item and entity type ids are not looked up; item
//! components, dialogs and click payloads are kept as NBT; `object` contents are refused.

use std::{
    borrow::Cow,
    io::{self, Cursor, Write},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    azalea::{
        buf::{AzBuf, BufReadError},
        chat::{
            FormattedText,
            click_event::ClickEvent,
            contents::{
                DataSource, KeybindComponent, NbtComponent, ScoreComponent, SelectorComponent,
            },
            hover_event::HoverEvent,
            style::{Style, TextColor},
            text_component::TextComponent,
            translatable_component::{PrimitiveOrComponent, TranslatableComponent},
        },
    },
    nbt::{self, JavaString, Limits, Tag},
    util::number_text::float_to_string,
};

type R<T> = Result<T, String>;

/// Read limits for components from the network: `NbtAccounter.defaultQuota()` (2 MiB, depth 512),
/// with the quota counted in bytes read rather than Java's estimated heap size.
const READ_LIMITS: Limits = Limits {
    max_bytes: 2 * 1024 * 1024,
    max_nodes: 1_000_000,
    max_depth: 512,
};

/// Writes a component as the network does (`ComponentSerialization.STREAM_CODEC`): its NBT form as
/// a nameless root tag.
pub fn write_component(component: &FormattedText, buf: &mut Vec<u8>) -> Result<(), nbt::Error> {
    buf.extend(nbt::write_network(&component.to_nbt(), Limits::default())?);
    Ok(())
}

/// Reads a component written by [`write_component`].
pub fn read_component(buf: &mut Cursor<&[u8]>) -> Result<FormattedText, BufReadError> {
    let start = usize::try_from(buf.position()).unwrap_or(usize::MAX);
    let bytes = buf.get_ref().get(start..).unwrap_or_default();
    let (tag, used) =
        nbt::read_network(bytes, READ_LIMITS).map_err(|e| BufReadError::Custom(e.to_string()))?;
    buf.set_position((start + used) as u64);
    if tag == Tag::End {
        return Err(BufReadError::Custom(
            "Expected a component, got TAG_End".into(),
        ));
    }
    FormattedText::from_nbt(&tag)
        .map_err(|e| BufReadError::Custom(format!("Failed to decode: {e}")))
}

impl AzBuf for FormattedText {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        read_component(buf)
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        let mut out = Vec::new();
        write_component(self, &mut out).map_err(io::Error::other)?;
        buf.write_all(&out)
    }
}

impl FormattedText {
    /// The NBT form (`CODEC` over `NbtOps`): a plain literal is a string tag.
    pub fn to_nbt(&self) -> Tag {
        encode(self)
    }
    pub fn from_nbt(tag: &Tag) -> Result<Self, String> {
        decode(tag)
    }
    /// The JSON form (`CODEC` over `JsonOps`), with vanilla's field order.
    pub fn to_json(&self) -> Value {
        encode(self)
    }
    pub fn from_json(json: &Value) -> Result<Self, String> {
        decode(json)
    }
}

impl Serialize for FormattedText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FormattedText {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Self::from_json(&Value::deserialize(de)?).map_err(de::Error::custom)
    }
}

/// A Java `Number` as DFU passes it between formats.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Number {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl Number {
    /// `Number.intValue()`.
    fn int(self) -> i32 {
        match self {
            Self::Byte(v) => v.into(),
            Self::Short(v) => v.into(),
            Self::Int(v) => v,
            Self::Long(v) => v as i32,
            Self::Float(v) => v as i32,
            Self::Double(v) => v as i32,
        }
    }
    fn double(self) -> f64 {
        match self {
            Self::Byte(v) => v.into(),
            Self::Short(v) => v.into(),
            Self::Int(v) => v.into(),
            Self::Long(v) => v as f64,
            Self::Float(v) => v.into(),
            Self::Double(v) => v,
        }
    }
    fn integral(v: i64) -> Self {
        if let Ok(v) = i8::try_from(v) {
            Self::Byte(v)
        } else if let Ok(v) = i16::try_from(v) {
            Self::Short(v)
        } else if let Ok(v) = i32::try_from(v) {
            Self::Int(v)
        } else {
            Self::Long(v)
        }
    }
    /// A JSON number as `JsonOps.convertTo` hands it on: an integral value as the smallest of
    /// byte/short/int/long holding it, anything else as a float when that is exact, else a double.
    fn from_json(n: &serde_json::Number) -> Self {
        if let Some(v) = n.as_i64() {
            return Self::integral(v);
        }
        let d = n.as_f64().unwrap_or(f64::NAN);
        if d.fract() == 0.0 && (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&d) {
            Self::integral(d as i64)
        } else if f64::from(d as f32) == d {
            Self::Float(d as f32)
        } else {
            Self::Double(d)
        }
    }
    fn to_json(self) -> Value {
        let float = match self {
            Self::Byte(v) => return v.into(),
            Self::Short(v) => return v.into(),
            Self::Int(v) => return v.into(),
            Self::Long(v) => return v.into(),
            // Gson writes `Float.toString`: 0.1f is `0.1`, not its double value.
            Self::Float(v) => float_to_string(v).parse().unwrap_or(f64::NAN),
            Self::Double(v) => v,
        };
        serde_json::Number::from_f64(float).map_or(Value::Null, Value::Number)
    }
}

/// One of the formats components are written in.
trait Format: Clone + Sized {
    fn as_string(&self) -> Option<String>;
    fn as_number(&self) -> Option<Number>;
    fn as_bool(&self) -> Option<bool>;
    fn as_list(&self) -> Option<Cow<'_, [Self]>>;
    /// A map's value for `key`; JSON nulls count as absent.
    fn field(&self, key: &str) -> Option<&Self>;
    fn is_map(&self) -> bool;
    /// The value converted to NBT (`Dynamic.convert`), for the parts kept untyped.
    fn to_tag(&self) -> R<Tag>;
    fn from_tag(tag: &Tag) -> Self;
    fn string(value: &str) -> Self;
    fn number(value: Number) -> Self;
    fn boolean(value: bool) -> Self;
    fn list(values: Vec<Self>) -> Self;
    fn int_array(values: [i32; 4]) -> Self;
    fn map(entries: Vec<(&str, Self)>) -> Self;
}

impl Format for Value {
    fn as_string(&self) -> Option<String> {
        self.as_str().map(str::to_owned)
    }
    fn as_number(&self) -> Option<Number> {
        match self {
            Value::Number(n) => Some(Number::from_json(n)),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        Value::as_bool(self)
    }
    fn as_list(&self) -> Option<Cow<'_, [Self]>> {
        self.as_array().map(|a| Cow::Borrowed(a.as_slice()))
    }
    fn field(&self, key: &str) -> Option<&Self> {
        self.get(key).filter(|v| !v.is_null())
    }
    fn is_map(&self) -> bool {
        self.is_object()
    }
    fn to_tag(&self) -> R<Tag> {
        Ok(match self {
            Value::Null => return Err("null has no NBT form".into()),
            Value::Bool(b) => Tag::boolean(*b),
            Value::Number(n) => Tag::number(Number::from_json(n)),
            Value::String(s) => Tag::string(s),
            Value::Array(a) => Tag::List(a.iter().map(Format::to_tag).collect::<R<_>>()?),
            Value::Object(m) => Tag::Compound(
                m.iter()
                    .map(|(k, v)| Ok((JavaString::from(k.as_str()), v.to_tag()?)))
                    .collect::<R<_>>()?,
            ),
        })
    }
    fn from_tag(tag: &Tag) -> Self {
        match tag {
            Tag::End => Value::Null,
            Tag::Byte(v) => (*v).into(),
            Tag::Short(v) => (*v).into(),
            Tag::Int(v) => (*v).into(),
            Tag::Long(v) => (*v).into(),
            Tag::Float(bits) => Number::Float(f32::from_bits(*bits)).to_json(),
            Tag::Double(bits) => Number::Double(f64::from_bits(*bits)).to_json(),
            Tag::ByteArray(v) => v.iter().map(|&b| Value::from(b as i8)).collect(),
            Tag::String(s) => s.to_string_lossy().into(),
            Tag::List(v) => v.iter().map(Value::from_tag).collect(),
            Tag::Compound(m) => Value::Object(
                m.iter()
                    .map(|(k, v)| (k.to_string_lossy(), Value::from_tag(v)))
                    .collect(),
            ),
            Tag::IntArray(v) => v.iter().copied().collect(),
            Tag::LongArray(v) => v.iter().copied().collect(),
        }
    }
    fn string(value: &str) -> Self {
        value.into()
    }
    fn number(value: Number) -> Self {
        value.to_json()
    }
    fn boolean(value: bool) -> Self {
        value.into()
    }
    fn list(values: Vec<Self>) -> Self {
        Value::Array(values)
    }
    fn int_array(values: [i32; 4]) -> Self {
        values.into_iter().collect()
    }
    fn map(entries: Vec<(&str, Self)>) -> Self {
        Value::Object(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect::<Map<_, _>>(),
        )
    }
}

impl Format for Tag {
    fn as_string(&self) -> Option<String> {
        match self {
            Tag::String(s) => Some(s.to_string_lossy()),
            _ => None,
        }
    }
    fn as_number(&self) -> Option<Number> {
        Some(match *self {
            Tag::Byte(v) => Number::Byte(v),
            Tag::Short(v) => Number::Short(v),
            Tag::Int(v) => Number::Int(v),
            Tag::Long(v) => Number::Long(v),
            Tag::Float(bits) => Number::Float(f32::from_bits(bits)),
            Tag::Double(bits) => Number::Double(f64::from_bits(bits)),
            _ => return None,
        })
    }
    /// `NbtOps.getBooleanValue`: any number, true when not zero.
    fn as_bool(&self) -> Option<bool> {
        self.as_number().map(|n| n.double() != 0.0)
    }
    fn as_list(&self) -> Option<Cow<'_, [Self]>> {
        Some(match self {
            Tag::List(v) => Cow::Borrowed(v.as_slice()),
            Tag::ByteArray(v) => v.iter().map(|&b| Tag::Byte(b as i8)).collect(),
            Tag::IntArray(v) => v.iter().map(|&i| Tag::Int(i)).collect(),
            Tag::LongArray(v) => v.iter().map(|&l| Tag::Long(l)).collect(),
            _ => return None,
        })
    }
    fn field(&self, key: &str) -> Option<&Self> {
        self.get(key)
    }
    fn is_map(&self) -> bool {
        matches!(self, Tag::Compound(_))
    }
    fn to_tag(&self) -> R<Tag> {
        Ok(self.clone())
    }
    fn from_tag(tag: &Tag) -> Self {
        tag.clone()
    }
    fn string(value: &str) -> Self {
        Tag::String(value.into())
    }
    fn number(value: Number) -> Self {
        match value {
            Number::Byte(v) => Tag::Byte(v),
            Number::Short(v) => Tag::Short(v),
            Number::Int(v) => Tag::Int(v),
            Number::Long(v) => Tag::Long(v),
            Number::Float(v) => Tag::Float(v.to_bits()),
            Number::Double(v) => Tag::Double(v.to_bits()),
        }
    }
    fn boolean(value: bool) -> Self {
        Tag::Byte(value.into())
    }
    fn list(values: Vec<Self>) -> Self {
        Tag::List(values)
    }
    fn int_array(values: [i32; 4]) -> Self {
        Tag::IntArray(values.to_vec())
    }
    fn map(entries: Vec<(&str, Self)>) -> Self {
        Tag::compound(entries)
    }
}

fn required<'a, F: Format>(v: &'a F, key: &str) -> R<&'a F> {
    v.field(key).ok_or_else(|| format!("No key {key}"))
}
fn as_string<F: Format>(v: &F) -> R<String> {
    v.as_string().ok_or_else(|| "Not a string".into())
}
fn as_bool<F: Format>(v: &F) -> R<bool> {
    v.as_bool().ok_or_else(|| "Not a boolean".into())
}
fn as_int<F: Format>(v: &F) -> R<i32> {
    v.as_number()
        .map(Number::int)
        .ok_or_else(|| "Not a number".into())
}
fn string_field<F: Format>(v: &F, key: &str) -> R<String> {
    as_string(required(v, key)?).map_err(|e| format!("{key}: {e}"))
}
/// `optionalFieldOf`: absent is `None`, a bad value an error.
fn optional<F: Format, T>(v: &F, key: &str, read: impl FnOnce(&F) -> R<T>) -> R<Option<T>> {
    v.field(key)
        .map(read)
        .transpose()
        .map_err(|e| format!("{key}: {e}"))
}
/// `lenientOptionalFieldOf`: a bad value counts as absent.
fn lenient<F: Format, T>(v: &F, key: &str, read: impl FnOnce(&F) -> R<T>) -> Option<T> {
    v.field(key).and_then(|f| read(f).ok())
}

/// `Identifier.CODEC`: `namespace:path`, `minecraft` when the namespace is left out; returned in full.
fn identifier(s: &str) -> R<String> {
    let (namespace, path) = match s.split_once(':') {
        Some(("", path)) => ("minecraft", path),
        Some(pair) => pair,
        None => ("minecraft", s),
    };
    let valid = |text: &str, slash: bool| {
        text.bytes().all(|c| {
            matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.') || (slash && c == b'/')
        })
    };
    if valid(namespace, false) && valid(path, true) {
        Ok(format!("{namespace}:{path}"))
    } else {
        Err(format!("Not a valid resource location: {s}"))
    }
}

/// `ExtraCodecs.CHAT_STRING`: no `§`, control characters or DEL.
fn chat_string(s: String) -> R<String> {
    match s.chars().find(|&c| c == '§' || c < ' ' || c == '\u{7f}') {
        Some(c) => Err(format!("Disallowed chat character: '{c}'")),
        None => Ok(s),
    }
}

/// `Util.parseAndValidateUntrustedUri`: an `http` or `https` URI. Java's `URI` grammar is only
/// approximated: a scheme, something after it, and none of the characters a URI never holds.
fn untrusted_url(url: String) -> R<String> {
    let scheme = url
        .split_once(':')
        .map(|(scheme, rest)| (scheme, rest.is_empty()));
    let Some((scheme, false)) = scheme.filter(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    }) else {
        return Err(format!("Missing protocol in URI: {url}"));
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(format!("Unsupported protocol in URI: {url}"));
    }
    if url
        .chars()
        .any(|c| c.is_control() || " \"<>\\^`{|}".contains(c))
    {
        return Err(format!("Illegal character in URI: {url}"));
    }
    Ok(url)
}

fn decode<F: Format>(v: &F) -> R<FormattedText> {
    if let Some(text) = v.as_string() {
        return Ok(TextComponent::new(text).into());
    }
    if let Some(items) = v.as_list() {
        let (first, rest) = items.split_first().ok_or("List must have contents")?;
        let mut component = decode(first)?;
        for item in rest {
            component.append(decode(item)?);
        }
        return Ok(component);
    }
    if !v.is_map() {
        return Err("Not a component".into());
    }
    let mut component = decode_contents(v)?;
    if let Some(extra) = v.field("extra") {
        let items = extra.as_list().ok_or("extra: not a list")?;
        if items.is_empty() {
            return Err("extra: List must have contents".into());
        }
        component.get_base_mut().siblings = items.iter().map(decode).collect::<R<_>>()?;
    }
    *component.get_base_mut().style = decode_style(v)?;
    Ok(component)
}

/// The content types in the order vanilla tries them when no `type` is given.
const CONTENT_TYPES: [&str; 7] = [
    "text",
    "translatable",
    "keybind",
    "score",
    "selector",
    "nbt",
    "object",
];

fn decode_contents<F: Format>(v: &F) -> R<FormattedText> {
    if let Some(kind) = v.field("type") {
        return decode_contents_of(&as_string(kind)?, v);
    }
    CONTENT_TYPES
        .iter()
        .find_map(|kind| decode_contents_of(kind, v).ok())
        .ok_or_else(|| "No matching codec found".into())
}

fn decode_contents_of<F: Format>(kind: &str, v: &F) -> R<FormattedText> {
    let base = Default::default();
    Ok(match kind {
        "text" => TextComponent::new(string_field(v, "text")?).into(),
        "translatable" => TranslatableComponent {
            base,
            key: string_field(v, "translate")?,
            fallback: lenient(v, "fallback", as_string),
            args: optional(v, "with", |with| {
                let items = with.as_list().ok_or("Not a list")?;
                items.iter().map(decode_arg).collect::<R<_>>()
            })?
            .unwrap_or_default(),
        }
        .into(),
        "keybind" => FormattedText::Keybind(KeybindComponent {
            base,
            keybind: string_field(v, "keybind")?,
        }),
        "score" => {
            let score = required(v, "score")?;
            FormattedText::Score(ScoreComponent {
                base,
                name: string_field(score, "name")?,
                objective: string_field(score, "objective")?,
            })
        }
        "selector" => FormattedText::Selector(SelectorComponent {
            base,
            selector: string_field(v, "selector")?,
            separator: optional(v, "separator", decode)?.map(Box::new),
        }),
        "nbt" => {
            let component = NbtComponent {
                base,
                path: string_field(v, "nbt")?,
                interpret: lenient(v, "interpret", as_bool).unwrap_or(false),
                plain: lenient(v, "plain", as_bool).unwrap_or(false),
                separator: lenient(v, "separator", decode).map(Box::new),
                source: decode_source(v)?,
            };
            if component.interpret && component.plain {
                return Err("'interpret' and 'plain' flags can't be both on".into());
            }
            FormattedText::Nbt(component)
        }
        "object" => return Err("object contents are not supported yet".into()),
        _ => return Err(format!("Unknown content type {kind}")),
    })
}

fn decode_source<F: Format>(v: &F) -> R<DataSource> {
    let read = |kind: &str| -> R<DataSource> {
        let value = string_field(v, kind)?;
        Ok(match kind {
            "entity" => DataSource::Entity(value),
            "block" => DataSource::Block(value),
            "storage" => DataSource::Storage(identifier(&value)?),
            _ => return Err(format!("Unknown data source {kind}")),
        })
    };
    if let Some(kind) = v.field("source") {
        return read(&as_string(kind)?);
    }
    ["entity", "block", "storage"]
        .iter()
        .find_map(|kind| read(kind).ok())
        .ok_or_else(|| "No matching data source".into())
}

/// `TranslatableContents.ARG_CODEC`: a string, number or boolean as it is, else a component, read
/// back as a string when it is a plain literal.
fn decode_arg<F: Format>(v: &F) -> R<PrimitiveOrComponent> {
    if let Some(text) = v.as_string() {
        return Ok(PrimitiveOrComponent::String(text));
    }
    if let Some(n) = v.as_number() {
        return Ok(match n {
            Number::Byte(v) => PrimitiveOrComponent::Byte(v),
            Number::Short(v) => PrimitiveOrComponent::Short(v),
            Number::Int(v) => PrimitiveOrComponent::Integer(v),
            Number::Long(v) => PrimitiveOrComponent::Long(v),
            Number::Float(v) => PrimitiveOrComponent::Float(v),
            Number::Double(v) => PrimitiveOrComponent::Double(v),
        });
    }
    if let Some(b) = v.as_bool() {
        return Ok(PrimitiveOrComponent::Boolean(b));
    }
    let component = decode(v)?;
    Ok(match component.try_collapse_to_string() {
        Some(text) => PrimitiveOrComponent::String(text.to_owned()),
        None => PrimitiveOrComponent::FormattedText(component),
    })
}

fn decode_style<F: Format>(v: &F) -> R<Style> {
    Ok(Style {
        color: optional(v, "color", |c| {
            let name = as_string(c)?;
            TextColor::parse(&name).ok_or_else(|| format!("Invalid color: {name}"))
        })?,
        shadow_color: optional(v, "shadow_color", decode_argb)?,
        bold: optional(v, "bold", as_bool)?,
        italic: optional(v, "italic", as_bool)?,
        underlined: optional(v, "underlined", as_bool)?,
        strikethrough: optional(v, "strikethrough", as_bool)?,
        obfuscated: optional(v, "obfuscated", as_bool)?,
        click_event: optional(v, "click_event", decode_click)?,
        hover_event: optional(v, "hover_event", decode_hover)?,
        insertion: optional(v, "insertion", as_string)?,
        font: optional(v, "font", |f| identifier(&as_string(f)?))?,
    })
}

/// `ExtraCodecs.ARGB_COLOR_CODEC`: an int, or `[r, g, b, a]` floats from 0 to 1.
fn decode_argb<F: Format>(v: &F) -> R<i32> {
    if let Some(n) = v.as_number() {
        return Ok(n.int());
    }
    let list = v.as_list().ok_or("Not a number or a list")?;
    let floats: Vec<f32> = list
        .iter()
        .map(|f| {
            f.as_number()
                .map(|n| n.double() as f32)
                .ok_or("Not a number")
        })
        .collect::<Result<_, _>>()?;
    let [r, g, b, a] = floats[..] else {
        return Err(format!(
            "Input is not a list of 4 elements: {}",
            floats.len()
        ));
    };
    let channel = |f: f32| ((f * 255.0).floor() as i32) & 0xFF;
    Ok(channel(a) << 24 | channel(r) << 16 | channel(g) << 8 | channel(b))
}

fn decode_click<F: Format>(v: &F) -> R<ClickEvent> {
    Ok(match string_field(v, "action")?.as_str() {
        "open_url" => ClickEvent::OpenUrl {
            url: untrusted_url(string_field(v, "url")?)?,
        },
        "run_command" => ClickEvent::RunCommand {
            command: chat_string(string_field(v, "command")?)?,
        },
        "suggest_command" => ClickEvent::SuggestCommand {
            command: chat_string(string_field(v, "command")?)?,
        },
        "show_dialog" => ClickEvent::ShowDialog {
            dialog: required(v, "dialog")?.to_tag()?,
        },
        "change_page" => match as_int(required(v, "page")?)? {
            page @ 1.. => ClickEvent::ChangePage { page },
            page => return Err(format!("Value must be positive: {page}")),
        },
        "copy_to_clipboard" => ClickEvent::CopyToClipboard {
            value: string_field(v, "value")?,
        },
        "custom" => ClickEvent::Custom {
            id: identifier(&string_field(v, "id")?)?,
            payload: optional(v, "payload", F::to_tag)?,
        },
        "open_file" => return Err("Click event type not allowed: OPEN_FILE".into()),
        action => return Err(format!("Unknown click event action: {action}")),
    })
}

fn decode_hover<F: Format>(v: &F) -> R<HoverEvent> {
    Ok(match string_field(v, "action")?.as_str() {
        "show_text" => HoverEvent::ShowText {
            value: Box::new(decode(required(v, "value")?)?),
        },
        "show_item" => HoverEvent::ShowItem {
            id: identifier(&string_field(v, "id")?)?,
            count: match optional(v, "count", as_int)?.unwrap_or(1) {
                count @ 1..=99 => count,
                count => return Err(format!("Value must be within range [1;99]: {count}")),
            },
            components: optional(v, "components", F::to_tag)?,
        },
        "show_entity" => HoverEvent::ShowEntity {
            id: identifier(&string_field(v, "id")?)?,
            uuid: decode_uuid(required(v, "uuid")?)?,
            name: optional(v, "name", decode)?.map(Box::new),
        },
        action => return Err(format!("Unknown hover event action: {action}")),
    })
}

/// `UUIDUtil.LENIENT_CODEC`: four ints, or the usual text form.
fn decode_uuid<F: Format>(v: &F) -> R<Uuid> {
    if let Some(text) = v.as_string() {
        return Uuid::parse_str(&text).map_err(|e| e.to_string());
    }
    let ints = v.as_list().ok_or("Not a UUID")?;
    let ints: Vec<i32> = ints.iter().map(as_int).collect::<R<_>>()?;
    let [a, b, c, d] = ints[..] else {
        return Err(format!("Input is not a list of 4 ints: {}", ints.len()));
    };
    let pair = |hi: i32, lo: i32| u64::from(hi as u32) << 32 | u64::from(lo as u32);
    Ok(Uuid::from_u64_pair(pair(a, b), pair(c, d)))
}

fn encode<F: Format>(component: &FormattedText) -> F {
    if let Some(text) = component.try_collapse_to_string() {
        return F::string(text);
    }
    let mut map = Vec::new();
    match component {
        FormattedText::Text(c) => map.push(("text", F::string(&c.text))),
        FormattedText::Translatable(c) => {
            map.push(("translate", F::string(&c.key)));
            if let Some(fallback) = &c.fallback {
                map.push(("fallback", F::string(fallback)));
            }
            if !c.args.is_empty() {
                map.push(("with", F::list(c.args.iter().map(encode_arg).collect())));
            }
        }
        FormattedText::Keybind(c) => map.push(("keybind", F::string(&c.keybind))),
        FormattedText::Score(c) => {
            let score = F::map(vec![
                ("name", F::string(&c.name)),
                ("objective", F::string(&c.objective)),
            ]);
            map.push(("score", score));
        }
        FormattedText::Selector(c) => {
            map.push(("selector", F::string(&c.selector)));
            if let Some(separator) = &c.separator {
                map.push(("separator", encode(separator)));
            }
        }
        FormattedText::Nbt(c) => {
            map.push(("nbt", F::string(&c.path)));
            if c.interpret {
                map.push(("interpret", F::boolean(true)));
            }
            if c.plain {
                map.push(("plain", F::boolean(true)));
            }
            if let Some(separator) = &c.separator {
                map.push(("separator", encode(separator)));
            }
            map.push((c.source.key(), F::string(c.source.value())));
        }
    }
    let base = component.get_base();
    if !base.siblings.is_empty() {
        map.push(("extra", F::list(base.siblings.iter().map(encode).collect())));
    }
    encode_style(&base.style, &mut map);
    F::map(map)
}

fn encode_arg<F: Format>(arg: &PrimitiveOrComponent) -> F {
    match arg {
        PrimitiveOrComponent::Boolean(v) => F::boolean(*v),
        PrimitiveOrComponent::Byte(v) => F::number(Number::Byte(*v)),
        PrimitiveOrComponent::Short(v) => F::number(Number::Short(*v)),
        PrimitiveOrComponent::Integer(v) => F::number(Number::Int(*v)),
        PrimitiveOrComponent::Long(v) => F::number(Number::Long(*v)),
        PrimitiveOrComponent::Float(v) => F::number(Number::Float(*v)),
        PrimitiveOrComponent::Double(v) => F::number(Number::Double(*v)),
        PrimitiveOrComponent::String(v) => F::string(v),
        PrimitiveOrComponent::FormattedText(c) => encode(c),
    }
}

fn encode_style<F: Format>(style: &Style, map: &mut Vec<(&str, F)>) {
    if let Some(color) = &style.color {
        map.push(("color", F::string(&color.serialize())));
    }
    if let Some(argb) = style.shadow_color {
        map.push(("shadow_color", F::number(Number::Int(argb))));
    }
    let flags = [
        ("bold", style.bold),
        ("italic", style.italic),
        ("underlined", style.underlined),
        ("strikethrough", style.strikethrough),
        ("obfuscated", style.obfuscated),
    ];
    for (key, flag) in flags {
        if let Some(flag) = flag {
            map.push((key, F::boolean(flag)));
        }
    }
    if let Some(click) = &style.click_event {
        map.push(("click_event", encode_click(click)));
    }
    if let Some(hover) = &style.hover_event {
        map.push(("hover_event", encode_hover(hover)));
    }
    if let Some(insertion) = &style.insertion {
        map.push(("insertion", F::string(insertion)));
    }
    if let Some(font) = &style.font {
        map.push(("font", F::string(font)));
    }
}

/// An event's fields, then its `action` (the order `dispatch` writes them in).
fn encode_click<F: Format>(event: &ClickEvent) -> F {
    let mut map = match event {
        ClickEvent::OpenUrl { url } => vec![("url", F::string(url))],
        ClickEvent::RunCommand { command } | ClickEvent::SuggestCommand { command } => {
            vec![("command", F::string(command))]
        }
        ClickEvent::ShowDialog { dialog } => vec![("dialog", F::from_tag(dialog))],
        ClickEvent::ChangePage { page } => vec![("page", F::number(Number::Int(*page)))],
        ClickEvent::CopyToClipboard { value } => vec![("value", F::string(value))],
        ClickEvent::Custom { id, payload } => {
            let mut map = vec![("id", F::string(id))];
            map.extend(payload.as_ref().map(|p| ("payload", F::from_tag(p))));
            map
        }
    };
    map.push(("action", F::string(event.action())));
    F::map(map)
}

fn encode_hover<F: Format>(event: &HoverEvent) -> F {
    let mut map = match event {
        HoverEvent::ShowText { value } => vec![("value", encode(value))],
        HoverEvent::ShowItem {
            id,
            count,
            components,
        } => {
            let mut map = vec![("id", F::string(id))];
            if *count != 1 {
                map.push(("count", F::number(Number::Int(*count))));
            }
            map.extend(components.as_ref().map(|c| ("components", F::from_tag(c))));
            map
        }
        HoverEvent::ShowEntity { id, uuid, name } => {
            let (hi, lo) = uuid.as_u64_pair();
            let ints = [(hi >> 32) as i32, hi as i32, (lo >> 32) as i32, lo as i32];
            let mut map = vec![("id", F::string(id)), ("uuid", F::int_array(ints))];
            map.extend(name.as_ref().map(|n| ("name", encode(n))));
            map
        }
    };
    map.push(("action", F::string(event.action())));
    F::map(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Components run through the real 26.3 classes (`ComponentSerialization.CODEC` over `JsonOps`
    /// and `NbtOps`, written with `TRUSTED_CONTEXT_FREE_STREAM_CODEC`), one per line: the input's
    /// format and text (JSON, or SNBT read with `TagParser`), the JSON vanilla writes back (`ERR`
    /// when it refuses the input) and the network bytes.
    const CASES: &str = include_str!("testdata/components.tsv");

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn matches_vanilla() {
        for line in CASES.lines() {
            let [format, input, json, nbt] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("bad test line: {line}");
            };
            let decoded = match format {
                "json" => FormattedText::from_json(&serde_json::from_str(input).unwrap()),
                _ => FormattedText::from_nbt(&nbt::parse_snbt(input).unwrap().0),
            };
            if json == "ERR" {
                assert!(decoded.is_err(), "{input} is refused, got {decoded:?}");
                continue;
            }
            let component = decoded.unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(serde_json::to_string(&component).unwrap(), json, "{input}");
            if format == "json" {
                assert_eq!(
                    FormattedText::from_json(&component.to_json()),
                    Ok(component.clone())
                );
            }
            // Java writes compounds in hash order: compare the trees.
            let bytes = hex(nbt);
            let (tag, _) = nbt::read_network(&bytes, Limits::default()).unwrap();
            assert_eq!(component.to_nbt(), tag, "{input}");
            let mut cursor = Cursor::new(bytes.as_slice());
            let read = read_component(&mut cursor).unwrap();
            assert_eq!(cursor.position() as usize, bytes.len());
            // Booleans come back from NBT as bytes, so compare what they write.
            assert_eq!(read.to_nbt(), tag, "{input}");
            let mut written = Vec::new();
            component.azalea_write(&mut written).unwrap();
            let again = FormattedText::azalea_read(&mut Cursor::new(written.as_slice())).unwrap();
            assert_eq!(again.to_nbt(), tag, "{input}");
        }
    }

    #[test]
    fn built_components() {
        let mut c: FormattedText = TranslatableComponent::new(
            "k".into(),
            vec![
                PrimitiveOrComponent::Float(0.1),
                PrimitiveOrComponent::Double(1e300),
                PrimitiveOrComponent::FormattedText("lit".into()),
            ],
        )
        .into();
        c.get_base_mut().style.shadow_color = Some(-1);
        // Gson writes the double as `1.0E300`: the same number.
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"translate":"k","with":[0.1,1e+300,"lit"],"shadow_color":-1}"#
        );
        let mut plain = Vec::new();
        write_component(&"Hello".into(), &mut plain).unwrap();
        assert_eq!(plain, hex("08000548656c6c6f"));
        let keybind = FormattedText::from_json(&serde_json::json!({"keybind": "key.jump"}));
        assert_eq!(keybind.unwrap().to_string(), "key.jump");
        let score = serde_json::json!({"score": {"name": "a", "objective": "b"}, "extra": ["!"]});
        assert_eq!(FormattedText::from_json(&score).unwrap().to_string(), "!");
    }

    #[test]
    fn network_errors_and_literals() {
        for bytes in ["00", "0800", "0a", "03000000"] {
            let read = read_component(&mut Cursor::new(hex(bytes).as_slice()));
            assert!(read.is_err(), "{bytes}");
        }
        // A literal keeps its `§`: vanilla leaves legacy codes to the renderer.
        let tag = Tag::String("§aHi".into());
        assert_eq!(FormattedText::from_nbt(&tag).unwrap(), "§aHi".into());
    }
}
