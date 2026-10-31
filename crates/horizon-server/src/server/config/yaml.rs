//! The YAML files Leaf keeps, read and written as SnakeYAML (YAML 1.1) does: every config library Leaf
//! uses (Configurate for Paper and Gale, Bukkit's `YamlConfiguration` for Spigot and Purpur,
//! ConfigurationMaster for Leaf) parses with SnakeYAML.
//!
//! Crate: `saphyr` (with its own parser, `saphyr-parser`). Compared: `serde_yaml` (archived in 2024),
//! `serde_yaml_ng` and `serde_norway` (its forks, no release since May and December 2024), and
//! `serde-saphyr` (maintained, but serde-only: no document tree to keep the keys this port does not know
//! yet). None of them is YAML 1.1: `yes`, `on`, `0x1F`, `1_000`, `012` (octal) and `1:30` (base 60) are
//! strings to them and typed values to SnakeYAML. saphyr is maintained (0.3.3, October 2026), loads an
//! ordered tree and can keep every scalar as its source text and style (`early_parse(false)`), so the
//! types are resolved here with SnakeYAML's own patterns (copied from `Resolver` in SnakeYAML 2.6), and
//! the values built as `SafeConstructor` builds them; both are checked against SnakeYAML 2.6 on Java 25
//! in the tests. No crate writes comments: the header is written as text before the document.
//!
//! Known differences from the Java writers: comments in a loaded file are not kept (Configurate drops
//! them too; Bukkit and ConfigurationMaster keep them); list items are indented under their key
//! (`key:\n  - a`) where SnakeYAML writes `key:\n- a`; dates (`2026-10-30`) stay text where SnakeYAML
//! builds a `Date`; integers beyond 128 bits stay text where SnakeYAML builds a `BigInteger`.

use std::{borrow::Cow, path::Path, sync::LazyLock};

use regex::Regex;
use saphyr::{MappingOwned, Scalar, ScalarStyle, Tag, Yaml, YamlEmitter, YamlLoader, YamlOwned};

// SnakeYAML 2.6 `Resolver`'s implicit types, as `Resolver.BOOL` (...) print on Java 25.
static BOOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)$")
        .unwrap()
});
static FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+]?[0-9]+)?|[-+]?(?:[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)|[-+]?\.[0-9_]+(?:[eE][-+]?[0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$").unwrap()
});
static INT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[-+]?0b_*[0-1][0-1_]*|[-+]?0_*[0-7][0-7_]*|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x_*[0-9a-fA-F][0-9a-fA-F_]*|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$").unwrap()
});
static NULL: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?:~|null|Null|NULL| )$").unwrap());
static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$").unwrap()
});
/// Strings SnakeYAML's writer quotes although they read back as text (`08`, `0_9`).
static LEADING_ZERO: LazyLock<Regex> = LazyLock::new(|| Regex::new("^0[0-9_]+$").unwrap());

#[derive(Debug, thiserror::Error)]
pub enum YamlError {
    #[error("{0}")]
    Syntax(#[from] saphyr::ScanError),
    /// Bukkit's message for a document that is not a mapping.
    #[error("Top level is not a Map.")]
    NotAMapping,
}

/// A value as SnakeYAML constructs it (`Integer`/`Long`/`BigInteger` are all `Int`).
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'a> {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(Cow<'a, str>),
    /// A mapping or a sequence.
    Collection,
}

impl Value<'_> {
    /// `String.valueOf(object)` for a scalar (`None` for a collection or null).
    pub fn java_string(&self) -> Option<String> {
        Some(match self {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => crate::util::number_text::double_to_string(*f),
            Value::Str(s) => s.to_string(),
            Value::Null | Value::Collection => return None,
        })
    }
}

/// One YAML file: an ordered tree whose scalars keep their source text and style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YamlDocument {
    root: MappingOwned,
}

/// How a library orders keys when it saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOrder {
    /// The file's order, keys added since at the end of their mapping.
    AsLoaded,
    /// Configurate's `MapFactories.sortedNatural()`: every mapping sorted by key.
    Sorted,
}

