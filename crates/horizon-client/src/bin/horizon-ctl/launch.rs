//! `horizon-ctl launch`: find a Horizon to start and start it with a control
//! socket. Mirrors where the launcher puts things (launcher/src/backend.rs):
//! versions under `<launcher home>/versions/<tag>`, each with its prepared
//! resources in `.local/horizon-client/<minecraft version>` and run from the
//! game home, where saves and options.txt live.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use horizon_client::logging::{data_dir, game_home};
use serde_json::{Value, json};

#[cfg(windows)]
const CLIENT: &str = "horizon-client.exe";
#[cfg(not(windows))]
const CLIENT: &str = "horizon-client";
#[cfg(windows)]
const SERVER: &str = "horizon-server.exe";
#[cfg(not(windows))]
const SERVER: &str = "horizon-server";

/// A world for the game to play in: `launch --world W [--seed N] [--creative]
/// [--time T] [--weather W] [--dimension D] [--test-terrain]` runs it on a
/// `horizon-server` beside the client and joins it (the client has no worlds
/// of its own).
#[derive(Debug, Default, PartialEq)]
pub struct WorldLaunch {
    pub world: String,
    pub seed: Option<String>,
    pub creative: bool,
    /// `level-type` for a new world (`minecraft:flat`...).
    pub level_type: Option<String>,
    /// Options the server takes as they are (`--time 6000`...).
    pub server_args: Vec<String>,
}

impl WorldLaunch {
    /// Take the world's options out of the game's arguments; None without
    /// `--world`.
    pub fn take(game_args: &mut Vec<String>) -> Result<Option<Self>, String> {
        let mut w = Self::default();
        let mut rest = Vec::new();
        let mut it = std::mem::take(game_args).into_iter();
        let mut world = None;
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
            match arg.as_str() {
                "--world" => world = Some(value()?),
                "--seed" => w.seed = Some(value()?),
                "--creative" => w.creative = true,
                "--level-type" => w.level_type = Some(value()?),
                "--test-terrain" => w.server_args.push(arg),
                "--time" | "--weather" | "--dimension" => {
                    let v = value()?;
                    w.server_args.extend([arg, v]);
                }
                _ => rest.push(arg),
            }
        }
        *game_args = rest;
        match world {
            Some(name) => {
                w.world = name;
                Ok(Some(w))
            }
            None if w.seed.is_some() || w.creative || w.level_type.is_some() || !w.server_args.is_empty() => {
                Err("--seed, --creative, --level-type, --time, --weather, --dimension and --test-terrain need --world".into())
            }
            None => Ok(None),
        }
    }

    /// `server.properties` for a test's server: offline, everyone welcome, the
    /// player an operator (ops.json), nothing protected, flight allowed, the
    /// client's render distance, normal difficulty, never paused.
    pub fn properties(&self, port: u16, level: &str, distance: &str) -> String {
        format!(
            "server-port={port}\nlevel-name={level}\nlevel-seed={}\nlevel-type={}\ngamemode={}\nonline-mode=false\n\
             white-list=false\nenforce-secure-profile=false\nspawn-protection=0\nallow-flight=true\n\
             view-distance={distance}\nsimulation-distance={distance}\ndifficulty=normal\n\
             pause-when-empty-seconds=0\nmotd=Horizon test\n",
            self.seed.as_deref().unwrap_or(""),
            self.level_type.as_deref().unwrap_or("minecraft:normal"),
            if self.creative {
                "creative"
            } else {
                "survival"
            },
        )
    }
}

/// The value after `name` in the game's arguments.
fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// A free TCP port on this machine.
fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

/// A test's server, running.
pub struct Server {
    pub socket: PathBuf,
    pub port: u16,
    pub log: PathBuf,
    pub pid: u32,
}

