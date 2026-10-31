//! Reading, writing and deciding with the user lists. The expected files are what the 26.3 classes
//! write (`LeafUserListsOracle.java`, rows `format`).

use std::path::Path;

use super::*;

const NOTCH: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
const JEB: &str = "61699b2e-d327-4a01-9f1e-0ea8c3f06bc6";
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

fn lists(dir: &Path) -> UserLists {
    UserLists::load_and_save(dir)
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

#[test]
fn start_makes_the_files_as_java_does() {
    let dir = tempfile::tempdir().unwrap();
    lists(dir.path());
    for file in [OP_LIST_FILE, WHITELIST_FILE] {
        let text = std::fs::read_to_string(dir.path().join(file)).unwrap();
        assert_eq!(text, "[]", "{file}");
    }
    // No ban lists: MineComplex's cleanup removed them.
    assert!(!dir.path().join("banned-players.json").exists());
    assert!(!dir.path().join("banned-ips.json").exists());
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
    // Java writes `<>&='` as \u escapes.
    let ops = format!(
        "[\n  {{\n    \"uuid\": \"{NOTCH}\",\n    \"name\": \"a\\u003c\\u0026\\u003e\",\n    \"level\": 4,\n    \"bypassesPlayerLimit\": true\n  }}\n]"
    );
    let files = [
        (WHITELIST_FILE, whitelist.as_str()),
        (OP_LIST_FILE, ops.as_str()),
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
    let mut list = UserList::<WhitelistEntry>::new(dir.path().join("w.json"));
    let users: Vec<NameAndId> = (0u128..30)
        .map(|i| NameAndId {
            id: uuid::Uuid::from_u128(i.wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835)),
            name: format!("p{i}"),
        })
        .collect();
    for user in &users {
        assert!(list.add(WhitelistEntry { user: user.clone() }));
    }
    let saved = std::fs::read_to_string(list.file()).unwrap();
    // What was saved is what a fresh start reads and writes again, in the same order.
    let mut again = UserList::<WhitelistEntry>::new(list.file());
    again.load().unwrap();
    assert_eq!(again.render(), saved);
    assert_eq!(saved.matches("\"uuid\"").count(), 30);
    // Not the insertion order: the order of the keys' hashes (checked in `oracle_tests`).
    let names: Vec<&str> = list.entries().map(|e| e.user.name.as_str()).collect();
    let inserted: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
    assert_ne!(names, inserted);
}

#[test]
fn add_replaces_and_remove_saves() {
    let dir = tempfile::tempdir().unwrap();
    let mut list = UserList::<WhitelistEntry>::new(dir.path().join("w.json"));
    assert!(list.add(WhitelistEntry { user: notch() }));
    assert!(list.add(WhitelistEntry { user: notch() }));
    assert!(list.contains(&notch()));
    assert!(!list.contains(&jeb()));
    assert!(list.remove(&notch()));
    assert!(!list.remove(&notch()));
    assert_eq!(std::fs::read_to_string(list.file()).unwrap(), "[]");
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
fn an_empty_whitelist_refuses_everyone_but_operators() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let on = LoginRules {
        using_whitelist: true,
        ..rules(20, 0)
    };
    let refused = Some(Reason::Literal(NOT_WHITELISTED.into()));
    assert_eq!(lists.can_player_login(&notch(), &on), refused);
    // Off: nobody is asked.
    assert_eq!(lists.can_player_login(&notch(), &rules(20, 0)), None);
    // On, listed or operator.
    lists.whitelist.add(WhitelistEntry { user: notch() });
    lists
        .ops
        .add(op(jeb(), PermissionLevel::Gamemasters, false));
    assert_eq!(lists.can_player_login(&notch(), &on), None);
    assert_eq!(lists.can_player_login(&jeb(), &on), None);
    let other = NameAndId::offline("Other");
    assert_eq!(lists.can_player_login(&other, &on), refused);
}

#[test]
fn a_full_server_refuses_all_but_operators_that_bypass() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    let full = rules(2, 2);
    let refused = Some(Reason::translatable("multiplayer.disconnect.server_full"));
    assert_eq!(lists.can_player_login(&notch(), &full), refused);
    assert_eq!(lists.can_player_login(&notch(), &rules(3, 2)), None);
    lists.ops.add(op(notch(), PermissionLevel::Owners, false));
    assert_eq!(lists.can_player_login(&notch(), &full), refused);
    lists.ops.add(op(notch(), PermissionLevel::Owners, true));
    assert_eq!(lists.can_player_login(&notch(), &full), None);
    // The whitelist comes first.
    let both = LoginRules {
        using_whitelist: true,
        ..full
    };
    assert_eq!(lists.can_player_login(&notch(), &both), None);
    assert_eq!(
        lists.can_player_login(&jeb(), &both),
        Some(Reason::Literal(NOT_WHITELISTED.into()))
    );
}

#[test]
fn the_whitelist_message_with_codes_is_a_component() {
    let dir = tempfile::tempdir().unwrap();
    let lists = lists(dir.path());
    let on = LoginRules {
        using_whitelist: true,
        whitelist_message: "§cNot §lwhitelisted",
        ..rules(20, 0)
    };
    let Some(Reason::Component(json)) = lists.can_player_login(&notch(), &on) else {
        panic!("a component")
    };
    assert_eq!(
        json.to_string(),
        r#"{"text":"Not ","extra":[{"text":"whitelisted","bold":true}],"color":"red"}"#
    );
}

#[test]
fn permissions_follow_the_operator_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut lists = lists(dir.path());
    assert_eq!(lists.profile_permissions(&notch()), PermissionLevel::All);
    lists
        .ops
        .add(op(notch(), PermissionLevel::Moderators, false));
    assert_eq!(
        lists.profile_permissions(&notch()),
        PermissionLevel::Moderators
    );
}
