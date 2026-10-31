//! The commands on a server over the synthetic world of the runtime's tests. No language is
//! installed here, so a message reads as its translation key; arguments are checked through the
//! command's result and the state it changed.

use super::*;
use crate::server::chunk_system::ChunkSettings;
use crate::server::runtime::tests::{config, start_with};
use crate::server::user_lists::NameAndId;
use crate::world::level_storage::Difficulty;

fn server() -> (Server, tempfile::TempDir) {
    let (mut server, dir) = start_with(config(8), ChunkSettings::default(), |_| {});
    server.commands.log = Some(Vec::new());
    server.config.proxy_online_mode = false;
    (server, dir)
}

/// Runs a console line; the feedback it printed and its result.
fn console(server: &mut Server, line: &str) -> (Vec<String>, Option<(bool, i32)>) {
    let source = server.console_source();
    let outcome = server.perform_prefixed_command(source, line);
    (
        std::mem::take(server.commands.log.as_mut().unwrap()),
        outcome,
    )
}

fn say(server: &mut Server, line: &str) -> Vec<String> {
    console(server, line).0
}

fn steve() -> NameAndId {
    NameAndId::offline("Steve")
}

#[test]
fn an_unknown_command_gets_leafs_message_with_the_position() {
    let (mut server, _dir) = server();
    let out = say(&mut server, "frobnicate now");
    assert_eq!(out.len(), 1);
    // Message, new line, then the input up to the error with the marker (no language: keys).
    assert_eq!(
        out[0],
        "command.unknown.command\nfrobnicate now<--[HERE]"
            .replace("<--[HERE]", "command.context.here")
    );
    // An incomplete command stops before its missing argument.
    let out = say(&mut server, "/whitelist add");
    assert_eq!(
        out[0],
        "command.unknown.command\n...telist addcommand.context.here"
    );
    // Too many arguments: the error points at the rest, underlined (the text is the same).
    let out = say(&mut server, "seed extra");
    assert!(
        out[0].starts_with("command.unknown.argument\nseed"),
        "{out:?}"
    );
}

#[test]
fn a_long_input_is_cut_ten_characters_before_the_error() {
    let (mut server, _dir) = server();
    let out = say(&mut server, "tick rate 20 and some more");
    assert!(out[0].contains("..."), "{out:?}");
}

#[test]
fn a_source_sees_only_the_commands_its_level_allows() {
    let (mut server, _dir) = server();
    let mut source = server.console_source();
    source.permissions = PermissionLevel::Gamemasters;
    // `seed` needs level 2, `stop` level 4: the second is as good as unknown.
    assert_eq!(
        server.perform_prefixed_command(source.clone(), "seed"),
        Some((true, 0))
    );
    assert!(
        server
            .perform_prefixed_command(source.clone(), "stop")
            .is_none()
    );
    assert!(server.exit_code.is_none());
    source.permissions = PermissionLevel::All;
    assert!(
        server
            .perform_prefixed_command(source.clone(), "seed")
            .is_none()
    );
    assert!(server.perform_prefixed_command(source, "list").is_some());
}

#[test]
fn stop_halts_the_server() {
    let (mut server, _dir) = server();
    let (out, outcome) = console(&mut server, "stop");
    assert_eq!(out, ["commands.stop.stopping"]);
    assert_eq!(outcome, Some((true, 1)));
    assert_eq!(server.exit_code, Some(0));
}

#[test]
fn the_namespaced_alias_runs_the_same_command() {
    let (mut server, _dir) = server();
    let (out, outcome) = console(&mut server, "minecraft:seed");
    assert_eq!(out.len(), 1);
    assert_eq!(outcome.map(|o| o.0), Some(true));
    let (out, _) = console(&mut server, "minecraft:time query gametime");
    assert_eq!(out, ["commands.time.query.gametime"]);
}

#[test]
fn seed_reports_the_levels_seed_as_an_int() {
    let (mut server, _dir) = server();
    let seed = server.world.levels[0].seed;
    let (out, outcome) = console(&mut server, "seed");
    assert_eq!(out, ["commands.seed.success"]);
    assert_eq!(outcome, Some((true, seed as i32)));
}