/// Start `horizon-server` beside the client on `world`, in `home`'s
/// `.horizon-server` folder (the world itself under `home/saves`, or where
/// `--world` names a path), and wait until it listens.
fn start_server(
    game: &Path,
    home: &Path,
    resources: Option<&Path>,
    control: &Path,
    world: &WorldLaunch,
    player: &str,
    distance: &str,
) -> Result<Server, String> {
    let binary = game.with_file_name(SERVER);
    if !binary.is_file() {
        return Err(format!(
            "{} is missing: a --world runs on horizon-server",
            binary.display()
        ));
    }
    let dir = home.join(".horizon-server");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = Path::new(&world.world);
    let (universe, level) = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            (parent.to_owned(), name.to_string_lossy().into_owned())
        }
        _ => (home.join("saves"), world.world.clone()),
    };
    let universe = if universe.is_absolute() {
        universe
    } else {
        home.join(universe)
    };
    let _ = std::fs::create_dir_all(&universe);
    let port = free_port()?;
    let uuid =
        horizon_protocol::crypto::uuid_string(horizon_protocol::crypto::offline_uuid(player));
    let files = [
        ("eula.txt", "eula=true\n".to_owned()),
        (
            "server.properties",
            world.properties(port, &level, distance),
        ),
        (
            "ops.json",
            json!([{ "uuid": uuid, "name": player, "level": 4, "bypassesPlayerLimit": false }])
                .to_string(),
        ),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"))?;
    }
    let file_name = control
        .file_name()
        .map_or("ctl".into(), |n| n.to_string_lossy().into_owned());
    let socket = control.with_file_name(format!("{file_name}.server"));
    let _ = std::fs::remove_file(&socket);
    let log = control.with_file_name(format!("{file_name}.server.log"));
    let out = std::fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&binary);
    if let Some(resources) = resources {
        cmd.arg("--resources").arg(resources);
    }
    cmd.arg("--control-socket")
        .arg(&socket)
        .args(&world.server_args)
        .args(["--nogui", "--universe"])
        .arg(&universe)
        .args(["--world", &level]);
    let mut child = cmd
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    let deadline = Instant::now() + Duration::from_secs(600);
    while Instant::now() < deadline {
        if horizon_client::control::request(&socket, "world")
            .is_ok_and(|r| r.contains("\"port\":") && !r.contains("\"port\":null"))
        {
            return Ok(Server {
                socket,
                port,
                log,
                pid: child.id(),
            });
        }
        if let Ok(Some(status)) = child.try_wait() {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = text.lines().rev().take(15).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(format!(
                "horizon-server exited ({status}) before listening:\n{}",
                tail.join("\n")
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    Err("horizon-server did not listen within 600 seconds".into())
}

pub fn launcher_home() -> PathBuf {
    match std::env::var_os("HORIZON_LAUNCHER_HOME").filter(|h| !h.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => data_dir().join("horizon-launcher"),
    }
}
/// `v2026.10.13.1` gives [2026, 10, 13, 1]; a pre-release suffix is ignored.
pub fn version_parts(tag: &str) -> Vec<u64> {
    tag.trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Installed launcher versions, newest first.
pub fn installed(versions: &Path) -> Vec<String> {
    let mut tags: Vec<String> = std::fs::read_dir(versions)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join(".installed").exists() && e.path().join(CLIENT).exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    tags.sort_by_key(|t| std::cmp::Reverse(version_parts(t)));
    tags
}

/// A folder's prepared resources, as the launcher prepares them.
pub fn prepared_resources(dir: &Path) -> Option<PathBuf> {
    let version = std::fs::read_to_string(dir.join("research-lock.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["minecraft"]["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| horizon_core::VANILLA_VERSION.into());
    let resources = dir.join(".local/horizon-client").join(version);
    resources
        .join("horizon-resources.json")
        .exists()
        .then_some(resources)
}

#[derive(Debug, PartialEq)]
pub struct Install {
    pub game: PathBuf,
    /// Passed as --resources unless the caller gave their own.
    pub resources: Option<PathBuf>,
    /// The working directory (the game home for prepared installs).
    pub cwd: Option<PathBuf>,
    pub source: &'static str,
    pub version: Option<String>,
}

/// Which game to start: an explicit binary, the one beside horizon-ctl, or a
/// launcher install.
pub fn find(
    game: Option<&Path>,
    version: Option<&str>,
    beside: Option<&Path>,
    launcher: &Path,
) -> Result<Install, String> {
    let prepared = |dir: &Path, source, version| {
        let resources = prepared_resources(dir);
        Install {
            game: dir.join(CLIENT),
            cwd: resources.as_ref().map(|_| game_home()),
            resources,
            source,
            version,
        }
    };
    if let Some(game) = game {
        if !game.is_file() {
            return Err(format!("{} is not a file", game.display()));
        }
        let dir = game.parent().unwrap_or(Path::new("."));
        let mut install = prepared(dir, "--game", None);
        install.game = game.to_owned();
        return Ok(install);
    }
    let versions = launcher.join("versions");
    if version.is_none()
        && let Some(dir) = beside
        && dir.join(CLIENT).is_file()
    {
        return Ok(prepared(dir, "beside horizon-ctl", None));
    }
    let tags = installed(&versions);
    let tag = match version {
        Some(v) => {
            let want = if v.starts_with('v') { v.to_owned() } else { format!("v{v}") };
            tags.iter()
                .find(|t| **t == want || **t == v)
                .cloned()
                .ok_or_else(|| format!("version {v} is not installed in {} (installed: {tags:?})", versions.display()))?
        }
        None => tags.first().cloned().ok_or_else(|| {
            format!(
                "no Horizon found: no horizon-client beside horizon-ctl and no version installed by the launcher in {}. Install one with the launcher (horizon-launcher install latest) or pass --game PATH.",
                versions.display()
            )
        })?,
    };
    let dir = versions.join(&tag);
    let install = prepared(&dir, "launcher", Some(tag.clone()));
    if install.resources.is_none() {
        return Err(format!(
            "{tag} is installed but not prepared; prepare it in the launcher first (horizon-launcher prepare {tag})"
        ));
    }
    Ok(install)
}

pub struct Options {
    pub game: Option<PathBuf>,
    pub version: Option<String>,
    pub wait: bool,
    pub game_args: Vec<String>,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        game: None,
        version: None,
        wait: false,
        game_args: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let value = |i: usize| {
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| format!("{} needs a value", args[i]))
        };
        match args[i].as_str() {
            "--game" => {
                o.game = Some(value(i)?.into());
                i += 1;
            }
            "--version" => {
                o.version = Some(value(i)?);
                i += 1;
            }
            "--wait" | "--wait-ready" => o.wait = true,
            other => o.game_args.push(other.to_owned()),
        }
        i += 1;
    }
    Ok(o)
}

/// Start the game and wait for its control endpoint; Ok(reply) or Err(reply).
pub fn launch(socket: &Path, args: &[String]) -> Result<Value, Value> {
    let o = parse(args).map_err(|e| json!({ "error": e, "kind": "usage" }))?;
    if horizon_client::control::connect(socket).is_ok() {
        return Err(json!({
            "error": format!("a game is already listening on {}; quit it first or pass another --socket", socket.display()),
            "kind": "connection",
        }));
    }
    let exe = std::env::current_exe().ok();
    let beside = exe.as_deref().and_then(Path::parent);
    let install = find(
        o.game.as_deref(),
        o.version.as_deref(),
        beside,
        &launcher_home(),
    )
    .map_err(|e| json!({ "error": e, "kind": "launch" }))?;
    let launch_error = |e: String| json!({ "error": e, "kind": "launch" });
    let mut game_args = o.game_args.clone();
    let world =
        WorldLaunch::take(&mut game_args).map_err(|e| json!({ "error": e, "kind": "usage" }))?;
    let server = match &world {
        Some(world) => {
            let home = match &install.cwd {
                Some(cwd) => cwd.clone(),
                None => std::env::current_dir().map_err(|e| launch_error(e.to_string()))?,
            };
            let player = match arg_value(&game_args, "--name") {
                Some(name) => name.to_owned(),
                None => match std::env::var("HORIZON_PLAYER_NAME")
                    .ok()
                    .filter(|n| !n.is_empty())
                {
                    Some(name) => name,
                    None => horizon_client::offline_name::load_or_create(&home),
                },
            };
            let resources = arg_value(&game_args, "--resources")
                .map(PathBuf::from)
                .or_else(|| install.resources.clone())
                // The client's default, relative to the folder it runs in.
                .or_else(|| {
                    let local =
                        Path::new(".local/horizon-client").join(horizon_core::VANILLA_VERSION);
                    home.join(&local).is_dir().then_some(local)
                })
                .map(|r| if r.is_absolute() { r } else { home.join(r) });
            let distance = arg_value(&game_args, "--render-distance")
                .or_else(|| arg_value(&game_args, "--radius"))
                .unwrap_or("12")
                .to_owned();
            let server = start_server(
                &install.game,
                &home,
                resources.as_deref(),
                socket,
                world,
                &player,
                &distance,
            )
            .map_err(launch_error)?;
            if arg_value(&game_args, "--name").is_none() {
                game_args.extend(["--name".to_owned(), player]);
            }
            game_args.extend([
                "--server".to_owned(),
                format!("127.0.0.1:{}", server.port),
                "--server-control".to_owned(),
                server.socket.to_string_lossy().into_owned(),
            ]);
            Some(server)
        }
        None => None,
    };
    let log = socket.with_extension("log");
    let file = std::fs::File::create(&log)
        .map_err(|e| json!({ "error": format!("{}: {e}", log.display()), "kind": "launch" }))?;
    let mut cmd = Command::new(&install.game);
    let gave_resources = game_args.iter().any(|a| a == "--resources");
    if let Some(resources) = install.resources.as_ref().filter(|_| !gave_resources) {
        cmd.arg("--resources").arg(resources);
    }
    if let Some(cwd) = &install.cwd {
        let _ = std::fs::create_dir_all(cwd);
        cmd.current_dir(cwd);
    }
    let stderr = file
        .try_clone()
        .map_err(|e| json!({ "error": e.to_string(), "kind": "launch" }))?;
    let mut child = cmd
        .args(&game_args)
        .arg("--control-socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(stderr)
        .spawn()
        .map_err(
            |e| json!({ "error": format!("{}: {e}", install.game.display()), "kind": "launch" }),
        )?;
    if let Some(server) = &server {
        // The server lives as long as the client.
        let _ =
            horizon_client::control::request(&server.socket, &format!("exit-with {}", child.id()));
    }
    let deadline = Instant::now() + Duration::from_secs(300);
    let info = json!({
        "server": server.as_ref().map(|s| json!({
            "port": s.port,
            "socket": s.socket,
            "log": s.log,
            "pid": s.pid,
        })),
        "pid": child.id(),
        "socket": socket,
        "log": log,
        "game": install.game,
        "source": install.source,
        "version": install.version,
        "resources": install.resources,
        "home": install.cwd,
    });
    while Instant::now() < deadline {
        if horizon_client::control::connect(socket).is_ok() {
            let mut reply = json!({ "ok": true });
            reply
                .as_object_mut()
                .unwrap()
                .extend(info.as_object().unwrap().clone());
            return Ok(reply);
        }
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = tail.lines().rev().take(15).collect();
            return Err(json!({
                "error": format!("the game exited ({status}) before opening the control socket"),
                "kind": "launch",
                "log_tail": tail.into_iter().rev().collect::<Vec<_>>(),
                "log": log,
            }));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(
        json!({ "error": "control socket did not open within 300 seconds", "kind": "launch", "log": log }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-ctl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn fake_version(versions: &Path, tag: &str, prepared: bool) -> PathBuf {
        let dir = versions.join(tag);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CLIENT), "").unwrap();
        std::fs::write(dir.join(".installed"), "").unwrap();
        std::fs::write(
            dir.join("research-lock.json"),
            r#"{"minecraft":{"version":"26.3"}}"#,
        )
        .unwrap();
        if prepared {
            let res = dir.join(".local/horizon-client/26.3");
            std::fs::create_dir_all(&res).unwrap();
            std::fs::write(res.join("horizon-resources.json"), "{}").unwrap();
        }
        dir
    }

    #[test]
    fn versions_sort_newest_first() {
        let home = temp("sort");
        let versions = home.join("versions");
        for tag in ["v2026.9.30", "v2026.10.13", "v2026.10.13.1", "v2026.10.2"] {
            fake_version(&versions, tag, true);
        }
        assert_eq!(
            installed(&versions),
            ["v2026.10.13.1", "v2026.10.13", "v2026.10.2", "v2026.9.30"]
        );
        let install = find(None, None, None, &home).unwrap();
        assert_eq!(install.version.as_deref(), Some("v2026.10.13.1"));
        assert!(
            install
                .resources
                .unwrap()
                .ends_with(".local/horizon-client/26.3")
        );
        assert_eq!(install.cwd, Some(game_home()));
        let picked = find(None, Some("2026.10.2"), None, &home).unwrap();
        assert_eq!(picked.version.as_deref(), Some("v2026.10.2"));
        assert!(find(None, Some("v1.0"), None, &home).is_err());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_game_beside_ctl_wins_and_dev_builds_keep_the_directory() {
        let home = temp("beside");
        fake_version(&home.join("versions"), "v2026.10.13", true);
        let build = temp("build");
        std::fs::write(build.join(CLIENT), "").unwrap();
        let install = find(None, None, Some(&build), &home).unwrap();
        assert_eq!(install.source, "beside horizon-ctl");
        assert_eq!((install.resources, install.cwd), (None, None));
        // --version always means a launcher install.
        assert_eq!(
            find(None, Some("v2026.10.13"), Some(&build), &home)
                .unwrap()
                .source,
            "launcher"
        );
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(build);
    }

    #[test]
    fn unprepared_or_missing_installs_explain_themselves() {
        let home = temp("unprepared");
        assert!(
            find(None, None, None, &home)
                .unwrap_err()
                .contains("no Horizon found")
        );
        fake_version(&home.join("versions"), "v2026.10.13", false);
        assert!(
            find(None, None, None, &home)
                .unwrap_err()
                .contains("not prepared")
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn launch_options_pass_the_rest_to_the_game() {
        let args: Vec<String> = ["--world", "A", "--version", "v1", "--wait", "--seed", "3"]
            .map(String::from)
            .to_vec();
        let o = parse(&args).unwrap();
        assert_eq!(o.version.as_deref(), Some("v1"));
        assert!(o.wait);
        assert_eq!(o.game_args, ["--world", "A", "--seed", "3"]);
        assert!(parse(&["--game".to_owned()]).is_err());
        let mut game = o.game_args.clone();
        let world = WorldLaunch::take(&mut game).unwrap().unwrap();
        assert_eq!(world.world, "A");
        assert_eq!(world.seed.as_deref(), Some("3"));
        assert!(game.is_empty());
        let mut joining = vec!["--server".to_owned(), "h:1".to_owned()];
        assert_eq!(WorldLaunch::take(&mut joining).unwrap(), None);
        assert!(WorldLaunch::take(&mut vec!["--creative".to_owned()]).is_err());
        let props = world.properties(25599, "A", "6");
        assert!(props.contains("level-seed=3\n") && props.contains("view-distance=6\n"));
        assert!(props.contains("online-mode=false\n") && props.contains("gamemode=survival\n"));
    }
}
