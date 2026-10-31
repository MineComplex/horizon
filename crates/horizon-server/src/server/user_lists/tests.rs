//! Reading, writing and deciding with the user lists. The expected files are what the 26.3 classes
//! write (`LeafUserListsOracle.java`, rows `format`).

use std::net::IpAddr;
use std::path::Path;

use jiff::tz::TimeZone;

use super::*;

const NOTCH: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
const JEB: &str = "61699b2e-d327-4a01-9f1e-0ea8c3f06bc6";
/// 2023-11-14 22:13:20 UTC.
const NOW: i64 = 1_700_000_000_000;
const NOT_WHITELISTED: &str = "You are not whitelisted on this server!";

fn notch() -> NameAndId {
    NameAndId {
        id: NOTCH.parse().unwrap(),
        name: "Notch".into(),
    }
}

fn jeb() -> NameAndId {
    NameAndId {
        id: JEB.parse().unwrap(),
        name: "jeb_".into(),
    }
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn lists(dir: &Path) -> UserLists {
    UserLists::load_and_save(dir, TimeZone::UTC, NOW)
}

fn rules(max_players: i32, online_players: usize) -> LoginRules<'static> {
    LoginRules {
        using_whitelist: false,
        whitelist_message: NOT_WHITELISTED,
        max_players,
        online_players,
    }
}

fn op(user: NameAndId, level: PermissionLevel, bypasses_player_limit: bool) -> OpEntry {
    OpEntry {
        user,
        level,
        bypasses_player_limit,
    }
}

fn ban(expires: Option<i64>, reason: Option<&str>) -> Ban {
    Ban::new(
        NOW - 1000,
        Some("Server".into()),
        expires,
        reason.map(str::to_owned),
    )
}

#[test]
fn start_makes_the_files_as_java_does() {
    let dir = tempfile::tempdir().unwrap();
    lists(dir.path());
    for file in [
        USER_BAN_LIST_FILE,
        IP_BAN_LIST_FILE,
        OP_LIST_FILE,
        WHITELIST_FILE,
    ] {
        let text = std::fs::read_to_string(dir.path().join(file)).unwrap();
        assert_eq!(text, "[]", "{file}");
    }
}

#[test]
fn start_rewrites_ops_but_not_an_existing_whitelist() {
    let dir = tempfile::tempdir().unwrap();
    let compact = format!("[{{\"uuid\":\"{NOTCH}\",\"name\":\"Notch\"}}]");
    std::fs::write(dir.path().join(WHITELIST_FILE), &compact).unwrap();
    std::fs::write(dir.path().join(OP_LIST_FILE), &compact).unwrap();
    lists(dir.path());
    let whitelist = std::fs::read_to_string(dir.path().join(WHITELIST_FILE)).unwrap();
    assert_eq!(whitelist, compact);
    let ops = std::fs::read_to_string(dir.path().join(OP_LIST_FILE)).unwrap();
    assert_eq!(
        ops,
        format!(
            "[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"name\": \"Notch\",\n    \"level\": 0,\n    \"bypassesPlayerLimit\": false\n  }}\n]"
        )
    );
}

#[test]
fn files_round_trip_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let whitelist =
        format!("[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"name\": \"Notch\"\n  }}\n]");
    let ops = format!(
        "[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"name\": \"Notch\",\n    \"level\": 4,\n    \"bypassesPlayerLimit\": true\n  }}\n]"
    );
    // Java writes a `null` reason as no property, and `<>&='` as \u escapes.
    let bans = format!(
        "[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"name\": \"Notch\",\n    \"created\": \"2025-03-09 01:59:59 +0000\",\n    \"source\": \"Rcon\\u003c\\u0026\\u003e\",\n    \"expires\": \"2099-12-31 23:59:59 +0000\"\n  }}\n]"
    );
    let ip_bans = "[\n  {\n    \"ip\": \"203.0.113.5\",\n    \"created\": \"2025-01-01 00:00:00 +0000\",\n    \"source\": \"Server\",\n    \"expires\": \"forever\",\n    \"reason\": \"Banned by an operator.\"\n  }\n]";
    let files = [
        (WHITELIST_FILE, whitelist.as_str()),
        (OP_LIST_FILE, ops.as_str()),
        (USER_BAN_LIST_FILE, bans.as_str()),
        (IP_BAN_LIST_FILE, ip_bans),
    ];
    for (file, text) in files {
        std::fs::write(dir.path().join(file), text).unwrap();
    }
    lists(dir.path());
    for (file, text) in files {
        let saved = std::fs::read_to_string(dir.path().join(file)).unwrap();
        assert_eq!(saved, text, "{file}");
    }
}