#[test]
fn save_commands() {
    let (mut server, _dir) = server();
    let (out, outcome) = console(&mut server, "save-all flush");
    assert_eq!(out, ["commands.save.saving", "commands.save.success"]);
    assert_eq!(outcome, Some((true, 1)));
    assert_eq!(say(&mut server, "save-on"), ["commands.save.alreadyOn"]);
    assert_eq!(say(&mut server, "save-off"), ["commands.save.disabled"]);
    assert!(server.commands.no_save);
    assert_eq!(say(&mut server, "save-off"), ["commands.save.alreadyOff"]);
    // `save-all` forces the save even when saving is off.
    assert_eq!(say(&mut server, "save-all")[1], "commands.save.success");
    assert_eq!(say(&mut server, "save-on"), ["commands.save.enabled"]);
    assert!(!server.commands.no_save);
}

#[test]
fn list_with_nobody_online() {
    let (mut server, _dir) = server();
    let (out, outcome) = console(&mut server, "list");
    assert_eq!(out, ["commands.list.players"]);
    assert_eq!(outcome, Some((true, 0)));
    assert_eq!(console(&mut server, "list uuids").1, Some((true, 0)));
}

#[test]
fn whitelist_commands_edit_the_list_and_the_setting() {
    let (mut server, dir) = server();
    assert_eq!(
        say(&mut server, "whitelist list"),
        ["commands.whitelist.none"]
    );
    assert_eq!(
        say(&mut server, "whitelist off"),
        ["commands.whitelist.alreadyOff"]
    );
    // Offline mode: the name resolves to its offline profile, which is saved.
    let (out, outcome) = console(&mut server, "whitelist add Steve");
    assert_eq!(out, ["commands.whitelist.add.success"]);
    assert_eq!(outcome, Some((true, 1)));
    assert!(server.user_lists.whitelist.contains(&steve()));
    let text = std::fs::read_to_string(dir.path().join("whitelist.json")).unwrap();
    assert!(
        text.contains(&steve().id.to_string()) && text.contains("Steve"),
        "{text}"
    );
    assert_eq!(
        say(&mut server, "whitelist add Steve"),
        ["commands.whitelist.add.failed"]
    );
    let (out, outcome) = console(&mut server, "whitelist list");
    assert_eq!(out, ["commands.whitelist.list"]);
    assert_eq!(outcome, Some((true, 1)));

    assert_eq!(
        say(&mut server, "whitelist on"),
        ["commands.whitelist.enabled"]
    );
    assert!(server.config.white_list);
    assert_eq!(
        say(&mut server, "whitelist on"),
        ["commands.whitelist.alreadyOn"]
    );
    let properties = std::fs::read_to_string(dir.path().join("server.properties")).unwrap();
    assert!(properties.contains("white-list=true"), "{properties}");
    assert_eq!(
        say(&mut server, "whitelist reload"),
        ["commands.whitelist.reloaded"]
    );

    assert_eq!(
        say(&mut server, "whitelist remove Steve"),
        ["commands.whitelist.remove.success"]
    );
    assert_eq!(
        say(&mut server, "whitelist remove Steve"),
        ["commands.whitelist.remove.failed"]
    );
    assert_eq!(
        say(&mut server, "whitelist off"),
        ["commands.whitelist.disabled"]
    );
    let properties = std::fs::read_to_string(dir.path().join("server.properties")).unwrap();
    assert!(properties.contains("white-list=false"), "{properties}");
}

#[test]
fn op_and_deop_edit_ops_json() {
    let (mut server, dir) = server();
    assert_eq!(say(&mut server, "deop Steve"), ["commands.deop.failed"]);
    assert_eq!(say(&mut server, "op Steve"), ["commands.op.success"]);
    assert!(server.user_lists.is_op(&steve()));
    assert_eq!(
        server.user_lists.profile_permissions(&steve()),
        PermissionLevel::Owners
    );
    let text = std::fs::read_to_string(dir.path().join("ops.json")).unwrap();
    assert!(text.contains("\"level\": 4"), "{text}");
    assert_eq!(say(&mut server, "op Steve"), ["commands.op.failed"]);
    assert_eq!(say(&mut server, "deop Steve"), ["commands.deop.success"]);
    assert!(!server.user_lists.is_op(&steve()));
}