impl YamlDocument {
    /// Parses one document (an empty file is an empty mapping; a later duplicate key wins, as SnakeYAML).
    pub fn parse(text: &str) -> Result<Self, YamlError> {
        let mut loader: YamlLoader<YamlOwned> = YamlLoader::default();
        loader.early_parse(false);
        loader.allow_duplicate_keys(true);
        saphyr_parser::Parser::new_from_str(text).load(&mut loader, false)?;
        if let Some(e) = loader.error() {
            return Err(e.clone().into());
        }
        match loader.into_documents().into_iter().next() {
            None => Ok(Self::default()),
            Some(YamlOwned::Mapping(root)) => Ok(Self { root }),
            Some(node) if resolve_node(&node) == Value::Null => Ok(Self::default()),
            Some(_) => Err(YamlError::NotAMapping),
        }
    }

    /// The file's text, or `None` when it does not exist.
    pub fn read(path: &Path) -> std::io::Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The value at a dotted path (`proxies.velocity.secret`); `Null` when it is missing.
    pub fn get(&self, path: &str) -> Value<'_> {
        let mut map = &self.root;
        let mut parts = path.split('.').peekable();
        while let Some(part) = parts.next() {
            let Some(node) = find(map, part) else {
                return Value::Null;
            };
            match (parts.peek(), node) {
                (None, node) => return resolve_node(node),
                (Some(_), YamlOwned::Mapping(m)) => map = m,
                (Some(_), _) => return Value::Null,
            }
        }
        Value::Null
    }

    /// The keys of the mapping at a dotted path, in file order (`ConfigurationSection.getKeys(false)`);
    /// empty when it is missing or not a mapping.
    pub fn keys(&self, path: &str) -> Vec<String> {
        let mut map = &self.root;
        for part in path.split('.') {
            match find(map, part) {
                Some(YamlOwned::Mapping(m)) => map = m,
                _ => return Vec::new(),
            }
        }
        map.keys().filter_map(key_text).map(str::to_owned).collect()
    }

    /// Sets the value at a dotted path, creating (or replacing non-mapping) parents, as
    /// `ConfigurationSection.set` does. An existing key keeps its place.
    pub fn set(&mut self, path: &str, value: Value<'_>) {
        let node = match value {
            Value::Bool(b) => plain(b.to_string()),
            Value::Int(i) => plain(i.to_string()),
            Value::Float(f) => plain(crate::util::number_text::double_to_string(f)),
            Value::Str(s) => {
                YamlOwned::Representation(s.into_owned(), ScalarStyle::DoubleQuoted, None)
            }
            Value::Null => plain("null".into()),
            Value::Collection => YamlOwned::Mapping(MappingOwned::new()),
        };
        let (parents, last) = path.rsplit_once('.').unwrap_or(("", path));
        let mut map = &mut self.root;
        for part in parents.split('.').filter(|p| !p.is_empty()) {
            let key = find_key(map, part).unwrap_or_else(|| plain(part.into()));
            let entry = map
                .entry(key)
                .or_insert(YamlOwned::Mapping(MappingOwned::new()));
            if !matches!(entry, YamlOwned::Mapping(_)) {
                *entry = YamlOwned::Mapping(MappingOwned::new());
            }
            let YamlOwned::Mapping(next) = entry else {
                unreachable!()
            };
            map = next;
        }
        // hashlink's `insert` would move an existing key to the end.
        match find_key(map, last).and_then(|key| map.get_mut(&key)) {
            Some(slot) => *slot = node,
            None => {
                map.insert(plain(last.into()), node);
            }
        }
    }

    /// The text a save writes: `header` as `# ` comment lines (`blank_line` for its empty lines), a blank
    /// line, then the document in block style with 2-space indents, as SnakeYAML writes it.
    pub fn to_text(&self, header: &str, blank_line: &str, order: KeyOrder) -> String {
        let mut out = String::new();
        if !header.is_empty() {
            for line in header.lines() {
                out.push_str(if line.is_empty() { blank_line } else { "# " });
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        if self.root.is_empty() {
            out.push_str("{}\n");
            return out;
        }
        let tree = emit_form(&YamlOwned::Mapping(self.root.clone()), order);
        let mut text = String::new();
        let mut emitter = YamlEmitter::new(&mut text);
        emitter.multiline(true);
        // Writing into a `String` cannot fail, and the tree holds no alias or block scalar
        // representation (the only nodes the emitter refuses).
        emitter.dump(&tree).expect("YAML emit into a String");
        out.push_str(text.strip_prefix("---\n").unwrap_or(&text));
        out.push('\n');
        out
    }
}

fn plain(text: String) -> YamlOwned {
    YamlOwned::Representation(text, ScalarStyle::Plain, None)
}

fn key_text(key: &YamlOwned) -> Option<&str> {
    match key {
        YamlOwned::Representation(text, ..) => Some(text),
        _ => None,
    }
}

fn find<'m>(map: &'m MappingOwned, key: &str) -> Option<&'m YamlOwned> {
    map.iter()
        .find(|(k, _)| key_text(k) == Some(key))
        .map(|(_, v)| v)
}

