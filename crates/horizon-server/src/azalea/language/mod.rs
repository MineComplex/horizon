//! Translation strings (`en_us.json`). Adapted from azalea-language 0.16.0
//! (https://github.com/azalea-rs/azalea, MIT).
//!
//! As in vanilla (`Language.loadDefault` + `Language.inject`), the table is loaded once at startup and
//! installed for the whole process; nothing here reads files on its own. Mojang's `en_us.json` is never
//! committed: startup loads it from the server jar's `assets/minecraft/lang/en_us.json`.

use std::{
    collections::HashMap,
    path::Path,
    sync::{LazyLock, OnceLock},
};

/// A translation table: key -> template (`"Hello, %s"`).
///
/// A `HashMap` like vanilla's: the table is built once, looked up by `&str` without allocating, and its
/// order never matters.
#[derive(Clone, Debug, Default)]
pub struct Language {
    entries: HashMap<Box<str>, Box<str>>,
}

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    #[error("couldn't read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("couldn't parse {path}: {source}")]
    Json {
        path: String,
        source: serde_json::Error,
    },
}

impl Language {
    /// A table built from `(key, template)` pairs, taken as they are (no format rewriting).
    pub fn from_entries<K: Into<Box<str>>, V: Into<Box<str>>>(
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    /// Parses a lang file: one JSON object of key -> string, as `Language.loadFromJson` reads it.
    /// Numbers and booleans become their text (`GsonHelper.convertToString`); `%d`/`%.2f`-style
    /// formats become `%s` (`UNSUPPORTED_FORMAT_PATTERN`). A later duplicate key wins.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        use serde::de::Error;

        let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json)?;
        let mut entries = HashMap::with_capacity(object.len());
        for (key, value) in object {
            let text = match value {
                serde_json::Value::String(s) => s,
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                other => {
                    let kind = match other {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Array(_) => "an array",
                        _ => "an object",
                    };
                    return Err(serde_json::Error::custom(format!(
                        "expected {key} to be a string, was {kind}"
                    )));
                }
            };
            entries.insert(
                key.into_boxed_str(),
                rewrite_unsupported_formats(&text).into_boxed_str(),
            );
        }
        Ok(Self { entries })
    }

    /// Reads and parses a lang file.
    pub fn load(path: &Path) -> Result<Self, LanguageError> {
        let shown = || path.display().to_string();
        let json = std::fs::read_to_string(path).map_err(|source| LanguageError::Io {
            path: shown(),
            source,
        })?;
        Self::from_json(&json).map_err(|source| LanguageError::Json {
            path: shown(),
            source,
        })
    }

    /// The template for `key`, if the table has it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|v| &**v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Replaces every match of vanilla's `%(\d+\$)?[\d.]*[df]` with `%$1s`.
fn rewrite_unsupported_formats(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some((index, end)) = unsupported_format_at(bytes, i + 1)
        {
            out.push_str(&text[copied..i]);
            out.push('%');
            out.push_str(index);
            out.push('s');
            copied = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Matches `(\d+\$)?[\d.]*[df]` at `start`: the `\d+\$` group's text and the end of the match.
fn unsupported_format_at(bytes: &[u8], start: usize) -> Option<(&str, usize)> {
    let digits = bytes[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let rest_matches = |from: usize| {
        let skipped = bytes[from..]
            .iter()
            .take_while(|&&b| b.is_ascii_digit() || b == b'.')
            .count();
        matches!(bytes.get(from + skipped), Some(b'd' | b'f')).then_some(from + skipped + 1)
    };
    if digits > 0 && bytes.get(start + digits) == Some(&b'$') {
        let group_end = start + digits + 1;
        if let Some(end) = rest_matches(group_end) {
            // ASCII digits and '$' only, so this is a char boundary
            let group = std::str::from_utf8(&bytes[start..group_end]).ok()?;
            return Some((group, end));
        }
    }
    rest_matches(start).map(|end| ("", end))
}

static INSTALLED: OnceLock<Language> = OnceLock::new();
static EMPTY: LazyLock<Language> = LazyLock::new(Language::default);

/// Installs the process-wide table (`Language.inject`). Startup calls it once; a second call returns
/// the table it was given.
pub fn install(language: Language) -> Result<(), Language> {
    INSTALLED.set(language)
}

/// The installed table, if startup installed one.
pub fn installed() -> Option<&'static Language> {
    INSTALLED.get()
}

/// The installed table, or an empty one (every key then renders as itself or its fallback).
pub(crate) fn installed_or_empty() -> &'static Language {
    installed().unwrap_or(&EMPTY)
}

/// The installed template for `key`; `None` when it is missing or nothing is installed.
pub fn get(key: &str) -> Option<&'static str> {
    installed()?.get(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_json_reads_strings_and_primitives() {
        let lang =
            Language::from_json(r#"{"a": "A %s", "n": 1.5, "b": true, "a": "later"}"#).unwrap();
        assert_eq!(lang.get("a"), Some("later"));
        assert_eq!(lang.get("n"), Some("1.5"));
        assert_eq!(lang.get("b"), Some("true"));
        assert_eq!(lang.get("missing"), None);
        assert_eq!(lang.len(), 3);
    }

    #[test]
    fn from_json_rejects_non_primitives() {
        assert!(Language::from_json(r#"{"a": {"b": "c"}}"#).is_err());
        assert!(Language::from_json(r#"{"a": null}"#).is_err());
        assert!(Language::from_json(r#"["a"]"#).is_err());
    }

    #[test]
    fn unsupported_formats_become_s() {
        let cases = [
            ("%d", "%s"),
            ("%.2f items", "%s items"),
            ("%1$d and %2$.1f", "%1$s and %2$s"),
            ("%12f", "%s"),
            ("%s %1$s %%", "%s %1$s %%"),
            ("%%d", "%%s"),
            ("%1$x %", "%1$x %"),
            ("é %d é", "é %s é"),
        ];
        for (input, expected) in cases {
            assert_eq!(rewrite_unsupported_formats(input), expected, "{input}");
        }
    }

    #[test]
    fn load_reports_missing_file() {
        let err = Language::load(Path::new("/nonexistent/horizon/en_us.json")).unwrap_err();
        assert!(matches!(err, LanguageError::Io { .. }));
    }
}
