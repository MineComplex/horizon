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
| `FoodLevelChangeEvent` | the food level changed (once a tick) | `cancelled` (the old level is put back) |
| `PlayerFishEvent` | a rod is cast or reeled in (`Fishing`, `CaughtFish`, `CaughtEntity`, `ReelIn`) | `cancelled` |
| `PlayerEditBookEvent` | a book and quill is saved or signed | `pages`, `title`, `cancelled` |
| `PlayerPortalEvent` | a player is about to go through a portal | `cancelled` |
| `PlayerShearEntityEvent` | a sheep, mooshroom, snow golem or bogged is about to be sheared | `cancelled` |

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
| `BlockDamageEvent` | a player starts digging a block | `cancelled` |
| `BlockDispenseEvent` | a dispenser or dropper is about to dispense | `cancelled` (the item stays inside) |
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
| `EntityRegainHealthEvent` | a player's health went up (once a tick) | `cancelled` (taken back) |
| `EntityTargetLivingEntityEvent` | a mob chose or dropped its target (once a tick) | `cancelled` (the old target back) |
| `EntityTameEvent` | a mob was tamed (once a tick) | `cancelled` (wild again) |
| `EntityBreedEvent` | two animals are about to make a baby | `cancelled` |
| `ProjectileLaunchEvent` | a projectile came into the world (once a tick) | `cancelled` (removed) |
| `ProjectileHitEvent` | a projectile hit a block or an entity | `cancelled` (the hit does nothing) |
| `ItemSpawnEvent` | a dropped item is about to come into the world | `cancelled` |

**Inventory and server**

