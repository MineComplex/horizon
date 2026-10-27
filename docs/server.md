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
- **Release build**: `cargo run -p xtask -- package-server ENGINE OUTDIR` packs the engine (built with
  `--no-default-features --features mods`: no sound or controllers) as `horizon-engine`, with `horizon-prepare`.

The server half of the game is moving into its own crate (docs/crates.md); the dedicated binary will come from
there, without the client's window code.