#[test]
fn profile_selectors_follow_vanilla_without_entities() {
    let (mut server, _dir) = server();
    // `@e`, `@n` and `@s` include entities.
    assert!(console(&mut server, "op @e").1.is_none());
    assert_eq!(
        say(&mut server, "op @e")[0].lines().next(),
        Some("argument.player.entities")
    );
    // `@a` finds nobody online.
    assert_eq!(
        say(&mut server, "op @a"),
        ["argument.entity.notfound.player"]
    );
    assert_eq!(
        say(&mut server, "op @p"),
        ["argument.entity.notfound.player"]
    );
    // An unknown selector type and options are the parser's errors.
    assert_eq!(
        say(&mut server, "op @x")[0].lines().next(),
        Some("argument.entity.selector.unknown")
    );
    assert_eq!(
        say(&mut server, "op @a[distance=..5]")[0].lines().next(),
        Some("argument.entity.selector.not_allowed")
    );
}

#[test]
fn proxy_online_mode_needs_a_known_player() {
    let (mut server, _dir) = server();
    server.config.proxy_online_mode = true;
    assert_eq!(say(&mut server, "op Nobody"), ["argument.player.unknown"]);
}

#[test]
fn difficulty_queries_sets_and_refuses_the_same() {
    let (mut server, _dir) = server();
    let (out, outcome) = console(&mut server, "difficulty");
    assert_eq!(out, ["commands.difficulty.query"]);
    assert_eq!(outcome, Some((true, Difficulty::Normal as i32)));
    let (out, outcome) = console(&mut server, "difficulty hard");
    assert_eq!(out, ["commands.difficulty.success"]);
    assert_eq!(outcome, Some((true, 0)));
    assert_eq!(
        server.world.levels[0].overrides.difficulty,
        Difficulty::Hard
    );
    assert_eq!(server.world.data.difficulty, Difficulty::Hard);
    assert_eq!(
        say(&mut server, "difficulty hard"),
        ["commands.difficulty.failure"]
    );
    // A hardcore level stays hard.
    server.world.levels[0].overrides.hardcore = true;
    assert_eq!(
        say(&mut server, "difficulty easy"),
        ["commands.difficulty.success"]
    );
    assert_eq!(
        server.world.levels[0].overrides.difficulty,
        Difficulty::Hard
    );
}

#[test]
fn time_set_add_query_pause_rate() {
    let (mut server, _dir) = server();
    let clock = "minecraft:overworld";
    let total = |server: &Server| server.world.levels[0].clocks.state(clock).unwrap();
    assert_eq!(console(&mut server, "time set 6000").1, Some((true, 6000)));
    assert_eq!(total(&server).total_ticks, 6000);
    assert_eq!(
        say(&mut server, "time set 6000"),
        ["commands.time.set.already_at_time"]
    );
    assert_eq!(console(&mut server, "time add 1d").1, Some((true, 30000)));
    assert_eq!(console(&mut server, "time add -40000").1, Some((true, 0)));
    assert_eq!(console(&mut server, "time query time").1, Some((true, 0)));
    assert_eq!(
        console(&mut server, "time query gametime").0,
        ["commands.time.query.gametime"]
    );
    assert_eq!(
        say(&mut server, "time resume"),
        ["commands.time.pause.already_running"]
    );
    assert_eq!(say(&mut server, "time pause"), ["commands.time.pause"]);
    assert!(total(&server).paused);
    assert_eq!(
        say(&mut server, "time pause"),
        ["commands.time.pause.already_paused"]
    );
    assert_eq!(say(&mut server, "time rate 2.5"), ["commands.time.rate"]);
    assert_eq!(total(&server).rate, 2.5);
    assert_eq!(
        say(&mut server, "time rate 2.5"),
        ["commands.time.rate.already_same"]
    );
    // The argument's bounds are the parser's.
    assert!(console(&mut server, "time rate 0").1.is_none());
    // The test registries have no timelines: no marker is known.
    assert_eq!(
        say(&mut server, "time set day"),
        ["commands.time.no_time_marker_found"]
    );
    assert!(server.world.levels[0].clocks.take_dirty());
}

