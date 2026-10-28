# Running a server

A Horizon server is the game without a window, run as one process the way Paper or Leaf run: settings in
`server.properties`, a console on standard input, and logs in `logs/`. There is no web panel, control API or
supervisor; use the console, the logs, and the tools a host already has (systemd, tmux, Docker).

```sh
# In the server's folder (server.properties, eula.txt, the world, plugins/):
horizon-client --resources /path/to/prepared/26.3 --dedicated --nogui
```

- **Console**: type commands on standard input (`stop`, `say`, `op`, `whitelist`, `plugins`...), as on a vanilla
  server; output goes to standard output.
- **Logs**: `logs/latest.log`, rotated to `logs/<date>-<n>.log.gz` at the next start, as vanilla's log4j does.
- **Crashes**: `crash-reports/`.
- **Settings**: `server.properties`, `ops.json`, `whitelist.json`, `banned-*.json`, `usercache.json`
  (`horizon-server-files`).
- **Behind Velocity**: `config/paper-global.yml` (written on the first start, as Paper does), `proxies.velocity`:
  `enabled: true` and `secret:` the proxy's forwarding secret. The server then asks the proxy for each player's
  address and profile on `velocity:player_info` (modern forwarding) and refuses connections that don't come
  through it. Without it the server is offline: names give offline UUIDs.
- **No Mojang**: the server never asks Mojang's services (no `online-mode` session checks, no profile lookups for
  `op`/`whitelist`/`ban`, no chat-signing keys): the proxy authenticates players. `online-mode` in
  `server.properties` is read and ignored.
- **Release build**: `cargo run -p xtask -- package-server ENGINE OUTDIR` packs the engine (built with
  `--no-default-features --features mods`: no sound or controllers) as `horizon-engine`, with `horizon-prepare`.

The server half of the game is moving into its own crate (docs/crates.md); the dedicated binary will come from
there, without the client's window code.
