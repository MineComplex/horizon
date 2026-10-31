//! The user lists against the real classes: `LeafUserListsOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs the 26.3 jar's `StoredUserList`
//! family, the JDK's `ConcurrentHashMap`, `SimpleDateFormat` and `UUID`; this compares every row.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafUserListsOracle.java user_lists.jsonl
//! HORIZON_USER_LISTS_ORACLE=user_lists.jsonl \
//!     cargo test -p horizon-server user_lists_match_java -- --ignored --nocapture
//! ```

use std::net::IpAddr;

use jiff::tz::TimeZone;
use serde_json::Value;

use super::map::ConcurrentMap;
use super::{
    Entry, IpBan, LoadFailure, OpEntry, UserBan, UserList, WhitelistEntry, format_date,
    format_login_date, ip_key, java_uuid, parse_date,
};

const NOW: i64 = 1_700_000_000_000;

fn zone(name: &str) -> TimeZone {
    TimeZone::get(if name == "default" {
        "America/New_York"
    } else {
        name
    })
    .expect("tz database")
}

fn str_of<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {row}"))
}

fn rows_of<E: Entry>(list: &UserList<E>, row: impl Fn(&E) -> String) -> Vec<String> {
    let mut rows: Vec<String> = list.entries().map(row).collect();
    rows.sort();
    rows
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
                let tz = zone("default");
                let output = match str_of(&row, "list") {
                    "whitelist" => render::<WhitelistEntry>(&file, input, tz),
                    "ops" => render::<OpEntry>(&file, input, tz),
                    "bans" => render::<UserBan>(&file, input, tz),
                    _ => render::<IpBan>(&file, input, tz),
                };
                assert_eq!(output, str_of(&row, "output"), "{line}");
            }
            "read" => {
                let input = str_of(&row, "input");
                let (rows, result) = match str_of(&row, "list") {
                    "whitelist" => {
                        read::<WhitelistEntry>(input, |e| format!("{} {}", e.user.id, e.user.name))
                    }
                    "ops" => read::<OpEntry>(input, |e| {
                        format!(
                            "{} {} {} {}",
                            e.user.id,
                            e.user.name,
                            e.level.id(),
                            e.bypasses_player_limit
                        )
                    }),
                    _ => read::<UserBan>(input, |e| {
                        format!(
                            "{} {} {} {}",
                            e.id,
                            e.name.clone().unwrap(),
                            e.ban.source,
                            e.ban.reason.clone().unwrap_or("null".into())
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
            "date_format" => {
                let tz = zone(str_of(&row, "zone"));
                let millis = row["millis"].as_i64().unwrap();
                assert_eq!(format_date(millis, &tz), str_of(&row, "file"), "{line}");
                // Java names the zone from its current raw offset, so only recent instants are comparable.
                let login = str_of(&row, "login");
                if !login.is_empty() && millis >= 1_700_000_000_000 {
                    assert_eq!(format_login_date(millis, &tz), login, "{line}");
                }
            }
            "date_parse" => {
                assert_eq!(
                    parse_date(str_of(&row, "text")),
                    row.get("millis").and_then(Value::as_i64),
                    "{line}"
                );
            }
            "uuid" => {
                assert_eq!(
                    java_uuid(str_of(&row, "text")).map(|u| u.to_string()),
                    row.get("uuid").and_then(Value::as_str).map(str::to_owned),
                    "{line}"
                );
            }
            "ip_key" => {
                let address: IpAddr = str_of(&row, "address").parse().unwrap();
                assert_eq!(ip_key(address), str_of(&row, "key"), "{line}");
            }
            other => panic!("unknown row {other}"),
        }
    }
    println!("checked rows: {counts:?}");
    assert_eq!(counts.len(), 7, "{counts:?}");
}

/// `load` then `save` of one list file, and the bytes written.
fn render<E: Entry>(file: &std::path::Path, input: &str, tz: TimeZone) -> String {
    std::fs::write(file, input).unwrap();
    let mut list = UserList::<E>::new(file, tz);
    list.load(NOW).unwrap();
    list.save(NOW).unwrap();
    std::fs::read_to_string(file).unwrap()
}

fn read<E: Entry>(
    input: &str,
    row: impl Fn(&E) -> String,
) -> (Vec<String>, Result<(), LoadFailure>) {
    let mut list = UserList::<E>::new("unused.json", TimeZone::UTC);
    let result = list.load_text(input, NOW);
    (rows_of(&list, row), result)
}
