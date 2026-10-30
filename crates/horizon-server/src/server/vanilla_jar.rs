//! The vanilla 26.3 server jar the admin provides: the game's data and `en_us.json` are read from it,
//! never committed. Leaf's Paperclip downloads Mojang's jar to `cache/mojang_26.3.jar` (the default of
//! `--vanilla-jar`); that file is Mojang's bundler, whose game jar is
//! `META-INF/versions/26.3/server-26.3.jar` inside it, listed in `META-INF/versions.list`. Both the
//! bundler and the game jar itself (as the bundler extracts it to `versions/26.3/server-26.3.jar`) are
//! accepted. Read with the `zip` crate (the version the rest of the workspace uses).
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::{Path, PathBuf},
};

use zip::{ZipArchive, result::ZipError};

use crate::azalea::language::Language;

/// The version this server is: the jar's `version.json` `id` must be this.
pub const VERSION_ID: &str = "26.3";
const LANG_PATH: &str = "assets/minecraft/lang/en_us.json";
const DEPRECATED_PATH: &str = "assets/minecraft/lang/deprecated.json";

#[derive(Debug, thiserror::Error)]
pub enum VanillaJarError {
    #[error("couldn't open the vanilla server jar {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },
    #[error("{path} is not a readable jar: {source}")]
    Zip { path: String, source: ZipError },
    #[error("{path}: {entry}: {message}")]
    Entry {
        path: String,
        entry: String,
        message: String,
    },
    #[error("{path} is Minecraft {found}, but this server is Minecraft {VERSION_ID}")]
    WrongVersion { path: String, found: String },
}

enum Archive {
    File(ZipArchive<BufReader<File>>),
    /// The game jar read out of Mojang's bundler.
    Nested(ZipArchive<Cursor<Vec<u8>>>),
}

/// An open vanilla 26.3 server jar.
pub struct VanillaJar {
    path: PathBuf,
    archive: Archive,
}

impl std::fmt::Debug for VanillaJar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VanillaJar")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>, ZipError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::with_capacity(entry.size().try_into().unwrap_or(0));
    entry.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

impl VanillaJar {
    /// Opens the jar (or the bundler around it) and checks that it is Minecraft 26.3.
    pub fn open(path: &Path) -> Result<Self, VanillaJarError> {
        let shown = path.display().to_string();
        let zip_err = |source| VanillaJarError::Zip {
            path: shown.clone(),
            source,
        };
        let file = File::open(path).map_err(|source| VanillaJarError::Open {
            path: shown.clone(),
            source,
        })?;
        let mut outer = ZipArchive::new(BufReader::new(file)).map_err(zip_err)?;
        let archive = match read_entry(&mut outer, "META-INF/versions.list").map_err(zip_err)? {
            None => Archive::File(outer),
            Some(list) => {
                // Lines of `sha256 \t id \t path` (the path relative to META-INF/versions/).
                let list = String::from_utf8_lossy(&list);
                let inner = list
                    .lines()
                    .filter_map(|line| line.split('\t').nth(2))
                    .next()
                    .map(|p| format!("META-INF/versions/{p}"))
                    .ok_or_else(|| VanillaJarError::Entry {
                        path: shown.clone(),
                        entry: "META-INF/versions.list".into(),
                        message: "lists no server jar".into(),
                    })?;
                let bytes = read_entry(&mut outer, &inner)
                    .map_err(zip_err)?
                    .ok_or_else(|| VanillaJarError::Entry {
                        path: shown.clone(),
                        entry: inner.clone(),
                        message: "missing".into(),
                    })?;
                Archive::Nested(ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?)
            }
        };
        let mut jar = Self {
            path: path.to_owned(),
            archive,
        };
        let version = jar
            .read("version.json")?
            .ok_or_else(|| jar.entry_error("version.json", "missing".into()))?;
        let id = serde_json::from_slice::<serde_json::Value>(&version)
            .ok()
            .and_then(|v| v.get("id")?.as_str().map(str::to_owned))
            .ok_or_else(|| jar.entry_error("version.json", "has no \"id\"".into()))?;
        if id != VERSION_ID {
            return Err(VanillaJarError::WrongVersion {
                path: shown,
                found: id,
            });
        }
        Ok(jar)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn entry_error(&self, entry: &str, message: String) -> VanillaJarError {
        VanillaJarError::Entry {
            path: self.path.display().to_string(),
            entry: entry.into(),
            message,
        }
    }

    /// An entry's bytes (`data/minecraft/...`, `assets/...`); `None` when the jar has no such entry.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, VanillaJarError> {
        let result = match &mut self.archive {
            Archive::File(a) => read_entry(a, name),
            Archive::Nested(a) => read_entry(a, name),
        };
        result.map_err(|e| self.entry_error(name, e.to_string()))
    }