fn find_key(map: &MappingOwned, key: &str) -> Option<YamlOwned> {
    map.keys().find(|k| key_text(k) == Some(key)).cloned()
}

fn resolve_node(node: &YamlOwned) -> Value<'_> {
    match node {
        YamlOwned::Representation(text, style, tag) => resolve(text, *style, tag.as_ref()),
        YamlOwned::Mapping(_) | YamlOwned::Sequence(_) => Value::Collection,
        YamlOwned::Tagged(_, inner) => resolve_node(inner),
        _ => Value::Null,
    }
}

/// SnakeYAML's implicit typing of a scalar, then `SafeConstructor`'s value for that type.
pub fn resolve<'a>(text: &'a str, style: ScalarStyle, tag: Option<&Tag>) -> Value<'a> {
    let is_str_tag = tag.is_some_and(|t| t.is_yaml_core_schema() && t.suffix == "str");
    if style != ScalarStyle::Plain || is_str_tag {
        return Value::Str(Cow::Borrowed(text));
    }
    if text.is_empty() || NULL.is_match(text) {
        Value::Null
    } else if BOOL.is_match(text) {
        Value::Bool(
            matches!(text.as_bytes()[0], b'y' | b'Y' | b't' | b'T')
                || text.eq_ignore_ascii_case("on"),
        )
    } else if INT.is_match(text) {
        construct_int(text).map_or(Value::Str(Cow::Borrowed(text)), Value::Int)
    } else if FLOAT.is_match(text) {
        Value::Float(construct_float(text))
    } else {
        Value::Str(Cow::Borrowed(text))
    }
}

/// The sign, then the rest with `_` removed.
fn sign_and_digits(text: &str) -> (i128, String) {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    match digits.strip_prefix('-') {
        Some(rest) => (-1, rest.to_owned()),
        None => (1, digits.strip_prefix('+').unwrap_or(&digits).to_owned()),
    }
}

/// `ConstructYamlInt`: `0b`, `0x`, leading-`0` octal, `:` base 60, else decimal.
fn construct_int(text: &str) -> Option<i128> {
    let (sign, value) = sign_and_digits(text);
    let magnitude = if let Some(bin) = value.strip_prefix("0b") {
        i128::from_str_radix(bin, 2).ok()?
    } else if let Some(hex) = value.strip_prefix("0x") {
        i128::from_str_radix(hex, 16).ok()?
    } else if value.len() > 1 && value.starts_with('0') {
        i128::from_str_radix(&value[1..], 8).ok()?
    } else if value.contains(':') {
        value.split(':').try_fold(0i128, |acc, d| {
            acc.checked_mul(60)?.checked_add(d.parse().ok()?)
        })?
    } else {
        value.parse().ok()?
    };
    Some(sign * magnitude)
}

/// `ConstructYamlFloat`.
fn construct_float(text: &str) -> f64 {
    let (sign, value) = sign_and_digits(text);
    let sign = sign as f64;
    let lower = value.to_ascii_lowercase();
    if lower == ".inf" {
        sign * f64::INFINITY
    } else if lower == ".nan" {
        f64::NAN
    } else if value.contains(':') {
        let mut parts = value.split(':').rev();
        let seconds: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let rest = parts
            .rev()
            .fold(0.0, |acc, d| acc * 60.0 + d.parse::<f64>().unwrap_or(0.0));
        sign * (rest * 60.0 + seconds)
    } else {
        sign * value.parse::<f64>().unwrap_or(0.0)
    }
}

