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

Their fields are in `horizon_server_api::event`; what a listener may change is listed here. Events seen
"once a tick" are found by comparing the players with the last tick: the client did it already, so they
cannot be cancelled.

**Player**

| Event | Fires | May change |
| --- | --- | --- |
| `PlayerLoginEvent` | a player is placed, before the join message | `allowed` false (`kick_message`) turns them away |
| `PlayerJoinEvent` | a player is placed in the world | `join_message` (None: none) |
| `PlayerQuitEvent` | a player left | `quit_message` |
| `PlayerKickEvent` | a plugin kicks a player | `reason`, `cancelled` |
| `PlayerChatEvent` (`AsyncPlayerChatEvent`) | a chat message | `message`, `format`, `recipients`, `cancelled` |
| `PlayerCommandPreprocessEvent` | a command a player typed | `message`, `cancelled` |
| `PlayerMoveEvent` | a player moved (1/256 of a block) or turned (10°) this tick | `to`, `cancelled` (back to `from`) |
| `PlayerTeleportEvent` | `/tp`, an ender pearl, a chorus fruit | `to`, `cancelled` |
| `PlayerChangedWorldEvent` | a player arrived in another level (once a tick) | |
| `PlayerRespawnEvent` | a dead player comes back | `respawn_location` |
| `PlayerDeathEvent` | a player dies | `death_message`, `keep_inventory` |
| `PlayerInteractEvent` | a left click on a block, a right click on a block or in the air | `cancelled` (nothing used, placed or dug) |
| `PlayerInteractEntityEvent` | a right click on an entity | `cancelled` |
| `PlayerItemHeldEvent` | another hotbar slot | `cancelled` (the old slot stays) |
| `PlayerSwapHandItemsEvent` | the hands swap (F) | `cancelled` |
| `PlayerDropItemEvent` | a player throws an item from their hand | `cancelled` (it stays in the hand) |
| `PlayerPickupItemEvent` | a player is about to pick up a dropped item | `cancelled` (it stays on the ground) |
| `PlayerItemConsumeEvent` | a player finished eating or drinking | `cancelled` (the item and its effects stay) |
| `PlayerBucketFillEvent`, `PlayerBucketEmptyEvent` | a bucket is filled or emptied | `cancelled` |
| `PlayerBedEnterEvent` | a player lies down | `cancelled` (no sleep, no spawn point) |
| `PlayerBedLeaveEvent` | a player got up (once a tick) | |
| `PlayerGameModeChangeEvent` | a player's game mode changes | `cancelled` (the old mode stays) |
| `PlayerToggleSneakEvent`, `PlayerToggleSprintEvent` | sneaking or sprinting starts or stops (once a tick) | |
| `PlayerToggleFlightEvent` | flying starts or stops (once a tick) | `cancelled` (put back as they were) |
| `PlayerExpChangeEvent`, `PlayerLevelChangeEvent` | experience points or levels changed (once a tick) | |
| `PlayerAdvancementDoneEvent` | a player completed an advancement | |
| `PlayerResourcePackStatusEvent` | the client answered a resource pack | |

**Block**

| Event | Fires | May change |
| --- | --- | --- |
| `BlockBreakEvent` | a player breaks a block | `cancelled` (the block goes back to the player), `drop_items` |
| `BlockPlaceEvent` | a player placed a block | `cancelled` (the old block is put back) |
| `SignChangeEvent` | a player finished editing a sign | `lines`, `cancelled` |
| `BlockPistonExtendEvent`, `BlockPistonRetractEvent` | a piston is about to move | `cancelled` |
| `BlockFromToEvent` | a fluid is about to flow into a block | `cancelled` |
| `BlockGrowEvent` | a plant grew | `cancelled` (the blocks are put back) |
| `BlockSpreadEvent` | grass, mycelium, vines, mushrooms or fire spread | `cancelled` |
| `BlockFormEvent` | snow, ice; stone, cobblestone, obsidian or basalt where lava meets water | `cancelled` |
| `BlockFadeEvent` | ice or snow melting, coral dying, fire going out, grass or farmland to dirt | `cancelled` |
| `BlockFertilizeEvent` | bone meal is about to be used (a player's or a dispenser's) | `cancelled` (the meal stays) |
| `LeavesDecayEvent` | leaves are about to decay | `cancelled` |
| `BlockBurnEvent` | fire is about to burn a block away | `cancelled` |

**Entity**

| Event | Fires | May change |
| --- | --- | --- |
| `EntityDamageEvent` (also `EntityDamageByEntityEvent`: `damager` set) | a player is about to be hurt (mobs, players, explosions, falls, fire, lava, drowning, effects, rockets, falling blocks); a mob by its surroundings; a mob or a player by a player's hit | `damage` (before armour), `cancelled` |
| `CreatureSpawnEvent` | a mob is about to come into the world (natural, chunk generation, spawner, command or spawn egg, raid, patrol, golems built) | `cancelled` |
| `EntityDeathEvent` | a mob died | |
| `EntityExplodeEvent` | an explosion is about to break blocks and hurt | `blocks` (those removed are spared), `cancelled` |

**Inventory and server**

| Event | Fires | May change |
| --- | --- | --- |
| `InventoryOpenEvent` | a container, a workstation, a crafting table or trades are about to open | `cancelled` |
| `InventoryClickEvent` | a click in a menu (the player's own inventory too) | `cancelled` (the menu is sent again) |
| `InventoryCloseEvent` | a menu closed | |
| `ServerCommandEvent` | a console command | `command`, `cancelled` |

While a plugin listens to `EntityDamageEvent`, the damage of the surroundings (fire, lava, falls, drowning,
suffocation, the void, freezing, cactus, magma) is held back in the tick and dealt once the plugins have seen it
(`defer_env_damage` on `PlayerMotion` and `MobWorld`); without one, it is dealt in vanilla's order. Blocks
growing, decaying and burning, pistons and fluids go one at a time while their events have listeners.

Not yet: hunger (`FoodLevelChangeEvent`), projectiles (`ProjectileLaunchEvent`, `ProjectileHitEvent`), mobs
choosing targets (`EntityTargetEvent`), crafting, enchanting and anvils (`CraftItemEvent`, `EnchantItemEvent`,
`PrepareAnvilEvent`), vehicles, weather and the world's chunks (`WeatherChangeEvent`, `ChunkLoadEvent`),
redstone (`BlockRedstoneEvent`), dispensers (`BlockDispenseEvent`), ignition (`BlockIgniteEvent`).

## Tests

`cargo test -p horizon-server-api` (the manager: load order, priorities, cancellation, commands, timers,
panics) and `python3 tools/test_plugins_e2e.py` (a horizon-server built with the example plugin and a
horizon-client on it: join, commands, `/plugins`, chat, a cancelled break, a kept diamond, the game mode line,
a fall that hurts nothing, a teleport's cause, a cancelled phantom, a mob's death, TNT that breaks no blocks,
enable and disable in the log).
