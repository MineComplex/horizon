# horizon-proxy

MineComplex's Velocity fork (`MineComplex/Velocity`, branch 26.3) rewritten in Rust, with the
Network plugin (`Network/Proxy`) built in: crate `crates/horizon-proxy`, binary
`horizon-proxy [velocity.toml] [--network]`.

Like the fork it speaks only the pinned protocol (26.3), forwards players only with Velocity's
modern forwarding (`velocity:player_info`, HMAC-SHA256 over `forwarding.secret`), and lets players
in by the launcher's token instead of Mojang's session service. It runs on tokio: every connection
is a task (the fork runs on Netty).

## Running

```sh
cargo build --release -p horizon-proxy
target/release/horizon-proxy velocity.toml             # writes the default file and forwarding.secret when missing
target/release/horizon-proxy velocity.toml --network   # also writes plugins/network/config.toml: the Network plugin
```

The config is the fork's `velocity.toml` (`[packet-limiter]`, `[servers]`, `[advanced]`; keys the
port doesn't use are ignored), plus `[auth]`:

| Key | Meaning |
|---|---|
| `auth.mode` | `launcher` (default): the token the client sends on `minecomplex:auth`. `offline`: anyone, offline UUIDs (tests only) |
| `auth.token-public-key` | `tokenPublicKey` of Rest's `GET /api/meta` (base64 X.509) |
| `auth.token-issuer`, `auth.token-ttl-minutes` | as `launcher.token-issuer` / `launcher.token-ttl-minutes` of the Network plugin |

`VELOCITY_FORWARDING_SECRET` overrides the secret file. Backends run offline-mode with Velocity
modern forwarding on (Paper: `proxies.velocity`).

Console: `end`, `glist [all]`, `send <player|all|current> <server>`, and the plugins' commands
(`maintenance`, `proxy`).

## The proxy (the fork)

- Status: answered by the proxy (MOTD in MiniMessage), then by the plugins (ProxyPingEvent).
- Login: encryption with `shouldAuthenticate = false`, the token query on `minecomplex:auth`, the
  auth handler (RS256 LauncherToken: `iss`, `iat` within the TTL, `sub`, `sid`), login rate limit,
  duplicate names refused or the old player kicked (`kick-existing-players`).
- Servers: the initial server (PlayerChooseInitialServerEvent), switching through the
  configuration state (StartConfiguration, the client's brand and ClientInformation replayed,
  boss bars cleared), and KickedFromServerEvent with Velocity's results: redirect to the next
  `try` server with "Вы были кикнуты с сервера ...", notify, or disconnect.
- Commands: `/server` (`velocity.command.server` unless false), `/glist` and `/send`
  (`velocity.command.glist` / `.send` true), the plugins' commands; announced in the server's
  command tree (`announce-proxy-commands`) and completed with Tab; `command-rate-limit`,
  `forward-commands-if-rate-limited`, `kick-after-rate-limited-commands`, the same for Tab.
- `[packet-limiter]`: packets, bytes and decompressed bytes per second over a moving window.
- `haproxy-protocol`: the PROXY header (v1 and v2) gives the client's real address.

## Plugins

`plugin::Plugin` is Velocity's event API as a Rust trait compiled into the proxy: `pre_login`,
`login`, `disconnect`, `choose_initial_server`, `server_connected`, `kicked`, `command`,
`command_executed`, `tab_complete`, `ping`, `permission` (the permission provider) and `auth` (the
AuthHandler). Hooks that wait on the network return futures; the rest answer from memory.
`ProxyHandle` gives plugins the players (message, title, kick, connect, profile properties),
`register_command`, `broadcast` and permissions.

## The Network plugin

Enabled by `plugins/network/config.toml` (the keys of the Java plugin's `config.yml`). The proxy
becomes a PROXY node of Core over gRPC (Core's `.proto` files are copied to
`crates/horizon-proxy/proto`; copy them again when Core's protocol changes):

- the session stream (`SessionService.Connect`, `authorization: Bearer <core.token>`,
  `x-session-id`), reconnected with a backoff, with the resync of the players after a reconnection;
- login: the launcher token, then `AccountService.CheckLogin` (the account's name, its skin as the
  `textures` property), a running lobby, permissions (`PermissionService.GetUser`), the ban
  (`PunishmentService.GetActive`; a bought unban is claimed), `PresenceService.Join`;
  `Switch` and `Leave` after;
- maintenance (refuses and kicks addresses off the whitelist), connections per address;
- lobbies: the initial server and the fallback go to the emptiest accessible LOBBY of Core's
  registry, except for the ignored kick reasons;
- the server list: Core's total online, the description lines, `bungee.name`, rotating MOTDs,
  "Тех. работы" during maintenance;
- permissions from Core's groups and the player's nodes (wildcards, inheritance, expiry);
- Core's events: messages, alerts, kicks, moves (`/lobby` with its cooldown), the reload signal,
  maintenance, the server registry, permission groups and users, new skins, password changes and
  social links (the `verified` group);
- blocked commands and the command cooldown, the action log of the proxy's commands;
- announcements (`broadcast.messages`), `/maintenance add|remove|toggle`, `/proxy cpu|ram|network`.

## Not yet

- Sounds of the social link messages (SoundUtil), and display names with group prefixes in ban
  texts (the issuer's account name is shown).
- `/proxy cpu` and `ram` read `/proc`: on other systems they say "н/д".
- Velocity's `/velocity` command, plugin messaging between backends (`bungeecord:main`), and
  plugins loaded at run time (plugins are compiled in).
