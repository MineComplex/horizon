//! `StoredUserList`: one JSON file of entries, kept in memory in `ConcurrentHashMap` order (Paper)
//! and saved whole, as Gson pretty-prints it. Owned by the tick thread: no locks. Leaf saves on
//! another thread (Akarin); here the file is written before `add`/`remove` return.

use std::path::{Path, PathBuf};

use jiff::tz::TimeZone;
use serde_json::{Map, Value};

use super::map::ConcurrentMap;

/// One field of a serialized entry (`JsonObject.addProperty`). A `Null` is not written (Gson's
/// `serializeNulls` is off).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Field {
    Str(String),
    Int(i32),
    Bool(bool),
    Null,
}

/// Why a file could not be loaded.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoadFailure {
    /// `JsonParseException` or `NullPointerException` (Spigot): the file is moved to `<name>.backup`
    /// and a new one is made.
    #[error("{0}")]
    Unreadable(String),
    /// Any other exception (`getAsString` on a `null`, `getAsInt` on text): the entries read before
    /// it stay, the file is left as it is.
    #[error("{0}")]
    Failed(String),
}

/// What a list holds: `StoredUserEntry` and the key `getKeyForUser` makes.
pub trait Entry: Sized {
    /// What the list is asked about: a `NameAndId`, or an address.
    type Lookup: ?Sized;

    fn lookup_key(lookup: &Self::Lookup) -> String;

    /// The key of this entry (its user). Entries without a user are never stored.
    fn key(&self) -> String;

    /// `createEntry` at `now`; `Ok(None)` is an entry whose `getUser()` is `null` (not kept).
    fn read(object: &Map<String, Value>, now: i64) -> Result<Option<Self>, LoadFailure>;

