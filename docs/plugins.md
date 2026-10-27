# Horizon plugins

Plugins are Horizon's Bukkit: server code that hears what happens on the server, cancels or changes it,
owns commands and permissions, and drives players, worlds, entities, items, inventories, scoreboards and
boss bars. They are written in Rust against the SDK, compiled to WebAssembly and run in the same sandbox as
[mods](mods.md), so one `.hmod` runs on every server, a crash or an endless loop stops only the plugin
that caused it, and a plugin reaches nothing outside the API (no files but its data folder, no sockets).

| Part | Where |
|---|---|
| The contract (WIT) | `crates/horizon-mod-sdk/wit/horizon.wit`, the `plugin-*` interfaces and world `horizon-plugin` |
| SDK | `horizon_mod_sdk::plugin` (`crates/horizon-mod-sdk/src/plugin.rs`) |
| Runtime: listeners, priorities, commands, permissions, tasks, files, plugin calls | `crates/horizon-mods/src/runtime/plugin.rs` |
| The server's side: events, the API on the game | `crates/horizon-client/src/game/plugins.rs`, `plugin_host.rs`, `plugin_events.rs` |
| Test server (no game) | `horizon_mods::plugin_test`, `cargo horizon-mod test` |
| Examples | [`essentials`](../crates/horizon-mod-sdk/examples/essentials/src/lib.rs) (spawn, homes, a menu, a sidebar, protection), [`event-log`](../crates/horizon-mod-sdk/examples/event-log/src/lib.rs) (logs every event, cancels the kinds you name: a debugging aid) |
| Tests | `cargo test -p horizon-mods --features runtime,pack --test plugins` (the examples' scripts and the runtime); `python3 tools/test_plugins_e2e.py` (a real dedicated server, scripted clients: `cargo build -p horizon-bots --example plugin_client`) |

## Installing plugins

Put `.hmod` files in `plugins/` in the server's folder (beside `server.properties`) and start the server.
A plugin there is on while its file is there, as with Bukkit's jars. Each plugin's data folder is
`plugins/<id>/`. Plugins also load from `horizon-mods/` when turned on there (`mod.<id>:true` in
`horizon-mods.txt`), and `--dev-mod <file>` runs one and reloads it when its file changes.

They run where the world is simulated: the dedicated server (`horizon-server`, `--dedicated`), a world
opened to LAN, and single player's own server thread. They are enabled on the first tick after the world
is up, dependencies first, and disabled when the server stops.

Console: `plugins` (or `pl`) lists them, green when enabled; players get the same with `/plugins`.

## Writing a plugin

```sh
cargo horizon-mod new my-plugin --template plugin
cd my-plugin
cargo horizon-mod build      # target/horizon-mod/my_plugin.hmod
cargo horizon-mod test       # tests/*.toml on a test server, no game needed
```

The manifest is the `plugin.yml`:

```toml
id = "essentials"
name = "Essentials"
version = "1.0.0"
api = "0.4"                  # plugins
side = "server"
capabilities = ["plugin"]    # every plugin-* interface; "storage" may be added
depend = ["economy"]         # enabled before this one; without them it stays off
softdepend = ["luckperms"]   # enabled before this one when installed
loadbefore = []              # enabled after this one when installed
prefix = "Ess"               # its name in the log (default: name)

[commands.gamemode]
description = "Changes your game mode"
usage = "/<command> <mode> [player]"   # shown when on_command returns false
aliases = ["gm"]
permission = "essentials.gamemode"
permission-message = "&cYou may not."

[permissions."essentials.gamemode"]
default = "op"                         # true, false, op, not-op
[permissions."essentials.admin"]
children = { "essentials.gamemode" = true }
```

Files under `defaults/` in the plugin's folder go into the `.hmod`; `data::config("config.yml")` writes the
bundled one to the data folder the first time and reads it.

A plugin is a type implementing `Plugin`:

```rust
use horizon_mod_sdk::plugin::*;

struct Hello;
impl Plugin for Hello {
    fn enable() -> Result<Self, String> {
        listen(EventKind::PlayerJoin, Priority::Normal);
        listen_uncancelled(EventKind::BlockBreak, Priority::High);
        timer::<Hello>(20, 20 * 60, |_| server::broadcast("&6Every minute", None));
        Ok(Hello)
    }
    fn on_event(&mut self, _priority: Priority, event: &mut Event) {
        match event {
            Event::PlayerJoin(e) => e.message = Some(format!("§a+ {}", e.player.name)),
            Event::BlockBreak(e) if e.block.state.contains("diamond") => e.cancelled = true,
            _ => {}
        }
    }
    fn on_command(&mut self, sender: &Sender, command: &str, label: &str, args: &[String]) -> bool {
        sender.send(&color("&eHello!"));
        true
    }
}
horizon_mod_sdk::export_plugin!(Hello);
```

### Text

Every text argument takes legacy colour codes (`§a`, or `&a` when the text has no `§`; `color()` turns `&`
into `§`) or a JSON text component when it starts with `{`, `[` or `"`.

### Events

`listen(kind, priority)` registers for an event; `listen_uncancelled` skips those already cancelled
(`ignoreCancelled`). `event.kind()`, `event.is_cancelled()` and `event.set_cancelled(bool)` work on any
event (Bukkit's `Cancellable`). Listeners run lowest first (`lowest`, `low`, `normal`, `high`, `highest`, `monitor`),
plugins in enable order at each priority. A listener changes the event in place; a `monitor` listener's
changes are ignored.

| Event | Fires | Changeable |
|---|---|---|
| `player-login` | a player was placed in the world | `allowed`, `kick-message` (refused: kicked) |
| `player-join` | then | `message` (the join message; none: no message), `first-join` |
| `player-quit` | a player left | `message` |
| `player-chat` | a chat message, before anyone gets it | `message`, `format` (`{name}`, `{message}`), `recipients`, `cancelled`. Unchanged, it goes signed as vanilla sends it |
| `player-command` | a command a player typed, before anything runs it | `line`, `cancelled` |
| `server-command` | a console line | `line`, `cancelled` |
| `player-move` | a player moved during the tick | `to`, `cancelled` (back to `from`) |
| `player-interact` | right click on a block or the air, left click on a block | `cancelled` (nothing happens; the client is corrected) |
| `player-interact-entity` | right click on an entity | `cancelled` |
| `player-toggle-sneak`, `-sprint`, `-flight` | a toggle during the tick | flight: `cancelled` |
| `player-item-held` | another hotbar slot | `cancelled` (the slot goes back) |
| `player-drop-item` | Q | `cancelled` |
| `player-swap-hands` | F | `cancelled` |
| `player-game-mode-change` | the F3+F4 switcher | `cancelled` |
| `player-death` | a player died | `message`, `keep-inventory`, `keep-level` |
| `player-respawn` | a player respawned | `location` |
| `player-changed-world` | a player went to another dimension | |
| `sign-change` | a sign was edited | `lines`, `cancelled` |
| `plugin-message` | a custom payload on a channel the plugin registered | |
| `player-resource-pack` | the client answered a resource pack | |
| `block-break` | a player breaks a block, before it goes | `drop-items`, `cancelled` (the block stays; the client is sent it again) |
| `block-place` | a player placed a block | `cancelled` (the old block is put back, the item is not used) |
| `block-piston-extend`, `block-piston-retract` | a piston is about to push or pull: the piston, its direction, the blocks it moves | `cancelled` (the piston stays as it is) |
| `block-from-to` | water or lava is about to flow into a block (`face`: `down`, `north` ...) | `cancelled` (cancelling the flow down stops it flowing to the sides this tick) |
| `block-grow` | a plant grew in a random tick: crops, stems and their fruit, cactus and its flower, sugar cane, berry bushes, cocoa, nether wart. `block` is the plant as it was; the world already shows `new-state` | `cancelled` (the blocks the growth set are put back) |
| `block-spread` | a block spread from `source`: grass, mycelium, vines, kelp and the other growing plants, mushrooms, fire. `block` as it was | `cancelled` (put back) |
| `block-form` | snow or ice formed in the weather | `cancelled` (put back) |
| `block-fade` | ice melted, coral died, fire went out, grass or farmland turned to dirt | `cancelled` (put back) |
| `block-fertilize` | bone meal is about to be used on a block that takes it (or on the water beside, for water plants), by `player` when a player uses it | `cancelled` (nothing grows, the bone meal stays) |
| `entity-damage` | a player attacks an entity or another player; a player is hurt by a mob, an explosion, a fall, an effect, a firework, or its surroundings (fire, lava, magma, cactus, berry bushes, campfires, freezing, drowning, suffocation, the void, flying into a wall); a mob is hurt by its surroundings (fire, lava, drowning, drying out, suffocation, the void, poison and wither). While a plugin listens, the surroundings' damage lands at the end of the mob's tick or the player's move | `damage`, `cancelled` |
| `entity-death` | a mob or a player died | |
| `entity-spawn` | a mob spawns naturally, from a spawner, a command or egg, a raid, an iron golem built | `cancelled` |
| `entity-explode` | an explosion, before it hurts and breaks | `blocks` (remove some to spare them), `cancelled` |
| `inventory-click` | a click in any menu, before the server applies it | `cancelled` (every slot stays; the client is corrected) |
| `inventory-close` | a menu was closed | |
| `plugin-state` | a plugin was enabled or disabled | |
| `plugin-call` | another plugin's `send-to-plugin` | |

Not yet: redstone (`BlockRedstoneEvent`), blocks burning (`BlockBurnEvent`), leaves decaying (`LeavesDecayEvent`), snow layers melting and the blocks lava and water form where they meet fire no block events; dispensers' bone meal fires no `block-fertilize`.

### Commands and completion

Manifest commands and `commands::register` at run time; a plugin's command takes precedence over vanilla's
of the same name, and `<plugin>:<command>` always reaches it. The sender is `Sender::Console` or
`Sender::Player(player)`; a command's `permission` is checked first (the console has every permission).
Returning `false` shows the `usage` with `<command>` replaced by the label typed. Clients get every
plugin command in their command tree, so the chat colours and completes them: `on_tab_complete` answers
for the last argument; an empty answer completes the names of the players online.

### Permissions

`permissions::has(player, node)`: what any plugin set for the player (`set`, `unset`, `clear`; `*` and
`a.b.*` match the nodes under them, the most specific wins), else the declared node's default, else
operators only (Bukkit's default for an undeclared node). A node set brings its declared children (flipped
when set false). A permissions plugin (LuckPerms-like) sets them on join; they go when the player leaves.

### The scheduler

`later::<P>(delay, |plugin| ...)` and `timer::<P>(delay, period, |plugin| ...)` run a closure with the
plugin's state; `cancel(task)`. Ticks are the server's (20 a second); 0 runs at the end of this tick. There
are no threads: an "async" task is a task.

### What a plugin can do

| Interface | Bukkit | Functions |
|---|---|---|
| `plugin-server` | `Server` | `info`, `tps`, `mspt`, `now-millis`, `random`, `broadcast` (with a permission), `console`, `dispatch-command`, `shutdown`, `set-motd`, `set-max-players`, `plugins`, `plugin-enabled`, `send-to-plugin`, `find-player` (online or the user cache), whitelist, operators, bans, IP bans |
| `plugin-players` | `Player` | the player record (location, mode, health, food, xp, op, address, ping, locale, brand, flying, sneaking ...); messages, action bar, titles, tab list, list and display names, sounds, particles, fake blocks, kick, transfer, resource packs; teleport (across worlds), game mode, health, max health, food, saturation, level and xp, flight, walk and fly speed, op, velocity, damage, kill, fire, effects, hiding players, commands and chat as the player, respawn point; inventory (41 slots), give with leftovers, remove, clear, ender chest, held slot, cursor, close, update |
| `plugin-worlds` | `World` | worlds' info (time, weather, spawn, difficulty, height), blocks and biomes, highest block, block entity data, `set-blocks` (as `/setblock`), chunks loaded and forced, time, weather, spawn, difficulty, game rules, lightning (real or effect only), explosions, dropped items, sounds and particles at a place, the world border, save |
| `plugin-entities` | `Entity` | entities by id, UUID, world, radius and tag; spawn (with NBT, a tag to find it), teleport, remove, velocity, custom name, glowing, invulnerable, silent, gravity, AI, damage, health, tags, passengers, NBT read and merge |
| `plugin-items` | `ItemStack`, `ItemMeta`, `PersistentDataContainer` | parse vanilla item syntax; display name, item name, lore, enchantments, damage, max damage, unbreakable, custom model data, max stack, glint, hidden tooltip; the plugin's own values on a stack (`get-tag`/`set-tag`, in `custom_data.PublicBukkitValues`); similar; registries |
| `plugin-inventories` | custom `Inventory` | chests of 1 to 6 rows, hoppers, droppers: slots, contents, add, title, open, viewers, destroy. They behave as chests; plugins cancel `inventory-click` for menus |
| `plugin-scoreboards` | `Scoreboard`, `Team` | per-player sidebars (15 lines, no numbers), name tag prefix/suffix/colour for everyone; the server's objectives, scores, display slots and teams |
| `plugin-bossbars` | `BossBar` | create, title, progress, colour, style, flags, players, remove |
| `plugin-messaging` | plugin channels | register incoming channels (`bungeecord:main` too), send payloads |
| `plugin-files` | data folder | read, write (atomic), append, delete, exists, list, bundled `defaults/`, `save-default` |
| `plugin-listeners`, `plugin-commands`, `plugin-permissions`, `plugin-scheduler` | | above |

Reads are live: the player, block or entity as it is during the call. Changes a client sees at once
(messages, titles, sounds, boss bars, sidebars, payloads) go out at once; the others (teleports, health,
inventories, blocks, commands) are made at the end of the tick, mostly through vanilla's own commands so
every client sees them as vanilla shows them.

### Limits

Each call has a fuel budget (enable 10 billion instructions, an event 300 million, a command 2 billion, a
task 1 billion) and the plugin 512 MiB of memory. A trap, a panic, running out of fuel or memory turns the
plugin off with its tasks, listeners, commands and permissions; the server and the other plugins go on.

## Testing a plugin

`cargo horizon-mod test` runs `tests/*.toml` against a test server (`horizon_mods::plugin_test`): no game,
the plugin in the same sandbox, everything it does recorded.

```toml
[[step]]
join = ["Steve"]                         # player-login, player-join
expect.announcements = ["+ Steve"]

[[step]]
op = ["Steve"]                           # then as = "Steve" or the first player on
command = "gm creative"
expect.handled = true
expect.told = ["Steve: Set Steve's game mode (gm)."]
expect.player = { name = "Steve", game-mode = "creative" }

[[step]]
break = { x = 3, y = 63, z = 3 }
expect.cancelled = true

[[step]]
command = "menu"
click = { slot = 11 }
expect.inventory = { player = "Steve", closed = true }
```

Steps, in the order a step runs them: `op`, `join`, `move`, `chat`, `command`, `console`, `complete`,
`break`, `place`, `click`, `damage`, `ticks`, `leave`, `stop` (`as` names the player; split a step in two
when the order matters). Expectations: `told`, `told-contains`, `action-bars`, `titles`,
`console`, `dispatched`, `performed`, `chat`, `announcements`, `kicked`, `sounds`, `cancelled`, `handled`,
`completions`, `blocks`, `inventory`, `sidebar`, `player` (position, health, food, mode, flight,
permissions, slots), `bars`, `file`, `stopped`. `with = ["../other.hmod"]` loads other plugins beside it.
`cargo horizon-mod check` gives a plugin a trial run on the same server.
