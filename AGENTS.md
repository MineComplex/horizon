# Horizon: a map for agents

Minecraft: Java Edition 26.3 rewritten in Rust (wgpu). Vanilla is the specification wherever it has the
feature. Read [CONTRIBUTING.md](CONTRIBUTING.md) once; this page tells you where things are.
Feature → files → harness for the most-changed areas: [docs/architecture.md](docs/architecture.md).

## Rules that bite

- **Never commit Mojang's files**: game jars, assets, decompiled source, prepared resources (`.local/`).
  `tools/check_public_tree.py` rejects them; `git config core.hooksPath .githooks` runs it on commit.
- **Keep your runs away from real player data.** Any game or launcher you start gets fresh `mktemp -d`
  folders in `HORIZON_HOME` (the game's data), `HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME` and
  `XDG_CONFIG_HOME`, plus `HORIZON_LAUNCHER_NO_SELF_UPDATE=1` and `HORIZON_NO_AUDIO=1`, and, for the
  launcher, `HORIZON_LAUNCHER_TRASH`, `HORIZON_LAUNCHER_CLIPBOARD` and `HORIZON_LAUNCHER_DESKTOP` pointing into the scratch home. Never write to
  `~/.local/share/{applications,horizon,horizon-launcher}` or `~/.local/bin`.
- **Mark what an agent posts.** A PR body starts with a line naming whose agent opened it and with what.
  Ludovic's agents write `> 🤖 Opened by Ludovic's AI agent (Claude), not by Ludovic himself.`, and a
  contributor's agent names its own person and tool (issues: "Filed by", reviews: "Review by"). Every
  comment starts with `🤖 Agent:`. Never write as Ludovic or as anyone else.
- **Attribution.** Commits end with a `Co-Authored-By:` line for the agent that wrote them, and PR bodies
  with that agent's own "Generated with" line. Ludovic's agents use
  `Co-Authored-By: Claude <model> <noreply@anthropic.com>` and
  `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- Everything players run is Rust. Python/Java only for dev tools and harness drivers in `tools/`.
- Horizon-only features follow [docs/design-system.md](docs/design-system.md) and are off by default where
  they would change the vanilla look.
- Small PRs (< ~500 lines). Labels: `feature` (players get something new), `skip-release` (they see no
  change), none for a fix. Agents never add `release-now`: the maintainer decides it. Merges ship in batches of 5, or after
  6 hours ([docs/release-flow.md](docs/release-flow.md)).
- Contributors first: `gh pr list --state open --search <area>` before you start; never duplicate theirs.

## Crates

Game workspace (root `Cargo.toml`). This fork has no launcher (MineComplex's own lives elsewhere), telemetry, Play with Friends, modpack porting or WebAssembly mods (mods are made in the game itself).

| Crate | Owns |
|---|---|
| `horizon-client` | The client: a lib (`src/lib.rs`: window, rendering, audio, input, screens, ~100 flat modules, the `horizon-game` modules re-exported, and `game` = `game.rs` + `game/`, the client's game, which only follows a server) and the bin (`src/main.rs`), plus `horizon-ctl` (`src/bin/horizon-ctl/`) |
| `horizon-game` | The game without a window: chunks, world, streaming and generation binding, blocks, items, menus, mobs, motion, saves, what both sides put on the wire (`item_sync`, `attribute_sync`, `entity_tracking`, `wire`). No wgpu, winit, audio or controllers. `horizon-client`'s lib re-exports every module under its old path (`horizon_client::mobs`...). Streaming is generic over `streaming::MeshContext`: the client's `WorkerContext` (`horizon-client/src/streaming.rs`) builds section meshes; mob and particle vertices are still built here on the CPU |
| `horizon-server-core` | The server's half of the game, which the client never links: the network side and its players (`server_net/`: connections, the play-state hub, `ServerPlayer`, tracking, commands' tree), world generation binding (`vanillagen/`, `flatgen`), raids and patrols, `/scoreboard`, `/data`, datapacks, pregeneration. Re-exports all of `horizon-game`, so its modules keep `crate::` paths |
| `horizon-items` | Blocks and items below gameplay: `blocks.rs` (block facts), `items.rs`, `item_data.rs` (components), `enchant.rs`, `cooldowns.rs`, `bundle.rs`, `item_nbt.rs`. Re-exported as `horizon_client::items`... |
| `horizon-command` | Command parsing (Brigadier): the vanilla tree, argument types, selectors, suggestions, the tree's network form. Re-exported as `horizon_client::command` |
| `horizon-client-core` | The client lib's bottom layer: `text.rs`, `lang.rs`, `snbt.rs`, `nbt_path.rs`, `component.rs`, `scoreboard.rs`, `bossbar.rs`, `gamerules.rs`, `block_entity_nbt.rs`, registry/particle/sound tables. Re-exported under the old paths (`horizon_client::text`...) |
| `horizon-client-input` | Key bindings and key names, toggle keys, mouse options, controller settings and glyphs, the clipboard, window options (`keybinds.rs`, `gamepad.rs`, `mouse.rs`, `window.rs`...). Re-exported under the old paths |
| `horizon-world` | Vanilla world data with no gameplay loop: NBT, regions, chunks, `worldgen/`, `level/` (server-side block behaviour, ticks, redstone), entity physics, items, effects, food, damage |
| `horizon-core` | Simulation primitives: Java `Random`s, `jmath` (HotSpot-exact `Math`), positions, noise, density, paletted storage, `version.rs` (the pinned version) |
| `horizon-protocol` | Network protocol 777: codecs, framing, encryption, `packets/{handshake,status,login,configuration,play}.rs`, `entity_data.rs` |
| `horizon-assets` | Resource decoding: block-state and item models, baking, atlases |
| `horizon-prepare` | Builds `.local/horizon-client/26.3` from the player's own Minecraft install (runs the Java exporters) |
| `horizon-server` | The server program, Leaf-style (`horizon-server --nogui`): server.properties, `config/paper-global.yml` (Velocity forwarding), console, `logs/` ([docs/server.md](docs/server.md)). Its own copy of the game (`src/game.rs` + `src/game/`): the world, simulation, players, commands, saving, plugins |
| `horizon-server-files` | `server.properties`, eula, ops/whitelist/bans, usercache |
| `horizon-server-api` | horizon-server's plugin API, Bukkit-shaped: `Plugin`, events with priorities and cancellation, commands, scheduler ([docs/plugins.md](docs/plugins.md)); the game fires it in `game/plugin_host.rs`. `horizon-plugin-example` is a plugin |
| `horizon-proxy` | MineComplex's Velocity fork in Rust on tokio: `velocity.toml`, launcher-token login, modern forwarding, server switching, plugins, the Network plugin on Core gRPC ([docs/proxy.md](docs/proxy.md)) |
| `horizon-shader-api` | Horizon's own WGSL shader packs ([docs/api/shaders.md](docs/api/shaders.md)): `horizon-shaders.toml`, the `.hshader` package; shared by the game, the launcher and the registry |
| `horizon-shaders` | Shader packs (the `shaders/` folder format), rebuilt clean-room from a written spec of the format; no wgpu |
| `horizon-theme` | Design tokens and fonts for the launcher, the game's own menus and the website |
| `horizon-bench` | The benchmark report format and the machine probes |
| `horizon-dualsense`, `horizon-threads`, `horizon-release-sign`, `horizon-bots`, `xtask` | DualSense output, thread priorities, signed release checksums, load-test bots, repo tasks |

Other top-level folders: `tools/` (harnesses and dev scripts), `tests/` (Python unit tests: `python3 -m
unittest discover -s tests`), `mods/horizon/src/test/java/dev/horizonlab/parity/` (Java oracles),
`docs/`, `.github/workflows/`. The website (playhorizon.dev) is the private horizon-rust/horizon-site: its build
reads `README.md`, `launcher/links.json`, `docs/api/`, `roadmap/roadmap.json` and `crates/horizon-theme/tokens.css` from main.

## Build and test fast

```sh
python3 tools/pr_ready.py                  # the self-check: fmt, clippy -D warnings, touched crates' tests, harnesses; paste its block in the PR
python3 tools/pr_ready.py --quick          # same without harnesses
cargo test -p horizon-client --lib --bins --tests   # skips the 104 examples (pr_ready does this); `--doc` separately
cargo build --profile fast -p horizon-client        # optimized, quick to link: use it to play or run lan_e2e
python3 tools/parity.py --changed origin/main       # the harnesses your branch can affect (CI's parity job)
python3 tools/parity.py --changed-files paths.txt --list   # which harnesses cover these files, without running them
python3 tools/parity.py mob_ai sculk       # named harnesses
python3 tools/lan_e2e.py --check chest     # two real clients on one horizon-server
python3 tools/agent_control_e2e.py         # horizon-ctl / MCP end to end
```

- Harnesses and the game need the vanilla data in `.local/` (`minecraft/`, `horizon-client/26.3`, `datagen/`,
  `tools/`): see [docs/parity-workflow.md](docs/parity-workflow.md). Without it, pr_ready lists the
  harnesses for CI.
- A harness = `tools/test_<name>_parity.py` (324 of them) + a Java oracle (`mods/horizon/src/test/java/dev/horizonlab/parity/Vanilla*Oracle.java`)
  + a Rust probe: `crates/<crate>/examples/<name>_{probe,fixture}.rs` (horizon-world 175, horizon-client 104),
  or a program with a fixture env var: `horizon-server` for `game/dispense_fixture.rs`, `game/sculk_fixture.rs`
  and `game/command_harness.rs` (with a headless `horizon-client`, `HORIZON_STATIONARY_CLIENT`), `horizon-client`
  for `game/net_harness.rs`. Copy a nearby harness for a new one.
- Probes build into `.local/parity-target/`; harnesses find binaries via `lab.cargo_target()`, never `target/`.
- What a change reruns (of 329 harnesses, measured 2026-10-23): a file of the game binary (`game.rs`, `game/`)
  ~25, a file of the client lib ~103, of `horizon-world` ~103–109, of `horizon-core` ~119. Plan for it.
- Rust integration tests: `crates/horizon-client/tests/` (`generation_saves.rs`, `streaming_meshes.rs`), `crates/horizon-server-core/tests/server_bot.rs`.
- Linux needs `libasound2-dev libudev-dev`; `--no-default-features` drops audio and controllers (the server build).
- Two programs: `horizon-client` (only joins servers: no single player, no LAN, no world of its own) and
  `horizon-server`. Tests run both (docs/crates.md).
- Run and drive the game (`launch --world` starts a `horizon-server` beside `horizon-ctl` on the world, then the
  `horizon-client`, which joins it as an operator; other arguments go to the client):
  `target/fast/horizon-ctl --socket $PWD/ctl.sock launch --world W --creative --offscreen --wait --resources $PWD/.local/horizon-client/26.3`,
  then `horizon-ctl --socket … state|keys|turn|command|screenshot|quit`
  ([docs/horizon-ctl-commands.md](docs/horizon-ctl-commands.md)).

## Vanilla reference

- Decompiled sources: `.local/minecraft/26.3/sources/{client,server}/` (`python3 tools/lab.py decompile`).
- Bytecode queries (callers through lambdas, every overload):
  `python3 tools/rea_vanilla.py method net.minecraft.world.level.block.DispenserBlock dispenseFrom`
  ([docs/decompilation-status.md](docs/decompilation-status.md)).
- A harness runs the oracle against the real 26.3 jars and the Rust probe on the same inputs, then compares
  their JSON. Oracle runs are cached by `tools/parity_shim.py`; `--fresh` reruns them.
- Mirror vanilla's `Math` calls with `horizon_core::jmath`, and vanilla's RNG with `horizon_core::random`:
  random draws must happen in vanilla's order.

## horizon-client: where things live

`L:` = lib: `crates/horizon-game/src/` for gameplay shared by both (chunks, world, items, mobs, motion, save...), `crates/horizon-server-core/src/` for the server's alone (server_net, vanillagen...), `crates/horizon-client/src/` for the window, rendering, audio, input and screens (it re-exports the game modules under the same names); `G:` = the client's game (`crates/horizon-client/src/game/`); `S:` = the server's own copy of the game (`crates/horizon-server/src/game.rs` + `game/`: the world, its simulation, players, commands, saving). The lib can't see
`Game`; bin code reaches the lib as `horizon_client::<module>`.

| Subsystem | Files |
|---|---|
| Game loop, tick, frame | `game.rs`: `fn run`, `impl ApplicationHandler for Game` (`window_event`, `about_to_wait`), `fn tick`, `fn tick_world`, `fn render`; L: `clock.rs`, `spans.rs`; `horizon-client-input/src/window.rs` |
| Rendering | L: `gfx.rs` (`Renderer::new`, `Renderer::render`), `gfx/` (cull, occlusion, translucency, upload, timing, weather, stars, end_sky), `*.wgsl`, `particles.rs`, `block_entity_render.rs`, `item_render.rs`, `fog.rs`, `clouds.rs`, `camera.rs`, `first_person.rs`; G: `render_tick.rs`, `item_draw.rs` |
| Chunks, streaming, light | L: `streaming.rs` (game: worker pool generate, light, mesh jobs; client: `WorkerContext`, the mesh job), `world.rs`, `chunk.rs`, `chunk_map.rs`, `light.rs`, `mesher.rs` + `mesher/`, `vanillagen.rs` (binds horizon-world's generator), `ticket_levels.rs`; G: game.rs `fn update_world`; S: `world_loading.rs`, `postprocess.rs`, game.rs `fn stream_world` |
| World generation | `crates/horizon-world/src/worldgen/` (`generator*.rs`, `feature_graph.rs`, `structure/`, `carver/`) |
| Block behaviour | `crates/horizon-world/src/level/` (`mod.rs`, `tick.rs`, `hooks.rs`, `redstone.rs`, `place.rs`, `interact.rs`); S: game.rs `fn tick_blocks`, `fn process_level_events` |
| Mobs | L: `mobs.rs` (types), `mobs/` (`ai.rs`, `goals.rs`, `brain.rs`, `*_brain.rs`, `nav.rs`, `path.rs`, `species.rs`, `spawning.rs`, `render.rs`, `anim.rs`); S: `creatures.rs` (ticking, spawning, mob events), one file per mob family (`dragon.rs`, `wither.rs`, `villagers.rs`, `piglins.rs`, `golems.rs`...), `natural_spawning.rs`; G: `creatures.rs` (rendering, the server's mob events' particles), `remote_mob_data.rs` |
| Other entities | L: `entities.rs`, `projectiles.rs`, `arrows.rs`; G and S: `boats.rs`, `minecarts.rs`, `mounts.rs`, `armor_stands.rs`, `paintings.rs`, `frames.rs`, `fireworks.rs`, `tnt.rs`; S: `explosion.rs` |
| Player actions | S: game.rs (`fn use_hand`, `fn use_item`, `fn tick_mining`, `fn break_block`); G: remote.rs (`fn remote_mining`, `fn predict_break`, `fn remote_use`, `fn remote_start_attack`, `fn end_predictions`: the client predicts, the server decides); G and S: `hand.rs`, `combat.rs`, `consume.rs`, `potions.rs`, `shield.rs`, `crossbow.rs`, `trident.rs`, `fishing.rs`; L: `motion.rs` + `motion/` (movement), `collisions.rs`; crate `horizon-items`: `items.rs`, `item_data.rs`, `enchant.rs`, `blocks.rs` |
| Inventory and containers | L: `menu.rs` (`AbstractContainerMenu.clicked`), `container_screen.rs`, `containers.rs`, `creative.rs`, `crafting.rs`, `recipe_book*.rs`, `stations.rs`, `merchant.rs`; game.rs `fn menu_ops`, `fn click_inventory`; G: `stations.rs`, `trading.rs`, `recipe_book.rs`, `machines.rs` |
| Commands | parse: crate `horizon-command` (`args.rs`, `selector.rs`, `tree_*.rs`, `vanilla.rs`); run (only on the server): S: `commands.rs` (dispatch) + `commands/*.rs` (`admin`, `execute`, `items`, `players`, `world_cmds`...), `functions.rs`, `command_blocks.rs`; L: `scoreboard_cmd.rs`, `data_cmd.rs`; `horizon-client-core`: `nbt_path.rs`, `scoreboard.rs`; G: `chat.rs` sends them, `command_blocks.rs` (the edit screen) |
| UI screens | `Screen` enum in game.rs; L: `title.rs` + `title/`, `options_ui.rs`, `multiplayer.rs`, `pack_screen.rs`, `graphics_ui.rs`, `screen_draw.rs`, `ui.rs` (widgets), `hud.rs`, `hud_layout.rs`, `hud_overlays.rs`, `chat.rs`, `tab_list.rs`; G: `startup_menu.rs`, `screens.rs` (pause, death), `options.rs`, `chat.rs`, `sign_edit.rs`, `books.rs` |
| Multiplayer client | L: `net.rs` (network thread); G: `remote.rs` (`fn handle_packet`, `fn tick_remote`), `remote_*.rs` (mob data, objects, projectiles, vehicles, stations) |
| Server network side, players | `horizon-server-core/src/server_net/` (`conn.rs` handshake + login threads, `hub.rs` + `hub/` play state, `chunks.rs`, `entity.rs`, `player.rs` (`ServerPlayer`), `playerdata.rs`; the tracking table is `horizon-game/src/entity_tracking.rs`); S: `server_net.rs` (`Network`), `net_actions.rs` (`fn as_player`, `fn tick_players`), `net_*.rs` per feature, `net_entities.rs` (tracked entities) |
| Server levels, test control | S: `levels.rs` (one level per dimension, `fn tick_server`), `server_travel.rs`, `server_control.rs` (the control socket's requests), `fixtures.rs`; G: `server_control.rs` (`ask_server`, `server_command`: the client's way to it) |
| Dedicated server | S: `dedicated.rs`, `dedicated/status.rs`, `plugin_host.rs` (plugin events), `crates/horizon-server/src/plugins.rs`; crate `horizon-server-files` ([docs/server.md](docs/server.md)) |
| Save and load | L: `save.rs` (level.dat, regions, playerdata), `entity_nbt.rs`, `options_file.rs`; `horizon-client-core/src/block_entity_nbt.rs`; S: `persist.rs` (entities), game.rs `fn save_game`; `crates/horizon-world/src/{region.rs,nbt.rs}` |
| Input | game.rs `fn window_event`, `fn key_pressed`, `fn play_mouse`; crate `horizon-client-input` (`keybinds.rs`, `keymapping.rs`, `key_names.rs`, `mouse.rs`, `input.rs`, `interaction_input.rs`, `gamepad.rs`, `clipboard.rs`); L: `osk.rs`; G: `pad.rs`, `pad_menu.rs` |
| Audio | L: `audio.rs` (mixer), `sound_engine.rs`, `sound_events.rs`, `music.rs`, `ambience.rs`; G: `music.rs`, `subtitle_hud.rs` |
| Resource packs | L: `packs.rs` + `packs/`, `pack_screen.rs`, `atlas_sources.rs`, `sprites.rs`, `mipmap.rs`, `model_library.rs`, `font.rs`; `horizon-client-core/src/lang.rs`; crate `horizon-assets`; G: `packs.rs` |
| Agent control | L: `control.rs` (socket), `bin/horizon-ctl/`; G: `control.rs`, `agent.rs`, `events.rs`, `ui_control.rs` ([docs/agent-control.md](docs/agent-control.md)) |
| Horizon extras, settings | L: `extras.rs`, `graphics.rs`, `settings.rs`, `minimap.rs`; G: `extras.rs`, `tweaks.rs`, `options.rs` |

## How `Game` is organised

- Two games: the client's `Game` (`crates/horizon-client/src/game.rs` + `game/`) and the server's
  (`crates/horizon-server/src/game.rs` + `game/`, its `Server`). Their methods are spread over many
  files, each `game/<feature>.rs` adding an `impl` block for one feature. Fields are private.
- The client only follows a server: `fn tick` runs `tick_remote()` (game/remote.rs): it predicts the
  player's own movement, mining, placing and menu clicks, and takes the world from the server's packets.
  Drops, loot, commands, statistics, advancements, trades, spawning and saving are the server's.
- Find a feature's code: `grep -rn "fn tick_<thing>\|fn <verb>_<thing>" crates/horizon-{client,server}/src/game*`;
  the file is usually `game/<thing>s.rs`.
- On the server, players run through the gameplay code one at a time: `as_player` swaps the player's
  state (position, inventory, mode, mining, item use) into `Game` for one call (`game/net_actions.rs`), and
  `levels.rs` swaps whole dimensions in the same way. Changes reach clients through the block log, level
  events and the entity tracker (`horizon-server-core/src/server_net/entity.rs`, `game/net_entities.rs`).
- Shared read-only data: `Resources` (`res`: blocks, items, recipes, mob types, biomes) and `PackResources`
  (`pack`), both in game.rs.

## Where a world runs

Worlds run only on `horizon-server` (one level per dimension, its `game/levels.rs`); the client has none of its own
(docs/crates.md). A test's client gets the server's control socket (`--server-control`), through which
`horizon-ctl` asks what only the server knows: `give`, `time`, `mobs`, the block commands, `command`
(`crates/horizon-server/src/game/server_control.rs`).

```sh
target/fast/horizon-ctl --socket $PWD/c.sock launch --world T --resources $PWD/.local/horizon-client/26.3 --wait
python3 tools/lan_e2e.py --check <name>                  # two clients on one horizon-server
python3 tools/parity.py dedicated_server server_entities server_player_damage
```

## Docs worth knowing

[docs/parity-workflow.md](docs/parity-workflow.md) (runner, gate, shards, scopes) ·
[docs/native-client.md](docs/native-client.md) (options, controls, horizon-ctl) ·
[docs/server-levels.md](docs/server-levels.md) (levels per dimension) ·
[docs/rust-parity.md](docs/rust-parity.md) (what is ported) · [docs/known-issues.md](docs/known-issues.md) ·
[docs/release-flow.md](docs/release-flow.md) · [docs/README.md](docs/README.md) (index).