    /// `serialize`, in the order Java adds the properties.
    fn fields(&self, zone: &TimeZone) -> Vec<(&'static str, Field)>;

    /// `hasExpired`.
    fn has_expired(&self, _now: i64) -> bool {
        false
    }

    /// `equals`, which only ban entries override (the others are equal to themselves alone).
    fn same_as(&self, _other: &Self) -> bool {
        false
    }
}

pub struct UserList<E: Entry> {
    file: PathBuf,
    entries: ConcurrentMap<E>,
    zone: TimeZone,
}

impl<E: Entry> UserList<E> {
    /// `zone` is the JVM's default time zone, which the ban dates are written in.
    pub fn new(file: impl Into<PathBuf>, zone: TimeZone) -> Self {
        Self {
            file: file.into(),
            entries: ConcurrentMap::default(),
            zone,
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `getEntries`, in the map's order.
    pub fn entries(&self) -> impl Iterator<Item = &E> {
        self.entries.iter().map(|(_, entry)| entry)
    }

    /// `add`: false when an equal entry is there; saves otherwise.
    pub fn add(&mut self, entry: E, now: i64) -> bool {
        let key = entry.key();
        if self.entries.get(&key).is_some_and(|old| entry.same_as(old)) {
            return false;
        }
        self.entries.put(key, entry);
        if let Err(e) = self.save(now) {
            tracing::warn!("Could not save the list after adding a user. {e}");
        }
        true
    }

    /// `get`: an expired entry is removed (and not saved) and not returned.
    pub fn get(&mut self, lookup: &E::Lookup, now: i64) -> Option<&E> {
        let key = E::lookup_key(lookup);
        if self.entries.get(&key).is_some_and(|e| e.has_expired(now)) {
            self.entries.remove(&key);
        }
        self.entries.get(&key)
    }

    /// `contains` (`removeExpired` first, for every entry).
    pub fn contains(&mut self, lookup: &E::Lookup, now: i64) -> bool {
        self.remove_expired(now);
        self.entries.get(&E::lookup_key(lookup)).is_some()
    }

    /// `remove`: true when it was there; saves then.
    pub fn remove(&mut self, lookup: &E::Lookup, now: i64) -> bool {
        if self.entries.remove(&E::lookup_key(lookup)).is_none() {
            return false;
        }
        if let Err(e) = self.save(now) {
            tracing::warn!("Could not save the list after removing a user. {e}");
        }
        true
    }

    /// `clear`.
    pub fn clear(&mut self, now: i64) {
        self.entries.clear();
        if let Err(e) = self.save(now) {
            tracing::warn!("Could not save the list after removing a user. {e}");
        }
    }

    fn remove_expired(&mut self, now: i64) {
        self.entries.remove_if(|e| e.has_expired(now));
    }

    /// The file's text: `GSON.toJson(array)` with pretty printing, after dropping expired entries.
    pub fn render(&mut self, now: i64) -> String {
        self.remove_expired(now);
        let rows: Vec<_> = self.entries().map(|e| e.fields(&self.zone)).collect();
        pretty_array(&rows)
    }

    /// `save`.
    pub fn save(&mut self, now: i64) -> std::io::Result<()> {
        let text = self.render(now);
        std::fs::write(&self.file, text)
    }

    /// `load`: nothing when the file does not exist.
    pub fn load(&mut self, now: i64) -> Result<(), LoadFailure> {
        let Ok(bytes) = std::fs::read(&self.file) else {
            return Ok(());
        };
        match self.load_text(&String::from_utf8_lossy(&bytes), now) {
            Err(LoadFailure::Unreadable(why)) => {
                let mut backup = self.file.clone().into_os_string();
                backup.push(".backup");
                tracing::warn!(
                    "Unable to read file {}, backing it up to {} and creating new copy: {why}",
                    self.file.display(),
                    Path::new(&backup).display()
                );
                let _ = std::fs::rename(&self.file, &backup);
                let _ = std::fs::remove_file(&self.file);
                Ok(())
            }
            other => other,
        }
    }

    /// `load`'s body on the file's text: the entries are replaced, and a failure leaves those read
    /// before it.
    pub fn load_text(&mut self, text: &str, now: i64) -> Result<(), LoadFailure> {
        self.entries.clear();
        self.read_all(text, now)
    }

    fn read_all(&mut self, text: &str, now: i64) -> Result<(), LoadFailure> {
        // `GSON.fromJson(reader, JsonArray.class)`: an empty file is `null` (nothing to do); the text
        // `null` is a `JsonNull`, not an array.
        if text.trim().is_empty() {
            return Ok(());
        }
        let value: Value = serde_json::from_str(text).map_err(|e| {
            LoadFailure::Unreadable(format!("com.google.gson.JsonSyntaxException: {e}"))
        })?;
        let items = match value {
            Value::Array(items) => items,
            other => {
                return Err(LoadFailure::Unreadable(format!(
                    "com.google.gson.JsonSyntaxException: Expected a com.google.gson.JsonArray but was {}",
                    kind(&other)
                )));
            }
        };
        for item in &items {
            let Value::Object(object) = item else {
                return Err(LoadFailure::Unreadable(format!(
                    "com.google.gson.JsonSyntaxException: Expected entry to be a JsonObject, was {}",
                    kind(item)
                )));
            };
            if let Some(entry) = E::read(object, now)? {
                self.entries.put(entry.key(), entry);
            }
        }
        Ok(())
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) | Value::Number(_) | Value::String(_) => "a primitive",
        Value::Array(_) => "a JsonArray",
        Value::Object(_) => "a JsonObject",
    }
}

/// Gson's pretty printing of an array of flat objects: two spaces, `"key": value`, no trailing
/// newline, an empty array as `[]`.
fn pretty_array(rows: &[Vec<(&'static str, Field)>]) -> String {
    if rows.is_empty() {
        return "[]".into();
    }
    let objects: Vec<String> = rows
        .iter()
        .map(|row| {
            let fields: Vec<String> = row
                .iter()
                .filter(|(_, f)| *f != Field::Null)
                .map(|(key, field)| {
                    let value = match field {
                        Field::Str(s) => gson_string(s),
                        Field::Int(i) => i.to_string(),
                        Field::Bool(b) => b.to_string(),
                        Field::Null => unreachable!("filtered"),
                    };
                    format!("    {}: {value}", gson_string(key))
                })
                .collect();
            if fields.is_empty() {
                "  {}".to_owned()
            } else {
                format!("  {{\n{}\n  }}", fields.join(",\n"))
            }
        })
        .collect();
    format!("[\n{}\n]", objects.join(",\n"))
}

/// `JsonWriter.string` with `htmlSafe` (`GSON.newJsonWriter` of a default `Gson`).
fn gson_string(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '=' | '\'' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `JsonElement.getAsString` of a property that is there: a primitive's text; a `null`, array or
/// object throws (`UnsupportedOperationException`, `IllegalStateException`).
pub fn as_string(object: &Map<String, Value>, key: &str) -> Result<Option<String>, LoadFailure> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(other) => Err(LoadFailure::Failed(format!(
            "java.lang.UnsupportedOperationException: cannot read {key} from {}",
            kind(other)
        ))),
    }
}

/// `JsonElement.getAsInt`: a number's `intValue()` (the low 32 bits of the truncated value), a
/// string's `Integer.parseInt`.
pub fn as_int(object: &Map<String, Value>, key: &str) -> Result<Option<i32>, LoadFailure> {
    let failed =
        |what: String| LoadFailure::Failed(format!("java.lang.NumberFormatException: {what}"));
    match object.get(key) {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(Some(match n.as_i64() {
            Some(i) => i as i32,
            None => match n.as_u64() {
                Some(u) => u as i32,
                None => n.as_f64().map_or(0, |f| f as i64 as i32),
            },
        })),
        Some(Value::String(s)) => s
            .parse::<i32>()
            .map(Some)
            .map_err(|_| failed(format!("For input string: \"{s}\""))),
        Some(Value::Bool(b)) => Err(failed(format!("For input string: \"{b}\""))),
        Some(other) => Err(LoadFailure::Failed(format!(
            "java.lang.UnsupportedOperationException: cannot read {key} from {}",
            kind(other)
        ))),
    }
}

/// `JsonElement.getAsBoolean`: a boolean, else `Boolean.parseBoolean` of the text.
pub fn as_bool(object: &Map<String, Value>, key: &str) -> Result<Option<bool>, LoadFailure> {
    match object.get(key) {
        Some(Value::Bool(b)) => Ok(Some(*b)),
        _ => Ok(as_string(object, key)?.map(|s| s.eq_ignore_ascii_case("true"))),
    }
}