#[test]
fn entries_are_saved_in_concurrent_hash_map_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut list = UserList::<WhitelistEntry>::new(dir.path().join("w.json"), TimeZone::UTC);
    let users: Vec<NameAndId> = (0u128..30)
        .map(|i| NameAndId {
            id: uuid::Uuid::from_u128(i.wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835)),
            name: format!("p{i}"),
        })
        .collect();
    for user in &users {
        assert!(list.add(WhitelistEntry { user: user.clone() }, NOW));
    }
    let saved = std::fs::read_to_string(list.file()).unwrap();
    // What was saved is what a fresh start reads and writes again, in the same order.
    let mut again = UserList::<WhitelistEntry>::new(list.file(), TimeZone::UTC);
    again.load(NOW).unwrap();
    assert_eq!(again.render(NOW), saved);
    assert_eq!(saved.matches("\"uuid\"").count(), 30);
    // Not the insertion order: the order of the keys' hashes (checked in `oracle_tests`).
    let names: Vec<&str> = list.entries().map(|e| e.user.name.as_str()).collect();
    let inserted: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
    assert_ne!(names, inserted);
}

#[test]
fn add_replaces_and_remove_saves() {
    let dir = tempfile::tempdir().unwrap();
    let mut list = UserList::<WhitelistEntry>::new(dir.path().join("w.json"), TimeZone::UTC);
    assert!(list.add(WhitelistEntry { user: notch() }, NOW));
    // `equals` is identity for these entries: another entry for the same user replaces it.
    assert!(list.add(WhitelistEntry { user: notch() }, NOW));
    assert!(list.contains(&notch(), NOW));
    assert!(!list.contains(&jeb(), NOW));
    assert!(list.remove(&notch(), NOW));
    assert!(!list.remove(&notch(), NOW));
    assert_eq!(std::fs::read_to_string(list.file()).unwrap(), "[]");
    // Bans compare by value: the same ban again is not added.
    let mut bans = UserList::<UserBan>::new(dir.path().join("b.json"), TimeZone::UTC);
    assert!(bans.add(UserBan::new(&notch(), ban(None, Some("x"))), NOW));
    assert!(!bans.add(UserBan::new(&notch(), ban(None, Some("x"))), NOW));
    assert!(bans.add(UserBan::new(&notch(), ban(None, Some("y"))), NOW));
}

#[test]
fn bans_that_ran_out_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[{{\"uuid\":\"{NOTCH}\",\"name\":\"Notch\",\"created\":\"2023-01-01 00:00:00 +0000\",\"expires\":\"2023-11-14 22:13:19 +0000\"}},\
          {{\"uuid\":\"{JEB}\",\"name\":\"jeb_\",\"created\":\"2023-01-01 00:00:00 +0000\",\"expires\":\"2023-11-14 22:13:21 +0000\"}}]"
    );
    std::fs::write(dir.path().join(USER_BAN_LIST_FILE), text).unwrap();
    let mut lists = lists(dir.path());
    // Notch's ended a second ago: not loaded (CraftBukkit's `checkExpiry`). jeb_'s still holds...
    assert!(!lists.bans.contains(&notch(), NOW));
    assert!(lists.bans.contains(&jeb(), NOW));
    // ... until it ends: `get` drops it, and a save does not write it.
    assert!(lists.bans.get(&jeb(), NOW + 1000).is_some());
    assert!(lists.bans.get(&jeb(), NOW + 1001).is_none());
    assert_eq!(lists.bans.render(NOW + 1001), "[]");
}

#[test]
fn an_unreadable_file_is_moved_aside() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(WHITELIST_FILE), "{\"not\": \"an array\"}").unwrap();
    let lists = lists(dir.path());
    assert!(lists.whitelist.is_empty());
    let backup = std::fs::read_to_string(dir.path().join("whitelist.json.backup")).unwrap();
    assert_eq!(backup, "{\"not\": \"an array\"}");
    // The file is gone, and a whitelist without a file is saved empty.
    assert_eq!(
        std::fs::read_to_string(dir.path().join(WHITELIST_FILE)).unwrap(),
        "[]"
    );
}

#[test]
fn a_ban_with_a_name_but_no_uuid_makes_the_file_unreadable() {
    // Spigot's `parseNameAndId` keeps the entry; `getKeyForUser` then throws a NullPointerException.
    let dir = tempfile::tempdir().unwrap();
    let text = "[{\"name\":\"Notch\",\"expires\":\"forever\"}]";
    std::fs::write(dir.path().join(USER_BAN_LIST_FILE), text).unwrap();
    let lists = lists(dir.path());
    assert!(lists.bans.is_empty());
    assert!(dir.path().join("banned-players.json.backup").exists());
}

