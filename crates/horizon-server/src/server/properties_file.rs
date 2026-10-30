//! The `.properties` files the server keeps (`server.properties`, `eula.txt`), as `Settings.loadFromFile`
//! and `Properties.store` handle them.
//!
//! Reading uses the `java-properties` crate, which parses the format as the JDK's `Properties.load`
//! does (line continuations, escapes, separators, comments; compared case by case against Java 25,
//! see the tests). Two things are done around it: the text is decoded first (UTF-8, else ISO-8859-1 for
//! the whole file, as `Settings.loadFromFile` retries), and a final line break is added, because the
//! crate drops a last line that ends in a bare `\` where Java keeps it.
//!
//! Writing is done here: the crate escapes every space and writes control characters as short `\u`
//! escapes, while the server's file is `Properties.store(Writer)`: keys sorted, only a value's leading
//! space escaped, everything else (control characters, non-ASCII) written as it is.

use std::{collections::HashMap, path::Path};

/// A `.properties` file's entries (Java's `Properties`: the last of repeated keys wins; no order until
/// it is stored, sorted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropertiesFile {
    entries: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PropertiesError {
    #[error("couldn't read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    /// A malformed `\uxxxx` escape: `Properties.load` throws `IllegalArgumentException` and the
    /// server does not start.
    #[error("couldn't parse {path}: {source}")]
    Format {
        path: String,
        source: java_properties::PropertiesError,
    },
}

