//! Taken from azalea-language 0.16.0 (https://github.com/azalea-rs/azalea, MIT).

use std::{collections::HashMap, sync::LazyLock};

use compact_str::CompactString;

static STORAGE: LazyLock<Box<[(CompactString, CompactString)]>> = LazyLock::new(|| {
    let json =
        std::fs::read_to_string(lang_file())
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<CompactString, CompactString>>(&s).ok())
            .unwrap_or_default();
    let mut json = json.into_iter().collect::<Vec<_>>();

    // sort by key to make binary search work
    json.sort_by(|a, b| a.0.cmp(&b.0));

    json.into_boxed_slice()
});

/// Get the string for the given translation ID.
///
/// This just does a lookup in Minecraft's `en_us.json` file.
pub fn get(key: &str) -> Option<&str> {
    let key = CompactString::from(key);
    let storage = &*STORAGE;
    // more memory efficient than a hashmap lookup
    let index = storage.binary_search_by(|(k, _)| k.cmp(&key));
    index.ok().map(|i| storage[i].1.as_str())
}

/// Where Minecraft's `en_us.json` is read from: `HORIZON_LANG_FILE`, else `lang/en_us.json` beside the
/// server (taken from the server jar's `assets/minecraft/lang/en_us.json`; Mojang's file is not committed).
fn lang_file() -> std::path::PathBuf {
    std::env::var_os("HORIZON_LANG_FILE").map(Into::into).unwrap_or_else(|| "lang/en_us.json".into())
}
