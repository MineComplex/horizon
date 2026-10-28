# Plugins (horizon-server)

horizon-server's plugin API is Bukkit's shape in Rust: crate `horizon-server-api`. A plugin implements
`Plugin` (`JavaPlugin`), registers listeners, commands and tasks in `on_enable`, and is compiled into
`horizon-server` (Rust has no stable ABI to load a library from `plugins/` at run time; horizon-proxy takes its
plugins the same way). `crates/horizon-plugin-example` is a complete one.

## Writing one

```rust
use horizon_server_api::{Plugin, PluginDescription, Plugins, event::*};

#[derive(Default)]
pub struct Spawn;

impl Plugin for Spawn {
    fn description(&self) -> PluginDescription {
        PluginDescription::new("Spawn", "1.0").author("me")
    }
    fn on_enable(&mut self, plugins: &mut Plugins) {
        // @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
        plugins.listen(EventPriority::High, true, |e: &mut BlockBreakEvent, server| {
            if e.pos.x.abs() < 16 && e.pos.z.abs() < 16 && !e.player.is_op() {
                e.cancelled = true;
                server.send_message(&e.player, "§cThe spawn is protected.");
            }
        });
        // plugin.yml commands + CommandExecutor + TabCompleter
        plugins
            .command("spawn", "Back to the spawn", None, |ctx| {
                if let Some(p) = ctx.sender.player().cloned() {
                    ctx.server.teleport(&p, horizon_server_api::Location::new("minecraft:overworld", 0.5, 80.0, 0.5));
                }
                true
            })
            .alias("s");
        // BukkitScheduler.runTaskTimer
        plugins.run_timer(20, 20 * 60, |server| {
            server.broadcast("§7One more minute.");
            true // false stops the timer
        });
    }
}
```

Then add the crate to `crates/horizon-server/Cargo.toml` and its type to `crates/horizon-server/src/plugins.rs`,
and build the server.

## How it runs

- **Load order**: `on_load` before the worlds, `on_enable` once they are loaded and the server listens, before
  `Done`; each plugin after what it `depend`s on (one with a missing dependency is not loaded). `on_disable` as
  the server stops, before the players are saved. The log says `[Name] Loading/Enabling/Disabling Name vX`.
- **Events** run on the server thread in the tick, listeners by priority (`Lowest` .. `Monitor`); with
  `ignore_cancelled` a listener skips events cancelled before it. A panicking listener is logged
  (`Could not pass event ... to ...`) and the others still run.
- **Changes** a listener, command or task asks for through `Server` (messages, broadcasts, kicks, commands it
  dispatches, blocks, teleports, items, game modes) are applied right after it returns, through the server's
  own commands, so they reach every player as any change does. The event's own fields (`cancelled`, messages,
  `drop_items`, `keep_inventory`) decide what the event itself does.
- **Commands**: a plugin's command wins over vanilla's of the same name (`plugin:command` names the plugin);
  `.op(level)` limits it to operators (Bukkit's no-permission message otherwise); an executor returning false
  shows its usage (`<command>` replaced). `/plugins` (`/pl`) lists the plugins.
- **Text** takes `§` colour codes; `colorize` turns `&a` into `§a`.

## Events

| Event | Fires | May change |
| --- | --- | --- |
| `PlayerJoinEvent` | a player is placed in the world | `join_message` (None: none) |
| `PlayerQuitEvent` | a player left | `quit_message` |
| `PlayerChatEvent` (`AsyncPlayerChatEvent`) | a chat message | `message`, `format`, `recipients`, `cancelled` (unchanged, the signed message goes as vanilla sends it) |
| `PlayerCommandPreprocessEvent` | a command a player typed | `message`, `cancelled` |
| `ServerCommandEvent` | a console command | `command`, `cancelled` |
| `BlockBreakEvent` | a player breaks a block | `cancelled` (the block goes back to the player), `drop_items` |
| `BlockPlaceEvent` | a player placed a block | `cancelled` (the old block is put back) |
| `PlayerDeathEvent` | a player dies | `death_message`, `keep_inventory` |

Next: damage (`EntityDamageEvent`), respawn, interact, move and teleport, item pickup and drop, inventories,
and the world's own block changes (pistons, growth, fluids): the server's hook points for them are listed in
git history before commit `a06a6484f` (the WebAssembly plugins), `game/plugin_events.rs`.

## Tests

`cargo test -p horizon-server-api` (the manager: load order, priorities, cancellation, commands, timers,
panics) and `python3 tools/test_plugins_e2e.py` (a horizon-server built with the example plugin and a
horizon-client on it: join, commands, `/plugins`, chat, a cancelled break, enable and disable in the log).
