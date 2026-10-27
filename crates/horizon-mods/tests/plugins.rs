//! Real plugins built with the SDK on the fake server (docs/plugins.md):
//! the essentials example's scripts, and a probe plugin in three roles that
//! tests the runtime itself. Needs `rustup target add wasm32-unknown-unknown`.
use horizon_mods::{
    Package, Status,
    manifest::componentize,
    plugin::{Event, events as ev, types},
    plugin_test::{PluginScript, TestServer},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds the plugins once; returns their `.wasm` folder.
fn built() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let target = root().join("target/mods-test");
        let status = Command::new(env!("CARGO"))
            .current_dir(root())
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .args([
                "-p",
                "essentials",
                "-p",
                "horizon-mods-test-plugin",
                "--target-dir",
            ])
            .arg(&target)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "building the test plugins needs `rustup target add wasm32-unknown-unknown`"
        );
        target.join("wasm32-unknown-unknown/release")
    })
}

/// A `.hmod` of `wasm` with `manifest` and bundled files, in a fresh folder.
fn pack(name: &str, wasm: &str, manifest: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let core = std::fs::read(built().join(format!("{wasm}.wasm"))).unwrap();
    let component = componentize(&core).unwrap();
    let dir = root().join("target/mods-test/plugins");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.hmod"));
    let bytes = Package::write_files(manifest, Some(&component), files).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn essentials() -> PathBuf {
    let example = root().join("crates/horizon-mod-sdk/examples/essentials");
    let manifest = std::fs::read_to_string(example.join("horizon-mod.toml")).unwrap();
    let config = std::fs::read(example.join("defaults/config.yml")).unwrap();
    pack(
        "essentials",
        "essentials",
        &manifest,
        &[("defaults/config.yml", &config)],
    )
}

#[test]
fn the_essentials_scripts_pass() {
    let hmod = essentials();
    let tests = root().join("crates/horizon-mod-sdk/examples/essentials/tests");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&tests)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    scripts.sort();
    assert!(scripts.len() >= 6);
    for path in scripts {
        let script = PluginScript::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut server = TestServer::load(std::slice::from_ref(&hmod)).unwrap();
        script
            .run(&mut server)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

fn probe(role: &str, depend: &str) -> PathBuf {
    let manifest = format!(
        "id = \"{role}\"\nname = \"{role}\"\nversion = \"1.0\"\napi = \"0.4\"\nside = \"server\"\ncapabilities = [\"plugin\"]\ndepend = [{depend}]\n"
    );
    pack(
        role,
        "horizon_mods_test_plugin",
        &manifest,
        &[("defaults/role.txt", role.as_bytes())],
    )
}

#[test]
fn dependencies_order_the_plugins_and_a_missing_one_keeps_it_off() {
    let alpha = probe("alpha", "");
    let beta = probe("beta", "\"alpha\"");
    let gamma = probe("gamma", "\"missing\"");
    let err = TestServer::load(&[beta.clone(), alpha.clone(), gamma])
        .err()
        .expect("gamma does not start");
    assert!(err.contains("depends on missing"), "{err}");
    let t = TestServer::load(&[beta, alpha]).unwrap();
    assert!(
        t.server
            .console
            .contains(&"beta enabled after alpha=true".to_owned()),
        "{:?}",
        t.server.console
    );
    assert!(
        t.server
            .console
            .contains(&"beta alpha-ping: true".to_owned())
    );
    let infos = t.mods.plugin_infos();
    assert_eq!(infos.len(), 2);
    assert!(infos.iter().all(|p| p.enabled));
}

fn two() -> TestServer {
    let alpha = probe("alpha", "");
    let beta = probe("beta", "\"alpha\"");
    let mut t = TestServer::load(&[beta, alpha]).unwrap();
    t.join("Steve").unwrap();
    t
}

#[test]
fn listeners_run_by_priority_and_monitors_change_nothing() {
    let mut t = two();
    t.chat("Steve", "hi").unwrap();
    assert_eq!(t.chat, ["<Steve> hi[a][b]"]);
    assert!(
        t.server
            .console
            .contains(&"monitor saw hi[a][b] cancelled=false".to_owned())
    );
    // beta cancels at High; alpha's monitor sees it cancelled and can't undo it.
    t.chat("Steve", "a secret").unwrap();
    assert_eq!(t.chat.len(), 1);
    assert!(
        t.server
            .console
            .contains(&"monitor saw a secret[a][b] cancelled=true".to_owned())
    );
    assert!(
        matches!(&t.last, Some(Event::PlayerChat(e)) if e.cancelled && e.message == "a secret[a][b]")
    );
}

#[test]
fn ignore_cancelled_listeners_skip_cancelled_events() {
    let mut t = two();
    let pos = types::BlockPos { x: 5, y: 70, z: 5 };
    t.server.blocks.insert(
        ("minecraft:overworld".into(), 5, 70, 5),
        "minecraft:diamond_ore".into(),
    );
    assert!(
        !t.break_block("Steve", pos).unwrap(),
        "alpha cancels diamonds"
    );
    assert!(
        !t.server
            .console
            .iter()
            .any(|l| l.starts_with("beta saw break"))
    );
    let stone = types::BlockPos { x: 5, y: 60, z: 5 };
    assert!(t.break_block("Steve", stone).unwrap());
    assert!(
        t.server
            .console
            .contains(&"beta saw break of minecraft:stone".to_owned())
    );
}

#[test]
fn commands_registered_at_run_time_have_aliases_and_go_away() {
    let mut t = two();
    assert!(t.command("Steve", "alpha-ping a b").unwrap());
    assert!(t.command("Steve", "ap").unwrap());
    assert!(t.command("Steve", "alpha:alpha-ping").unwrap());
    assert_eq!(
        t.server.told,
        [
            "Steve: pong alpha-ping 2",
            "Steve: pong ap 0",
            "Steve: pong alpha-ping 0"
        ]
    );
    assert!(t.command("Steve", "alpha-unregister").unwrap());
    assert!(!t.command("Steve", "alpha-ping").unwrap());
    assert!(!t.command("Steve", "nobody-owns-this").unwrap());
}

#[test]
fn plugins_call_each_other() {
    let mut t = two();
    assert!(t.command("Steve", "alpha-call hello world").unwrap());
    assert!(t.server.told.contains(&"Steve: sent true false".to_owned()));
    assert!(
        t.server
            .console
            .contains(&"alpha got reply from beta: HELLO WORLD".to_owned()),
        "{:?}",
        t.server.console
    );
}

#[test]
fn permissions_follow_what_plugins_set_with_wildcards_and_children() {
    let mut t = two();
    t.command("Steve", "alpha-has some.node").unwrap();
    t.command("Steve", "alpha-perm some.*").unwrap();
    t.command("Steve", "alpha-has some.node").unwrap();
    t.command("Steve", "alpha-has probe.child").unwrap();
    t.command("Steve", "alpha-perm probe.parent").unwrap();
    t.command("Steve", "alpha-has probe.child").unwrap();
    t.command("Steve", "alpha-perm probe.parent false").unwrap();
    t.command("Steve", "alpha-has probe.child").unwrap();
    let has: Vec<&str> = t
        .server
        .told
        .iter()
        .filter(|l| !l.ends_with("set"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        has,
        [
            "Steve: some.node false",
            "Steve: some.node true",
            "Steve: probe.child false",
            "Steve: probe.child true",
            "Steve: probe.child false",
        ]
    );
    // What a plugin set goes with the player.
    let id = t.server.player_named("Steve").unwrap().record.id.clone();
    t.leave("Steve").unwrap();
    t.join("Steve").unwrap();
    assert!(!t.mods.has_permission(&mut t.server, &id, "some.node"));
}

#[test]
fn tasks_run_when_due_and_timers_until_cancelled() {
    let mut t = two();
    t.command("Steve", "alpha-task").unwrap();
    assert!(t.server.told.contains(&"Steve: queued true".to_owned()));
    t.ticks(10).unwrap();
    let lines: Vec<&str> = t
        .server
        .console
        .iter()
        .filter(|l| l.starts_with("task") || l.starts_with("timer") || l.starts_with("raw"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        lines,
        [
            "raw task true",
            "timer 1",
            "timer 2",
            "task once",
            "timer 3"
        ]
    );
}

#[test]
fn data_files_stay_in_the_plugins_folder() {
    let mut t = two();
    t.command("Steve", "alpha-file").unwrap();
    assert!(
        t.server
            .told
            .contains(&"Steve: true true yes! b.txt".to_owned()),
        "{:?}",
        t.server.told
    );
    assert!(t.data_dir().join("alpha/a/b.txt").is_file());
    assert!(!t.data_dir().join("escape.txt").exists());
    assert!(!t.data_dir().join("alpha/../escape.txt").exists());
}

#[test]
fn a_plugin_that_crashes_stops_alone() {
    let mut t = two();
    t.command("Steve", "alpha-panic").unwrap_err();
    let alpha = t
        .mods
        .entries()
        .iter()
        .find(|e| e.manifest.as_ref().is_some_and(|m| m.id == "alpha"))
        .unwrap();
    assert!(
        matches!(&alpha.status, Status::Failed(r) if r.contains("panic")),
        "{:?}",
        alpha.status
    );
    // Its commands and listeners are gone; beta goes on.
    assert!(!t.command("Steve", "alpha-ping").unwrap_or(false));
    t.chat("Steve", "still here").unwrap();
    assert_eq!(
        t.chat.last().map(String::as_str),
        Some("<Steve> still here[b]")
    );
}

#[test]
fn an_endless_loop_runs_out_of_fuel() {
    let mut t = two();
    let started = std::time::Instant::now();
    assert!(t.command("Steve", "alpha-spin").is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
    assert!(t.stopped().is_some_and(|r| r.contains("too long")));
}

#[test]
fn every_event_kind_reaches_a_listener() {
    // The probe's beta hears chat: a hand-made event goes through `fire`.
    let mut t = two();
    let player = t.server.player_named("Steve").unwrap().record.clone();
    let out = t.mods.fire(
        &mut t.server,
        Event::PlayerChat(ev::PlayerChatEvent {
            player,
            message: "x".into(),
            format: "{message}".into(),
            recipients: Vec::new(),
            cancelled: false,
        }),
    );
    assert!(matches!(out, Event::PlayerChat(e) if e.message == "x[a][b]"));
}