#[test]
fn time_markers_resolve_like_clock_time_marker() {
    let day = time_marker(1000);
    // Before it: later today; after it: tomorrow; at it: it occurs.
    assert_eq!(day.resolve(0), 1000);
    assert_eq!(day.resolve(5000), 24000 + 1000);
    assert_eq!(day.resolve(24000 * 3 + 500), 24000 * 3 + 1000);
    assert!(day.occurs_at(1000) && day.occurs_at(24000 + 1000));
    assert!(!day.occurs_at(1001));
}

fn time_marker(ticks: i64) -> world::TimeMarker {
    world::TimeMarker::for_test(ticks, Some(24000))
}

#[test]
fn weather_sets_the_levels_weather() {
    let (mut server, _dir) = server();
    assert_eq!(
        console(&mut server, "weather rain 100").1,
        Some((true, 100))
    );
    let w = server.world.levels[0].weather;
    assert!(w.raining && !w.thundering);
    assert_eq!(
        (w.rain_time, w.thunder_time, w.clear_weather_time),
        (100, 100, 0)
    );
    assert_eq!(console(&mut server, "weather thunder 7").1, Some((true, 7)));
    assert!(server.world.levels[0].weather.thundering);
    // No duration: a draw from the type's range, and the result is the -1 given.
    assert_eq!(console(&mut server, "weather clear").1, Some((true, -1)));
    let w = server.world.levels[0].weather;
    assert!(!w.raining && !w.thundering);
    assert!((12000..=180_000).contains(&w.clear_weather_time));
    // The duration is at least 1.
    assert!(console(&mut server, "weather rain 0").1.is_none());
}

#[test]
fn tick_commands() {
    let (mut server, _dir) = server();
    assert_eq!(say(&mut server, "tick step"), ["commands.tick.step.fail"]);
    assert_eq!(
        say(&mut server, "tick step stop"),
        ["commands.tick.step.stop.fail"]
    );
    assert_eq!(
        say(&mut server, "tick sprint stop"),
        ["commands.tick.sprint.stop.fail"]
    );
    assert_eq!(console(&mut server, "tick rate 40").1, Some((true, 40)));
    assert_eq!(server.rate.tick_rate(), 40.0);
    assert_eq!(console(&mut server, "tick freeze").1, Some((true, 1)));
    assert!(server.rate.is_frozen());
    assert_eq!(
        say(&mut server, "tick step 5"),
        ["commands.tick.step.success"]
    );
    assert_eq!(console(&mut server, "tick unfreeze").1, Some((true, 0)));
    let (out, outcome) = console(&mut server, "tick query");
    assert_eq!(out.len(), 3, "{out:?}");
    assert_eq!(outcome, Some((true, 40)));
    assert_eq!(
        say(&mut server, "tick sprint 20"),
        ["commands.tick.status.sprinting"]
    );
    assert!(server.rate.is_sprinting());
    assert_eq!(say(&mut server, "tick sprint stop").len(), 2);
    // The bounds are the parser's.
    assert!(console(&mut server, "tick rate 0.5").1.is_none());
}

#[test]
fn the_commands_packet_follows_the_level() {
    use crate::command::network::Stub;
    let (server, _dir) = server();
    let count = |level: u8| {
        let tree = server.commands.tree.as_ref().unwrap();
        let packet = PlayerCommands::new(
            tree,
            |key| !matches!(key, Key::Permission(needed) if needed > level),
            true,
        )
        .to_packet(
            |key| matches!(key, Key::Permission(n) if n > 0),
            |_| SuggestionProvider::AskServer,
        );
        let roots = &packet.entries[packet.root as usize];
        roots
            .children
            .iter()
            .filter_map(|&c| match &packet.entries[c as usize].stub {
                Stub::Literal(name) if !name.contains(':') => Some(name.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(count(0), ["list"]);
    assert_eq!(count(2), ["difficulty", "list", "seed", "time", "weather"]);
    assert_eq!(
        count(4),
        [
            "difficulty",
            "list",
            "seed",
            "tick",
            "time",
            "weather",
            "deop",
            "op",
            "save-all",
            "save-off",
            "save-on",
            "stop",
            "whitelist"
        ]
    );
}
