# Client, game, server, proxy

Horizon is a Minecraft client, a Minecraft server that players join, and a proxy in front of several
servers. There is no single player, no Open to LAN and no world inside the client.

| Crate | Job |
|---|---|
| `horizon-game` | What the client and the server share: chunks, world, streaming, blocks, items, menus, mobs, motion, saves, what both put on the wire (`item_sync`, `attribute_sync`, `entity_tracking`, `wire`), the world's rules and clock (`world_data`). No wgpu, winit, audio or controllers |
| `horizon-server-core` | The server's half, which the client never links: the network side (`server_net/`), world generation binding (`vanillagen/`, `flatgen`), raids, patrols, server commands, datapacks, pregeneration |
| `horizon-client` | The client: window, rendering, section meshing (`streaming::WorkerContext`), audio, input, screens, and its `Game` (`src/game.rs` + `src/game/`), which only follows a server; re-exports every `horizon-game` module under its old path |
| `horizon-server` | The server, Leaf-style ([server.md](server.md)): its `Server` (`src/game.rs` + `src/game/`) runs the worlds, the players and the plugins |
| `horizon-server-api` | The server's plugin API, Bukkit-shaped ([plugins.md](plugins.md)) |
| `horizon-proxy` | Velocity in Rust ([proxy.md](proxy.md)) |

## The two programs

- `horizon-client` joins servers only. Its `Game` holds the copy of the world the server sends, the
  player it predicts (movement, mining, placing, menu clicks: the server confirms or corrects them), the
  window, input, rendering, sounds and the HUD. Drops, loot, commands, statistics, advancements, trades,
  spawning and saving are the server's. `--world`, `--seed`, `--creative`, `--benchmark` are unknown to it.
- `horizon-server` runs the worlds (one level per dimension, `game/levels.rs`), the players
  (`game/server_net.rs`, `game/net_*.rs`: each player's action runs through the gameplay code with their
  state swapped in, `as_player`), commands, saving and the plugins (`game/plugin_host.rs`). For tests it
  also takes `--control-socket PATH` (`game/server_control.rs`: what a client's `--server-control` and
  `horizon-ctl` ask), `--time`, `--weather`, `--dimension`, `--test-terrain`, and runs the fixtures
  (`HORIZON_DISPENSE_FIXTURE`, `HORIZON_SCULK_FIXTURE`, `HORIZON_COMMAND_SCRIPT` with a headless client,
  `HORIZON_STATIONARY_CLIENT`).

The client's `Game` and the server's `Server` started as one `Game`; each is now its own copy, cut down to
its side.

Tests run the two together: `horizon-ctl launch --world W` writes a scratch `server.properties` (offline,
the player in `ops.json`), starts `horizon-server` on `saves/W`, then the client with `--server` and
`--server-control`; the server stops with the client (`exit-with`). `tools/test_server.py` does the same for
scripts that start the client themselves.

## Next

- Leaf's touches: the `world_nether` / `world_the_end` layout, `/tps`, `/version`.