/// The tree normalized for saphyr's emitter: every scalar as SnakeYAML would write its value.
fn emit_form(node: &YamlOwned, order: KeyOrder) -> Yaml<'static> {
    match node {
        YamlOwned::Mapping(map) => {
            let mut entries: Vec<(Yaml<'static>, Yaml<'static>, &str)> = map
                .iter()
                .map(|(k, v)| {
                    (
                        emit_form(k, order),
                        emit_form(v, order),
                        key_text(k).unwrap_or(""),
                    )
                })
                .collect();
            if order == KeyOrder::Sorted {
                entries.sort_by(|a, b| a.2.cmp(b.2));
            }
            Yaml::Mapping(entries.into_iter().map(|(k, v, _)| (k, v)).collect())
        }
        YamlOwned::Sequence(items) => {
            Yaml::Sequence(items.iter().map(|i| emit_form(i, order)).collect())
        }
        YamlOwned::Tagged(_, inner) => emit_form(inner, order),
        _ => match resolve_node(node) {
            Value::Null => Yaml::Representation(Cow::Borrowed("null"), ScalarStyle::Plain, None),
            Value::Bool(b) => Yaml::Value(Scalar::Boolean(b)),
            Value::Int(i) => {
                Yaml::Representation(Cow::Owned(i.to_string()), ScalarStyle::Plain, None)
            }
            Value::Float(f) => Yaml::Representation(
                Cow::Owned(
                    crate::util::number_text::double_to_string(f)
                        .replace("Infinity", ".inf")
                        .replace("NaN", ".NaN"),
                ),
                ScalarStyle::Plain,
                None,
            ),
            Value::Str(s) => emit_string(&s),
            Value::Collection => unreachable!("collections are matched above"),
        },
    }
}

