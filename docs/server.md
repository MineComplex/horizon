# Running a server

A Horizon server is the game without a window, run as one process the way Paper or Leaf run: settings in
`server.properties`, a console on standard input, and logs in `logs/`. There is no web panel, control API or
supervisor; use the console, the logs, and the tools a host already has (systemd, tmux, Docker).

```sh
# In the server's folder (server.properties, eula.txt, config/, the worlds, logs/):
horizon-server --resources /path/to/prepared/26.3 --nogui
# the same game, the old way: horizon-client --resources ... --dedicated --nogui
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
- **Release build**: `cargo run -p xtask -- package-server target/release/horizon-server OUTDIR` packs
  `horizon-server` (no sound or controllers) with `horizon-prepare`.

`horizon-server` is its own crate (`crates/horizon-server`), built without sound or controllers. The server half of
the game still lives in `horizon-client`'s library (`horizon_client::game`) and moves into `horizon-server` step by
step, until the server links no window code (docs/crates.md). Arguments: vanilla's (`--nogui`, `--port`, `--world`,
`--universe`, `--initSettings`), plus `--resources DIR` (or `HORIZON_RESOURCES`) for the prepared 26.3 data,
`resources/` beside the program by default.
