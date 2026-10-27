# Client, game, server, proxy

Horizon is being split into crates with one job each. The goal: the client is the game with a window
(a copy of Minecraft), the server runs worlds headless with a Bukkit-like plugin API, and the proxy is a
Velocity analog.

| Crate | Job |
|---|---|
| `horizon-game` | The game without a window: chunks, world, streaming and generation binding, blocks, items, menus, mobs, motion, saves, the LAN/server side (`lan/`), the world's rules and clock (`world_data`). No wgpu, winit, audio or controllers |
| `horizon-client` | Window, rendering, section meshing (`streaming::WorkerContext`), audio, input, screens; re-exports every `horizon-game` module under its old path |
| `horizon-proxy` | Velocity in Rust ([proxy.md](proxy.md)) |
| `horizon-server` | Today: `server.toml` and the supervisor that runs the game headless ([hosting.md](hosting.md)) |

## Done

1. `horizon-game` holds the lib modules that need no window (moved with `git mv`; `horizon_client::X`
   paths still work).
2. Section meshing is pluggable: `horizon_game::streaming::Streaming<C: MeshContext>`; the client's
   `WorkerContext` builds meshes from block models. Block models, sprites and the mesher are client-only.
3. `Game::world_data` (`horizon_game::world_data::WorldData`): game rules, difficulty, clock, time markers,
   timelines and the command registries, out of the window's `UiState`.
4. `Game::front` (`Frontend` in game.rs): the 84 fields only a window uses (window, renderer, input,
   fog, LOD, camera, sounds, HUD widgets) in one struct, apart from the simulation's.

## Next

5. Move the server half of `Game` and its `impl` blocks into a crate without the window: the methods that
   touch `self.front` stay in the client, and the calls server code makes into it (sounds, particles,
   screens) go through a trait.
6. A dedicated server binary built from that crate alone (today `horizon-client --dedicated`).
7. `horizon-server-api`: the Bukkit-like API as native Rust plugins (traits, dynamic libraries), the way
   `horizon-proxy` loads its plugins.