impl PropertiesFile {
    /// `Properties.load` over a file's bytes, decoded as `Settings.loadFromFile` does: strict UTF-8,
    /// or ISO-8859-1 when the file is not valid UTF-8.
    pub fn parse(bytes: &[u8]) -> Result<Self, java_properties::PropertiesError> {
        let mut text = match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(_) => bytes.iter().copied().map(char::from).collect(),
        };
        text.push('\n');
        let mut entries = HashMap::new();
        java_properties::PropertiesIter::new_with_encoding(text.as_bytes(), encoding_rs::UTF_8)
            .read_into(|key, value| {
                entries.insert(key, value);
            })?;
        Ok(Self { entries })
    }

    /// Reads `path`; a missing file is an empty set (`Settings.loadFromFile`).
    pub fn load(path: &Path) -> Result<Self, PropertiesError> {
        let shown = || path.display().to_string();
        match std::fs::read(path) {
            Ok(bytes) => Self::parse(&bytes).map_err(|source| PropertiesError::Format {
                path: shown(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(PropertiesError::Io {
                path: shown(),
                source,
            }),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `Properties.store(Writer, comment)`: `#comment`, `#date`, then `key=value` lines sorted by key
    /// (by UTF-16 code units, as `String.compareTo`), with the platform's line separator.
    pub fn to_text(&self, comment: &str, date: &str) -> String {
        debug_assert!(
            !comment.contains(['\n', '\r']),
            "multi-line comments are not needed"
        );
        let newline = if cfg!(windows) { "\r\n" } else { "\n" };
        let mut out = format!("#{comment}{newline}#{date}{newline}");
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        for key in keys {
            escape_into(&mut out, key, true);
            out.push('=');
            escape_into(&mut out, &self.entries[key], false);
            out.push_str(newline);
        }
        out
    }

    /// Writes the file as UTF-8 (`Files.newBufferedWriter(path, UTF_8)` + `store`), dated now.
    pub fn store(&self, path: &Path, comment: &str) -> std::io::Result<()> {
        std::fs::write(path, self.to_text(comment, &java_date_now()))
    }
}

/// `Properties.saveConvert` with `escUnicode` off.
fn escape_into(out: &mut String, text: &str, escape_space: bool) {
    for (i, c) in text.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' if i == 0 || escape_space => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
}

/// `new Date().toString()` in the local time zone: `Mon Oct 26 09:33:20 EET 2026`.
pub fn java_date_now() -> String {
    java_date(&jiff::Zoned::now())
}

/// `Date.toString` for a moment in a zone. Java shows the zone's short name; where the time zone
/// database has none (only a numeric one such as `-03`), Java shows `GMT-03:00`.
pub fn java_date(time: &jiff::Zoned) -> String {
    let abbreviation = time
        .time_zone()
        .to_offset_info(time.timestamp())
        .abbreviation()
        .to_owned();
    let zone = if abbreviation.starts_with(['+', '-']) {
        let seconds = time.offset().seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let minutes = seconds.unsigned_abs() / 60;
        format!("GMT{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    } else {
        abbreviation
    };
    format!(
        "{} {zone} {}",
        time.strftime("%a %b %d %H:%M:%S"),
        time.strftime("%Y")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each input with the entries `Properties.load` gives on Java 25 (sorted).
    #[test]
    fn reads_like_the_jdk() {
        let cases: &[(&str, &[(&str, &str)])] = &[
            ("a=b", &[("a", "b")]),
            (
                "  key  =  value with spaces  ",
                &[("key", "value with spaces  ")],
            ),
            (
                "key:value\nkey2 value2\nkey3\n",
                &[("key", "value"), ("key2", "value2"), ("key3", "")],
            ),
            ("multi=line1\\\n    line2\n", &[("multi", "line1line2")]),
            ("# comment\n! bang\nx=1", &[("x", "1")]),
            (
                "esc=\\t\\n\\r\\f\\u0041\\=\\:\\ x\\q",
                &[("esc", "\t\n\r\u{c}A=: xq")],
            ),
            ("dup=1\ndup=2", &[("dup", "2")]),
            ("a\\ b=c", &[("a b", "c")]),
            ("trail=\\", &[("trail", "")]),
            (
                "crlf=1\r\nb=2\rc=3",
                &[("b", "2"), ("c", "3"), ("crlf", "1")],
            ),
            ("unicode=é😀", &[("unicode", "é😀")]),
            ("  \n\t\u{c}=empty key", &[("", "empty key")]),
            ("odd=\\\\\\\n next", &[("odd", "\\next")]),
            (
                "comment-continued=# not comment",
                &[("comment-continued", "# not comment")],
            ),
            ("  # indented comment\nk=v", &[("k", "v")]),
            (
                "k=v\\\n# continuation not comment",
                &[("k", "v# continuation not comment")],
            ),
            ("key\\=with\\:seps=v", &[("key=with:seps", "v")]),
            ("sep  :  v", &[("sep", "v")]),
            ("sep2 = = v", &[("sep2", "= v")]),
            ("blank=\n\n\nafter=1", &[("after", "1"), ("blank", "")]),
        ];
        for &(input, expected) in cases {
            let file = PropertiesFile::parse(input.as_bytes()).unwrap();
            let mut entries: Vec<(&str, &str)> = file
                .entries
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            entries.sort();
            assert_eq!(entries, expected, "{input:?}");
        }
        // Java throws IllegalArgumentException("Malformed \uxxxx encoding.").
        assert!(PropertiesFile::parse(b"bad=\\u12").is_err());
    }

    #[test]
    fn falls_back_to_latin1() {
        let file = PropertiesFile::parse(b"motd=caf\xe9").unwrap();
        assert_eq!(file.get("motd"), Some("café"));
    }

    /// `Properties.store(StringWriter, "Minecraft server properties")` on Java 25 with
    /// `-Djava.properties.date="Thu Jan 01 00:00:00 UTC 1970"`.
    #[test]
    fn stores_like_the_jdk() {
        let mut file = PropertiesFile::default();
        for (key, value) in [
            ("motd", "A Minecraft Server"),
            ("leading", "  two spaces"),
            ("key with space", "v"),
            ("seps", "a=b:c#d!e"),
            ("ctrl", "\u{1b}[0m"),
            ("uni", "é😀ü"),
            ("tab", "\tx"),
            ("empty", ""),
            ("back", "C:\\path"),
            ("level-type", "minecraft:normal"),
            ("#hash", "x"),
            ("multi", "l1\nl2\r"),
            ("!bang", "y"),
            ("Zed", "upper"),
            ("del", "\u{7f}"),
        ] {
            file.insert(key, value);
        }
        let expected = "#Minecraft server properties\n#Thu Jan 01 00:00:00 UTC 1970\n\\!bang=y\n\\#hash=x\n\
            Zed=upper\nback=C\\:\\\\path\nctrl=\u{1b}[0m\ndel=\u{7f}\nempty=\nkey\\ with\\ space=v\n\
            leading=\\  two spaces\nlevel-type=minecraft\\:normal\nmotd=A Minecraft Server\nmulti=l1\\nl2\\r\n\
            seps=a\\=b\\:c\\#d\\!e\ntab=\\tx\nuni=é😀ü\n";
        let text = file.to_text(
            "Minecraft server properties",
            "Thu Jan 01 00:00:00 UTC 1970",
        );
        if cfg!(windows) {
            assert_eq!(text, expected.replace('\n', "\r\n"));
        } else {
            assert_eq!(text, expected);
        }
        // And it reads back to the same entries.
        assert_eq!(PropertiesFile::parse(text.as_bytes()).unwrap(), file);
    }

    /// `new Date(1793000000000L).toString()` on Java 25 in these zones.
    #[test]
    fn dates_like_java() {
        let moment = jiff::Timestamp::from_millisecond(1_793_000_000_000).unwrap();
        for (zone, expected) in [
            ("UTC", "Mon Oct 26 07:33:20 UTC 2026"),
            ("Europe/Kyiv", "Mon Oct 26 09:33:20 EET 2026"),
            ("Europe/Moscow", "Mon Oct 26 10:33:20 MSK 2026"),
            ("America/Sao_Paulo", "Mon Oct 26 04:33:20 GMT-03:00 2026"),
            ("Asia/Kolkata", "Mon Oct 26 13:03:20 IST 2026"),
            ("Australia/Lord_Howe", "Mon Oct 26 18:33:20 GMT+11:00 2026"),
        ] {
            let Ok(tz) = jiff::tz::TimeZone::get(zone) else {
                // No time zone database on this machine.
                continue;
            };
            assert_eq!(java_date(&moment.to_zoned(tz)), expected, "{zone}");
        }
    }
}