#[test]
fn a_ban_with_a_uuid_only_is_kept_and_written_without_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[{{\"uuid\":\"{NOTCH}\",\"created\":\"2025-01-01 00:00:00 +0000\",\"expires\":\"forever\"}}]"
    );
    std::fs::write(dir.path().join(USER_BAN_LIST_FILE), text).unwrap();
    let mut lists = lists(dir.path());
    assert!(lists.bans.contains(&notch(), NOW));
    assert_eq!(
        lists.bans.render(NOW),
        format!(
            "[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"created\": \"2025-01-01 00:00:00 +0000\",\n    \"source\": \"(Unknown)\",\n    \"expires\": \"forever\"\n  }}\n]"
        )
    );
}

#[test]
fn an_empty_whitelist_refuses_everyone_but_operators() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let on = LoginRules {
        using_whitelist: true,
        ..rules(20, 0)
    };
    let refused = Some(Reason::Literal(NOT_WHITELISTED.into()));
    let from = ip("203.0.113.5");
    assert_eq!(lists.can_player_login(&notch(), from, &on, NOW), refused);
    // Off: nobody is asked.
    assert_eq!(
        lists.can_player_login(&notch(), from, &rules(20, 0), NOW),
        None
    );
    // On, listed or operator.
    lists.whitelist.add(WhitelistEntry { user: notch() }, NOW);
    lists
        .ops
        .add(op(jeb(), PermissionLevel::Gamemasters, false), NOW);
    assert_eq!(lists.can_player_login(&notch(), from, &on, NOW), None);
    assert_eq!(lists.can_player_login(&jeb(), from, &on, NOW), None);
    let other = NameAndId::offline("Other");
    assert_eq!(lists.can_player_login(&other, from, &on, NOW), refused);
}

fn json(reason: Option<Reason>) -> String {
    match reason.expect("refused") {
        Reason::Component(json) => json.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_order_of_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let on = LoginRules {
        using_whitelist: true,
        ..rules(0, 0)
    };
    let who = notch();
    let from = ip("203.0.113.5");
    // Everything at once: the player ban comes first, then the whitelist, the IP ban, a full server.
    lists.ip_bans.add(
        IpBan {
            ip: "203.0.113.5".into(),
            ban: ban(None, Some("spam")),
        },
        NOW,
    );
    lists
        .bans
        .add(UserBan::new(&who, ban(None, Some("griefing"))), NOW);
    assert_eq!(
        json(lists.can_player_login(&who, from, &on, NOW)),
        r#"{"translate":"multiplayer.disconnect.banned.reason","with":["griefing"]}"#
    );
    lists.bans.remove(&who, NOW);
    assert_eq!(
        lists.can_player_login(&who, from, &on, NOW),
        Some(Reason::Literal(NOT_WHITELISTED.into()))
    );
    lists
        .whitelist
        .add(WhitelistEntry { user: who.clone() }, NOW);
    assert_eq!(
        json(lists.can_player_login(&who, from, &on, NOW)),
        r#"{"translate":"multiplayer.disconnect.banned_ip.reason","with":["spam"]}"#
    );
    lists.ip_bans.remove("203.0.113.5", NOW);
    assert_eq!(
        lists.can_player_login(&who, from, &on, NOW),
        Some(Reason::translatable("multiplayer.disconnect.server_full"))
    );
    // An operator passes a full server only when its entry says so.
    lists
        .ops
        .add(op(who.clone(), PermissionLevel::Owners, false), NOW);
    assert!(lists.can_player_login(&who, from, &on, NOW).is_some());
    lists
        .ops
        .add(op(who.clone(), PermissionLevel::Owners, true), NOW);
    assert_eq!(lists.can_player_login(&who, from, &on, NOW), None);
}

#[test]
fn ban_messages_carry_the_reason_and_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let who = notch();
    let day = 86_400_000;
    lists
        .bans
        .add(UserBan::new(&who, ban(Some(NOW + day), None)), NOW);
    assert_eq!(
        json(lists.can_player_login(&who, ip("1.2.3.4"), &rules(20, 0), NOW)),
        r#"{"translate":"multiplayer.disconnect.banned.reason","with":[{"translate":"multiplayer.disconnect.banned.reason.default"}],"extra":[{"translate":"multiplayer.disconnect.banned.expiration","with":["2023-11-15 at 22:13:20 UTC"]}]}"#
    );
    // A second after it ended, the ban is gone.
    let later = NOW + day + 1000;
    assert_eq!(
        lists.can_player_login(&who, ip("1.2.3.4"), &rules(20, 0), later),
        None
    );
}

#[test]
fn permissions_follow_the_operator_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let default = PermissionLevel::Owners;
    let level = |lists: &mut UserLists| lists.profile_permissions(&notch(), default, NOW);
    assert_eq!(level(&mut lists), PermissionLevel::All);
    lists
        .ops
        .add(op(notch(), PermissionLevel::Moderators, false), NOW);
    assert_eq!(level(&mut lists), PermissionLevel::Moderators);
}

#[test]
fn addresses_as_the_ip_ban_list_keys_them() {
    assert_eq!(ip_key(ip("203.0.113.5")), "203.0.113.5");
    // `InetSocketAddress.toString()` of IPv6 is `/[...]:port`: the text before the first colon.
    assert_eq!(ip_key(ip("2001:db8::1")), "[2001");
    assert_eq!(ip_key(ip("::1")), "[0");
    assert_eq!(ip_key(ip("::ffff:1.2.3.4")), "1.2.3.4");
}
