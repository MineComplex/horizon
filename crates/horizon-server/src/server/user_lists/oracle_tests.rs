//! The user lists against the real classes: `LeafUserListsOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs the 26.3 jar's `StoredUserList`
//! family, the JDK's `ConcurrentHashMap` and `UUID`; this compares every row.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafUserListsOracle.java user_lists.jsonl
//! HORIZON_USER_LISTS_ORACLE=user_lists.jsonl \
//!     cargo test -p horizon-server user_lists_match_java -- --ignored --nocapture
//! ```

use serde_json::Value;

use super::map::ConcurrentMap;
use super::{Entry, LoadFailure, OpEntry, UserList, WhitelistEntry, java_uuid};

fn str_of<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {row}"))
}

#[test]
#[ignore = "needs HORIZON_USER_LISTS_ORACLE (LeafUserListsOracle.java on the 26.3 jar)"]
fn user_lists_match_java() {
    let path = std::env::var_os("HORIZON_USER_LISTS_ORACLE").expect("HORIZON_USER_LISTS_ORACLE");
    let text = std::fs::read_to_string(path).expect("the oracle's output");
    let dir = tempfile::tempdir().unwrap();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let kind = str_of(&row, "kind").to_owned();
        *counts.entry(kind.clone()).or_default() += 1;
        match kind.as_str() {
            "chm" => {
                let mut map = ConcurrentMap::default();
                for op in row["script"].as_array().unwrap() {
                    let op = op.as_str().unwrap();
                    match op.split_once(':') {
                        Some(("p", key)) => map.put(key.to_owned(), ()),
                        Some(("r", key)) => {
                            map.remove(key);
                        }
                        _ => map.clear(),
                    }
                }
                let ours: Vec<&str> = map.iter().map(|(k, _)| k).collect();
                let java: Vec<&str> = row["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap())
                    .collect();
                assert_eq!(ours, java, "{line}");
            }
            "format" => {
                let file = dir.path().join("format.json");
                let input = str_of(&row, "input");
                let output = match str_of(&row, "list") {
                    "whitelist" => render::<WhitelistEntry>(&file, input),
                    _ => render::<OpEntry>(&file, input),
                };
                assert_eq!(output, str_of(&row, "output"), "{line}");
            }
            "read" => {
                let input = str_of(&row, "input");
                let (rows, result) = match str_of(&row, "list") {
                    "whitelist" => {
                        read::<WhitelistEntry>(input, |e| format!("{} {}", e.user.id, e.user.name))
                    }
                    _ => read::<OpEntry>(input, |e| {
                        format!(
                            "{} {} {} {}",
                            e.user.id,
                            e.user.name,
                            e.level.id(),
                            e.bypasses_player_limit
                        )
                    }),
                };
                match row.get("error").and_then(Value::as_str) {
                    None => {
                        assert_eq!(result, Ok(()), "{line}");
                        let java: Vec<&str> = row["entries"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|e| e.as_str().unwrap())
                            .collect();
                        assert_eq!(rows, java, "{line}");
                    }
                    // Gson's own exceptions are `JsonParseException`s: the file is moved aside.
                    Some(class) if class.starts_with("com.google.gson.") => {
                        assert!(matches!(result, Err(LoadFailure::Unreadable(_))), "{line}");
                    }
                    Some(_) => {
                        assert!(matches!(result, Err(LoadFailure::Failed(_))), "{line}");
                    }
                }
            }
            "uuid" => {
                assert_eq!(
                    java_uuid(str_of(&row, "text")).map(|u| u.to_string()),
                    row.get("uuid").and_then(Value::as_str).map(str::to_owned),
                    "{line}"
                );
            }
            other => panic!("unknown row {other}"),
        }
    }
    println!("checked rows: {counts:?}");
    assert_eq!(counts.len(), 4, "{counts:?}");
}

/// `load` then `save` of one list file, and the bytes written.
fn render<E: Entry>(file: &std::path::Path, input: &str) -> String {
    std::fs::write(file, input).unwrap();
    let mut list = UserList::<E>::new(file);
    list.load().unwrap();
    list.save().unwrap();
    std::fs::read_to_string(file).unwrap()
}

fn read<E: Entry>(
    input: &str,
    row: impl Fn(&E) -> String,
) -> (Vec<String>, Result<(), LoadFailure>) {
    let mut list = UserList::<E>::new("unused.json");
    let result = list.load_text(input);
    let mut rows: Vec<String> = list.entries().map(row).collect();
    rows.sort();
    (rows, result)
}