    /// The jar's entry names (the vanilla datapack is listed from these).
    pub fn entry_names(&self) -> Vec<String> {
        match &self.archive {
            Archive::File(a) => a.file_names().map(str::to_owned).collect(),
            Archive::Nested(a) => a.file_names().map(str::to_owned).collect(),
        }
    }

    /// `Language.loadDefault`: `en_us.json` with `deprecated.json` applied. As in vanilla, an unreadable
    /// file is logged and leaves the table empty (or without the deprecations).
    pub fn load_language(&mut self) -> Language {
        let mut language = match self
            .read(LANG_PATH)
            .map(|b| b.map(|b| Language::from_json(&String::from_utf8_lossy(&b))))
        {
            Ok(Some(Ok(language))) => language,
            Ok(Some(Err(e))) => {
                tracing::error!("Couldn't read strings from /{LANG_PATH}: {e}");
                Language::default()
            }
            Ok(None) => {
                tracing::error!("Couldn't read strings from /{LANG_PATH}: missing");
                Language::default()
            }
            Err(e) => {
                tracing::error!("Couldn't read strings from /{LANG_PATH}: {e}");
                Language::default()
            }
        };
        match self.read(DEPRECATED_PATH) {
            Ok(None) => {}
            Ok(Some(bytes)) => match serde_json::from_slice::<DeprecatedTranslations>(&bytes) {
                Ok(info) => {
                    let renamed: Option<Vec<_>> = info
                        .renamed
                        .iter()
                        .map(|(k, v)| v.as_str().map(|v| (k, v)))
                        .collect();
                    match renamed {
                        Some(renamed) => language.apply_deprecated(&info.removed, renamed),
                        None => tracing::error!(
                            "Failed to read /{DEPRECATED_PATH}: Failed to parse deprecated language data: a renamed value is not a string"
                        ),
                    }
                }
                Err(e) => tracing::error!("Failed to read /{DEPRECATED_PATH}: {e}"),
            },
            Err(e) => tracing::error!("Failed to read /{DEPRECATED_PATH}: {e}"),
        }
        language
    }
}

/// `DeprecatedTranslationsInfo` (both fields required, as its codec has them).
#[derive(serde::Deserialize)]
struct DeprecatedTranslations {
    removed: Vec<String>,
    /// In the file's order (`serde_json`'s `preserve_order`), as the codec's map keeps it.
    renamed: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            out.start_file(*name, SimpleFileOptions::default()).unwrap();
            out.write_all(bytes).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    const VERSION: &[u8] = br#"{"id": "26.3", "name": "26.3", "protocol_version": 777}"#;
    const LANG: &[u8] = br#"{"a": "A %d", "gone": "x", "old": "O", "old2": "P", "new2": "Q"}"#;
    const DEPRECATED: &[u8] =
        br#"{"removed": ["gone"], "renamed": {"old": "new", "missing": "old2"}}"#;

    #[test]
    fn reads_a_game_jar_and_a_bundler() {
        let dir = tempfile::tempdir().unwrap();
        let game = jar(&[
            ("version.json", VERSION),
            (LANG_PATH, LANG),
            (DEPRECATED_PATH, DEPRECATED),
        ]);
        let game_path = dir.path().join("server.jar");
        std::fs::write(&game_path, &game).unwrap();
        let bundler = jar(&[
            (
                "META-INF/versions.list",
                b"abc\t26.3\t26.3/server-26.3.jar\n",
            ),
            ("META-INF/versions/26.3/server-26.3.jar", &game),
        ]);
        let bundler_path = dir.path().join("mojang_26.3.jar");
        std::fs::write(&bundler_path, bundler).unwrap();

        for path in [&game_path, &bundler_path] {
            let mut jar = VanillaJar::open(path).unwrap();
            assert_eq!(jar.read("nope").unwrap(), None);
            let lang = jar.load_language();
            assert_eq!(lang.get("a"), Some("A %s"));
            // `removed` drops, `renamed` moves, a rename whose key is missing removes its target.
            assert_eq!(
                (lang.get("gone"), lang.get("old"), lang.get("new")),
                (None, None, Some("O"))
            );
            assert_eq!((lang.get("old2"), lang.get("new2")), (None, Some("Q")));
        }

        let other = dir.path().join("old.jar");
        std::fs::write(&other, jar(&[("version.json", br#"{"id": "1.21.11"}"#)])).unwrap();
        let err = VanillaJar::open(&other).unwrap_err();
        assert!(
            matches!(err, VanillaJarError::WrongVersion { ref found, .. } if found == "1.21.11"),
            "{err}"
        );
        assert!(VanillaJar::open(&dir.path().join("absent.jar")).is_err());
    }
}