/// SnakeYAML's choice of style for a string: plain when it reads back as the same string, else single
/// quotes, double quotes when it has control characters, a literal block when it has line breaks.
fn emit_string(s: &str) -> Yaml<'static> {
    let has_control = s.chars().any(|c| c.is_control() && c != '\n');
    if has_control || s.contains('\n') {
        // saphyr writes a literal block for line breaks (multiline on) and escapes control characters
        // in double quotes.
        return Yaml::Value(Scalar::String(Cow::Owned(s.to_owned())));
    }
    let typed = !matches!(resolve(s, ScalarStyle::Plain, None), Value::Str(_));
    let quote = s.is_empty()
        || typed
        || s == "<<"
        || TIMESTAMP.is_match(s)
        || LEADING_ZERO.is_match(s)
        || s.starts_with([
            ' ', '#', ',', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
        ])
        || s.ends_with([' ', ':'])
        || s.starts_with("- ")
        || s.starts_with("? ")
        || s.starts_with(": ")
        || s.contains(": ")
        || s.contains(" #");
    if quote {
        Yaml::Representation(
            Cow::Owned(s.replace('\'', "''")),
            ScalarStyle::SingleQuoted,
            None,
        )
    } else {
        Yaml::Representation(Cow::Owned(s.to_owned()), ScalarStyle::Plain, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SnakeYAML 2.6 on Java 25, one case per line: `input => class => value` for
    /// `new Yaml(new SafeConstructor(new LoaderOptions())).load("k: " + input)` (the value's class and
    /// `String.valueOf`).
    const SNAKEYAML_LOADS: &str = r#"yes => Boolean => true
No => Boolean => false
on => Boolean => true
OFF => Boolean => false
y => String => y
n => String => n
true => Boolean => true
True => Boolean => true
tRUE => String => tRUE
~ => null => null
null => null => null
Null => null => null
NULL => null => null
 => null => null
0 => Integer => 0
-0 => Integer => 0
+12 => Integer => 12
012 => Integer => 10
0o12 => String => 0o12
0b101 => Integer => 5
-0b11 => Integer => -3
0x1F => Integer => 31
0x_1f => Integer => 31
1_000 => Integer => 1000
1:30 => Integer => 90
-190:20:30 => Integer => -685230
4294967296 => Long => 4294967296
-2147483649 => Long => -2147483649
99999999999999999999 => BigInteger => 99999999999999999999
1.5 => Double => 1.5
1. => Double => 1.0
.5 => Double => 0.5
-.5 => Double => -0.5
1e5 => Double => 100000.0
1E+5 => Double => 100000.0
1.5e3 => Double => 1500.0
.inf => Double => Infinity
-.Inf => Double => -Infinity
+.INF => Double => Infinity
.NaN => Double => NaN
1_0.5 => Double => 10.5
1:30.5 => Double => 90.5
abc => String => abc
08 => String => 08
0x => String => 0x
'1' => String => 1
"yes" => String => yes
!!str 12 => String => 12
1.0e => String => 1.0e
0.1 => Double => 0.1"#;

    fn cases(table: &str) -> impl Iterator<Item = Vec<&str>> {
        table.lines().map(|line| line.split(" => ").collect())
    }

    #[test]
    fn scalars_resolve_as_snakeyaml() {
        for case in cases(SNAKEYAML_LOADS) {
            let (input, class, text) = (case[0], case[1], case[2]);
            let doc = YamlDocument::parse(&format!("k: {input}\n")).unwrap();
            let value = doc.get("k");
            let got_class = match &value {
                Value::Null => "null",
                Value::Bool(_) => "Boolean",
                Value::Int(i) if i32::try_from(*i).is_ok() => "Integer",
                Value::Int(i) if i64::try_from(*i).is_ok() => "Long",
                Value::Int(_) => "BigInteger",
                Value::Float(_) => "Double",
                Value::Str(_) => "String",
                Value::Collection => "Map",
            };
            let got_text = value.java_string().unwrap_or_else(|| "null".into());
            assert_eq!((got_class, got_text.as_str()), (class, text), "{input:?}");
        }
    }

    /// SnakeYAML 2.6 `dump` (indent 2, block style) of `{k: input}`, one case per line:
    /// `input => written` (`<empty>` is the empty string).
    const SNAKEYAML_DUMPS: &str = r#"<empty> => ''
plain => plain
yes => 'yes'
Yes => 'Yes'
y => y
123 => '123'
1.5 => '1.5'
null => 'null'
~ => '~'
a: b => 'a: b'
a:b => a:b
it's => it's
 lead => ' lead'
trail  => 'trail '
#x => '#x'
x #y => 'x #y'
-x => -x
- x => '- x'
@x => '@x'
%x => '%x'
!x => '!x'
&x => '&x'
*x => '*x'
[x => '[x'
{x => '{x'
x] => x]
x,y => x,y
2026-10-30 => '2026-10-30'
08 => '08'
0_8 => '0_8'
-08 => -08
0x => 0x
:x => :x
x: => 'x:'
?x => ?x
|x => '|x'
>x => '>x'
'q => '''q'
"q => '"q'
`x => '`x'
= => =
<< => '<<'
0. => '0.'
1e5x => 1e5x"#;

    #[test]
    fn strings_are_written_as_snakeyaml() {
        let escaped = [("a\nb", "|-\n  a\n  b"), ("tab\there", "\"tab\\there\"")];
        let table =
            cases(SNAKEYAML_DUMPS).map(|c| (if c[0] == "<empty>" { "" } else { c[0] }, c[1]));
        for (input, written) in table.chain(escaped) {
            let mut doc = YamlDocument::default();
            doc.set("k", Value::Str(input.into()));
            let text = doc.to_text("", "", KeyOrder::AsLoaded);
            assert_eq!(text, format!("k: {written}\n"), "{input:?}");
            let read = YamlDocument::parse(&text).unwrap();
            assert_eq!(read.get("k"), Value::Str(input.into()));
        }
    }

    #[test]
    fn set_creates_parents_and_keeps_places() {
        let mut doc = YamlDocument::parse("b: 1\na:\n  y: 'x'\n  x: [1, 2]\nc: scalar\n").unwrap();
        doc.set("a.y", Value::Bool(false));
        doc.set("c.d", Value::Float(19.0));
        doc.set("e.f.g", Value::Int(5));
        assert_eq!(
            doc.to_text("Head\n\nTail", "#", KeyOrder::AsLoaded),
            "# Head\n#\n# Tail\n\nb: 1\na:\n  y: false\n  x:\n    - 1\n    - 2\nc:\n  d: 19.0\ne:\n  f:\n    g: 5\n"
        );
        assert_eq!(
            doc.to_text("", "", KeyOrder::Sorted),
            "a:\n  x:\n    - 1\n    - 2\n  y: false\nb: 1\nc:\n  d: 19.0\ne:\n  f:\n    g: 5\n"
        );
        assert!(matches!(
            YamlDocument::parse("- a\n"),
            Err(YamlError::NotAMapping)
        ));
        assert!(YamlDocument::parse("a: [\n").is_err());
        assert_eq!(
            YamlDocument::parse("")
                .unwrap()
                .to_text("", "", KeyOrder::Sorted),
            "{}\n"
        );
    }
}