| Event | Fires | May change |
| --- | --- | --- |
| `InventoryOpenEvent` | a container, a workstation, a crafting table or trades are about to open | `cancelled` |
| `InventoryClickEvent` | a click in a menu (the player's own inventory too) | `cancelled` (the menu is sent again) |
| `InventoryCloseEvent` | a menu closed | |
| `CraftItemEvent` | the result is taken out of a crafting grid | `cancelled` (the menu is sent again) |
| `EnchantItemEvent` | an item is about to be enchanted at a table | `cancelled` |
| `VehicleEnterEvent`, `VehicleExitEvent` | a player got on or off a boat, minecart or mount (once a tick) | |
| `WeatherChangeEvent`, `ThunderChangeEvent` | rain or a thunderstorm starts or stops | `cancelled` (the weather goes back) |
| `WorldSaveEvent` | a level is being saved | |
| `ServerLoadEvent` | the server finished starting | |
| `ServerCommandEvent` | a console command | `command`, `cancelled` |

While a plugin listens to `EntityDamageEvent`, the damage of the surroundings (fire, lava, falls, drowning,
suffocation, the void, freezing, cactus, magma) is held back in the tick and dealt once the plugins have seen it
(`defer_env_damage` on `PlayerMotion` and `MobWorld`); without one, it is dealt in vanilla's order. Blocks
growing, decaying and burning, pistons and fluids go one at a time while their events have listeners.

**More block events** (`event/blocks.rs`; the level's own changes are held back and asked first while a
plugin listens: `horizon-world/src/level/plugin_asks.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `BlockRedstoneEvent` | a wire, lever, button, torch or lamp changed its power | (heard after the change) |
| `BlockIgniteEvent` | flint and steel, fire charges, lightning, fireballs, explosions, End crystals, lava, fire spreading | `cancelled` |
| `BlockExplodeEvent` | a bed or respawn anchor explodes | `blocks`, `cancelled` |
| `NotePlayEvent`, `BellRingEvent` | a note block plays, a bell rings | `cancelled` |
| `TNTPrimeEvent` (`TntPrimeEvent`) | TNT is lit (redstone, fire, a player, a dispenser, a flaming arrow, an explosion) | `cancelled` |
| `SpongeAbsorbEvent` | a sponge is about to soak up water | `blocks`, `cancelled` |
| `BlockMultiPlaceEvent` | a bed, door or tall plant placed (after `BlockPlaceEvent`) | `cancelled` |
| `CauldronLevelChangeEvent` | a cauldron is filled or emptied (players, rain, dripstone, a burning player) | `new_state`, `cancelled` |
| `MoistureChangeEvent`, `FluidLevelChangeEvent` | farmland's moisture, a fluid's level | `new_state`, `cancelled` |
| `BlockShearEntityEvent` | a dispenser shears a mob | `cancelled` |
| `BlockCookEvent` | a campfire finished cooking | `result`, `cancelled` |

**More inventory events** (`event/inventories.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `PrepareItemCraftEvent` | a crafting grid's result is worked out | `result` |
| `PrepareAnvilEvent`, `PrepareSmithingEvent`, `PrepareGrindstoneEvent`, `PrepareInventoryResultEvent` | an anvil's, smithing table's, grindstone's, loom's, stonecutter's or cartography table's result | `result` (anvils: `repair_cost`) |
| `SmithItemEvent` | the smithing result is taken | `cancelled` |
| `FurnaceBurnEvent`, `FurnaceSmeltEvent`, `FurnaceExtractEvent` | a furnace lights fuel, smelts an item, a player takes the output | `burn_time`; `result`; `exp`; `cancelled` |
| `BrewingStandFuelEvent`, `BrewEvent` | a brewing stand takes blaze powder, finishes a brew | `fuel_power`, `results`, `cancelled` |
| `CrafterCraftEvent` | a crafter crafts | `result`, `cancelled` |
| `InventoryMoveItemEvent`, `InventoryPickupItemEvent` | a hopper or dropper moves an item, a hopper picks one up | `cancelled` |
| `InventoryDragEvent` | a drag spreads items over slots | `cancelled` |
| `TradeSelectEvent` | a trade is selected | `cancelled` |
| `PlayerRecipeDiscoverEvent`, `PlayerRecipeBookClickEvent` | a recipe is unlocked, the recipe book places one | `recipe`, `cancelled` |
| `PlayerTakeLecternBookEvent` | the book is taken from a lectern | `cancelled` |

**More entity events** (`event/entities.rs`; "once a tick" ones compare the mobs and players with the last tick)

| Event | Fires | May change |
| --- | --- | --- |
| `EntityPickupItemEvent` | a mob picks up an item | `cancelled` |
| `EntityChangeBlockEvent`, `EntityBreakDoorEvent`, `EntityInteractEvent` | a mob changes a block (endermen, sheep, silverfish, ravagers...), a falling block lands, a door is broken, farmland is trampled | `cancelled` |
| `EntityCombustEvent`, `EntityAirChangeEvent` | catching fire, air changing (once a tick) | duration, `amount`, `cancelled` |
| `ExplosionPrimeEvent` | a creeper, wither, sulfur cube or TNT is about to explode | radius, `fire`, `cancelled` |
| `EntityShootBowEvent` | a mob or a player shoots a bow or crossbow | `cancelled` |
| `EntityPotionEffectEvent` | an effect is added, changed, removed or cleared (once a tick) | `cancelled` |
| `EntityTeleportEvent`, `EntityPortalEvent` | an enderman or shulker teleports, an entity goes through a portal | `to`, `cancelled` |
| `EntityTransformEvent` | strays, drowned, zombified piglins, frogs, cured villagers, infections, lightning | `cancelled` |
| `EntityResurrectEvent` | a totem of undying | `cancelled` |
| `EntityToggleGlideEvent`, `EntityToggleSwimEvent` | gliding or swimming starts or stops (once a tick) | |
| `EntityMountEvent`, `EntityDismountEvent` | an entity gets on or off another (once a tick) | `cancelled` (mobs) |
| `CreeperPowerEvent`, `PigZapEvent`, `LightningStrikeEvent` | lightning powers a creeper, strikes a pig, strikes | `cancelled` |
| `SheepDyeWoolEvent`, `SheepRegrowWoolEvent`, `SlimeSplitEvent` | a sheep is dyed or regrows wool, a slime splits | colour, count, `cancelled` |
| `VillagerAcquireTradeEvent`, `VillagerReplenishTradeEvent`, `PiglinBarterEvent` | a villager gets or restocks a trade, a piglin barters | `result`, `max_uses`, outcome, `cancelled` |
| `ItemDespawnEvent`, `ItemMergeEvent` | a dropped item despawns, two merge | `cancelled` |
| `PotionSplashEvent`, `LingeringPotionSplashEvent`, `AreaEffectCloudApplyEvent` | a potion splashes, a cloud applies | the entities hit, `cancelled` |
| `FireworkExplodeEvent`, `HorseJumpEvent`, `EntityEnterLoveModeEvent`, `BatToggleSleepEvent` | | power, ticks, `cancelled` |
| `EntityDropItemEvent`, `EntitySpellCastEvent` | a mob drops an item (eggs, scutes, gifts), an evoker casts | `cancelled` |

**More player, world and server events** (`event/world.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `PlayerStatisticIncrementEvent` | a statistic goes up | `cancelled` |
| `PlayerItemDamageEvent`, `PlayerItemBreakEvent`, `PlayerItemMendEvent` | an item wears, breaks, is mended | damage, repair amount, `cancelled` |
| `PlayerRiptideEvent`, `PlayerHarvestBlockEvent` | a riptide trident, sweet berries or glow berries picked | the items, `cancelled` |
| `PlayerArmorStandManipulateEvent`, `PlayerLeashEntityEvent`, `PlayerUnleashEntityEvent`, `PlayerBucketEntityEvent`, `PlayerSignOpenEvent`, `PlayerInteractAtEntityEvent`, `PlayerAnimationEvent` | | `cancelled` |
| `PlayerChangedMainHandEvent`, `PlayerLocaleChangeEvent` | the client's settings changed | |
| `PlayerCommandSendEvent` | the command tree is sent | the commands |
| `PlayerVelocityEvent`, `PlayerEggThrowEvent`, `PlayerExpCooldownChangeEvent` | | velocity, hatching, cooldown |
| `AsyncPlayerPreLoginEvent`, `AsyncTabCompleteEvent`, `TabCompleteEvent`, `BroadcastMessageEvent` | | the result, completions, message |
| `ChunkLoadEvent`, `ChunkUnloadEvent`, `ChunkPopulateEvent` | chunks of a level load, unload, are generated | |
| `WorldInitEvent`, `WorldLoadEvent`, `WorldUnloadEvent` | a level opens or closes | `cancelled` (unload) |
| `StructureGrowEvent` | bone meal grows a tree or fungus | `blocks`, `cancelled` |
| `PortalCreateEvent`, `SpawnChangeEvent`, `TimeSkipEvent` | a portal is lit or made on arrival, the world spawn moves, the night is slept or `/time` jumps | `skip_amount`, `cancelled` |
| `RaidTriggerEvent`, `RaidSpawnWaveEvent`, `RaidFinishEvent`, `RaidStopEvent` | raids | `cancelled` (trigger) |
| `GenericGameEvent` | a vanilla game event (vibrations) | radius, `cancelled` |
| `LootGenerateEvent` | a container's or a mob's loot | the loot, `cancelled` |

**Paintings, frames and vehicles** (`event/vehicles.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `HangingPlaceEvent` | a painting, item frame or leash knot is placed | `cancelled` |
| `HangingBreakEvent`, `HangingBreakByEntityEvent` | a painting or frame breaks (`Entity`, `Obstruction`, `Physics`) | `cancelled` |
| `VehicleCreateEvent` | a boat or minecart is placed, dispensed or summoned | `cancelled` |
| `VehicleDamageEvent`, `VehicleDestroyEvent` | a boat or minecart is hurt or about to break | `damage`, `cancelled` |
| `VehicleMoveEvent`, `VehicleUpdateEvent` | once a tick for each vehicle (that moved) | |
| `VehicleBlockCollisionEvent`, `VehicleEntityCollisionEvent` | a vehicle runs into a block or an entity | `cancelled` (entities: no pickup, no push) |
| `EntityPlaceEvent` | a boat, minecart, armor stand or end crystal is placed | `cancelled` |
| `EntityUnleashEvent` | a lead comes off (distance, holder gone, player) | `drop_leash` |

**More mob events** (`event/mobs.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `EntityKnockbackEvent`, `EntityKnockbackByEntityEvent` | knockback about to land on a mob or a player | `knockback`, `cancelled` |
| `EntityExhaustionEvent` | a player's exhaustion (mining, attacks, jumps, sprinting, swimming, walking, regeneration, Hunger) | `exhaustion`, `cancelled` |
| `EntityPoseChangeEvent`, `EntityRemoveEvent` | a pose changed, an entity is gone (once a tick) | |
| `EntityPortalEnterEvent`, `EntityPortalExitEvent` | an entity touches a portal, arrives through one | `cancelled`; `to`, `after` |
| `EnderDragonChangePhaseEvent` | the dragon's phase changes | `new_phase`, `cancelled` |
| `PigZombieAngerEvent`, `VillagerCareerChangeEvent`, `StriderTemperatureChangeEvent` | (once a tick) | `new_anger`, `cancelled` |
| `EntitiesLoadEvent`, `EntitiesUnloadEvent` | a chunk's entities load or unload | |
| `EntityEnterBlockEvent`, `EntityBlockFormEvent` | a bee enters its hive; Frost Walker ice, a snow golem's snow | `cancelled` |
| `EntitySpawnEvent` | an orb, primed TNT, falling block, potion cloud or bolt comes into the world | `cancelled` |
| `EntityTargetEvent` | before `EntityTargetLivingEntityEvent` | `target`, `cancelled` |
| `EntityCombustByEntityEvent`, `EntityCombustByBlockEvent` | set on fire by an arrow, fireball, Fire Aspect, lightning; by fire or lava | `duration`, `cancelled` |
| `EntityDamageByBlockEvent` | damage from cactus, berry bushes, campfires, fire, lava, magma, dripstone | `damage`, `cancelled` |

**More block and station events** (`event/stations.rs`)

| Event | Fires | May change |
| --- | --- | --- |
| `FurnaceStartSmeltEvent`, `BrewingStartEvent`, `CampfireStartEvent` | cooking or brewing starts | the total time |
| `BlockDropItemEvent`, `BlockExpEvent` | the items and experience of a block a player broke | the items, `exp_to_drop`, `cancelled` |
| `BlockDamageAbortEvent` | a player stops digging | |
| `BlockDispenseArmorEvent`, `BlockDispenseLootEvent` | a dispenser puts armour on, a vault ejects its loot | the loot, `cancelled` |
| `HopperInventorySearchEvent` | a hopper looks for a container | `inventory` (None only) |
| `InventoryCreativeEvent` | a creative player sets a slot | the item, `cancelled` |
| `PrepareItemEnchantEvent` | the enchanting table's offers | the offers, `cancelled` |
| `SpawnerSpawnEvent`, `TrialSpawnerSpawnEvent` | a spawner spawns a mob | `cancelled` |
| `VaultDisplayItemEvent`, `SculkBloomEvent`, `BlockReceiveGameEvent` | a vault's shown item, a catalyst's bloom, a sensor or shrieker hears a vibration | the item, `charge`, `cancelled` |
| `BlockCanBuildEvent` | a player's block is about to be placed | `buildable` (may only refuse) |
| `AsyncStructureGenerateEvent`, `AsyncStructureSpawnEvent` | a structure is in a newly generated chunk | |
| `MapInitializeEvent` | a new map | centre, scale, tracking, locked |

**More player and server events** (`event/players.rs`, Paper's included)

| Event | Fires | May change |
| --- | --- | --- |
| `PlayerAttemptPickupItemEvent`, `PlayerBucketFishEvent` | an item in reach (even with a full inventory); a fish into a bucket | `fly_at_player`, `cancelled` |
| `PlayerInputEvent`, `PlayerRecipeBookSettingsChangeEvent`, `PlayerClientOptionsChangeEvent`, `PlayerHealthChangeEvent` | (once a tick) | |
| `PlayerLinksSendEvent` | the server links | the links |
| `PlayerSpawnChangeEvent` | a bed, an anchor, `/spawnpoint`, a lost spawn point | the point, `forced`, `cancelled` |
| `UnknownCommandEvent` | a command that doesn't parse | `message` |
| `PluginEnableEvent`, `PluginDisableEvent` | each plugin enabled or about to be disabled | |
| `PlayerRegisterChannelEvent`, `PlayerUnregisterChannelEvent` | `minecraft:register` / `unregister` | |
| `ClockTimeSkipEvent` | `/time` (then `TimeSkipEvent`) | `skip_amount`, `cancelled` |
| `PlayerJumpEvent` | a jump | `from`, `cancelled` |
| `PlayerArmorChangeEvent`, `PlayerInventorySlotChangeEvent`, `PlayerPostRespawnEvent` | (once a tick) | |
| `ServerTickStartEvent`, `ServerTickEndEvent` | every server tick | |
| `PlayerReadyArrowEvent`, `PlayerLaunchProjectileEvent`, `PlayerStopUsingItemEvent` | a bow picks its arrow; a snowball, egg or pearl is thrown; an item stops being used | `should_consume`, `cancelled` |
| `PlayerFlowerPotManipulateEvent`, `PlayerPickBlockEvent`, `PlayerPickEntityEvent` | | `cancelled` |
| `AsyncChatEvent` | after `AsyncPlayerChatEvent` | the message, viewers, `cancelled` |
| `WhitelistToggleEvent`, `ServerResourcesReloadedEvent` | `whitelist on/off`, `/reload` | |

Not covered: `BlockCanBuildEvent` can't force a placement horizon-world refuses; natural tree growth in
`StructureGrowEvent` (bone meal only); `BellResonateEvent` (the server doesn't run the bell's resonance),
`PlayerPickupArrowEvent` (the server has no arrow pickup by players yet), `ArrowBodyCountChangeEvent`
(arrows in bodies aren't kept), `PlayerTradeEvent`, `PlayerHideEntityEvent` and `PlayerShowEntityEvent` (no
hide/show in the API), and `BlockPhysicsEvent` (left out on purpose). Bukkit's abstract events
(`PlayerEvent`, `BlockEvent`...), the server-list ping (Velocity answers it), RCON and the plugin-messaging
service have no events here.

## Tests

`cargo test -p horizon-server-api` (the manager: load order, priorities, cancellation, commands, timers,
panics) and `python3 tools/test_plugins_e2e.py` (a horizon-server built with the example plugin and a
horizon-client on it: join, commands, `/plugins`, chat, a cancelled break, a kept diamond, the game mode line,
a fall that hurts nothing, a teleport's cause, a cancelled phantom, a mob's death, TNT that breaks no blocks, the server-loaded line,
food that never drops, rain taken back, a cancelled time jump, a dry sponge, TNT that redstone doesn't light, a
refused effect, a furnace that smelts gold, falling sand that never lands, a 20-tick cook time, the plugin's
unknown-command line, a minecart that never comes, enable and disable in the log).
