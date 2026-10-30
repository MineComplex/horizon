//! An example horizon-server plugin, and the one the plugin tests run:
//!
//! - a welcome: its own join message, and a line to the player;
//! - bedrock nobody breaks, and placed TNT taken back unless the player is an operator;
//! - diamonds nobody throws away, and a line when a player's game mode changes;
//! - no fall damage for players: a line instead;
//! - a line for each teleport, no phantoms, a line when a mob dies, explosions that break no blocks;
//! - nobody goes hungry, no rain, and a log line once the server has started;
//! - `/time add 1000` does nothing, sponges stay dry, redstone lights no TNT, no Speed, furnaces
//!   smelt gold, and falling blocks land as nothing;
//! - chat: `!` at the start shouts (gold, to everyone), `/mute` silences a player;
//! - `/hello [name]`, `/heal` (operators: full food, a golden apple), `/mute <player>`;
//! - every 5 minutes, a tip to everyone.
use horizon_server_api::{
    ItemStack, Plugin, PluginDescription, Plugins,
    event::{
        BlockBreakEvent, BlockPlaceEvent, CreatureSpawnEvent, EntityDamageEvent, EntityDeathEvent,
        EntityExplodeEvent, EventPriority, FoodLevelChangeEvent, PlayerChatEvent, PlayerDeathEvent,
        PlayerDropItemEvent, PlayerGameModeChangeEvent, PlayerJoinEvent, PlayerTeleportEvent,
        ServerLoadEvent, WeatherChangeEvent,
    },
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct Example {
    muted: Arc<Mutex<HashSet<u128>>>,
}

impl Plugin for Example {
    fn description(&self) -> PluginDescription {
        PluginDescription::new("Example", "1.0")
            .description("Shows horizon-server's plugin API")
            .author("Horizon")
    }

    fn on_enable(&mut self, plugins: &mut Plugins) {
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut PlayerJoinEvent, server| {
                e.join_message = Some(format!("§a+ §f{}", e.player.name));
                let greeting = if e.first_join {
                    "§6Welcome to the server!"
                } else {
                    "§6Welcome back!"
                };
                server.send_message(&e.player, greeting);
            },
        );
        plugins.listen(
            EventPriority::High,
            false,
            |e: &mut BlockBreakEvent, server| {
                if e.block == "minecraft:bedrock" {
                    e.cancelled = true;
                    server.send_message(&e.player, "§cBedrock stays.");
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            true,
            |e: &mut BlockPlaceEvent, server| {
                if e.block == "minecraft:tnt" && !e.player.is_op() {
                    e.cancelled = true;
                    server.send_message(&e.player, "§cOnly operators place TNT.");
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut PlayerDropItemEvent, server| {
                if e.item.item == "minecraft:diamond" {
                    e.cancelled = true;
                    server.send_message(&e.player, "§bDiamonds stay in your hand.");
                }
            },
        );
        plugins.listen(
            EventPriority::Monitor,
            true,
            |e: &mut PlayerGameModeChangeEvent, server| {
                server.send_message(&e.player, &format!("§7Game mode {}.", e.new_game_mode));
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut EntityDamageEvent, server| {
                if e.cause == "minecraft:fall"
                    && let Some(player) = &e.entity.player
                {
                    e.cancelled = true;
                    server.send_message(player, &format!("§aSoft landing ({:.1}).", e.damage));
                }
            },
        );
        plugins.listen(
            EventPriority::Monitor,
            true,
            |e: &mut PlayerTeleportEvent, server| {
                server.send_message(&e.player, &format!("§7Teleported ({:?}).", e.cause));
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut CreatureSpawnEvent, _| {
                if e.entity.kind == "minecraft:phantom" {
                    e.cancelled = true;
                }
            },
        );
        plugins.listen(
            EventPriority::Monitor,
            false,
            |e: &mut EntityDeathEvent, server| {
                server.broadcast(&format!("§7{} died.", e.entity.kind));
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut EntityExplodeEvent, _| {
                e.blocks.clear();
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut FoodLevelChangeEvent, _| {
                if e.food_level < 20 {
                    e.cancelled = true;
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut WeatherChangeEvent, server| {
                if e.to_raining {
                    e.cancelled = true;
                    server.broadcast("§7Rain stays away.");
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::TimeSkipEvent, _| {
                if e.skip_reason == horizon_server_api::event::TimeSkipReason::Command
                    && e.skip_amount == 1000
                {
                    e.cancelled = true;
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::SpongeAbsorbEvent, server| {
                e.cancelled = true;
                server.broadcast(&format!("§7The sponge stays dry ({}).", e.blocks.len()));
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::TntPrimeEvent, _| {
                if e.cause == horizon_server_api::event::PrimeCause::Redstone {
                    e.cancelled = true;
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::EntityPotionEffectEvent, server| {
                if e.new_effect
                    .as_ref()
                    .is_some_and(|n| n.kind == "minecraft:speed")
                {
                    e.cancelled = true;
                    if let Some(p) = &e.entity.player {
                        server.send_message(p, "§7No speed here.");
                    }
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::FurnaceSmeltEvent, _| {
                e.result = ItemStack::new("gold_ingot", e.result.count);
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut horizon_server_api::event::EntityChangeBlockEvent, _| {
                if e.entity.kind == "minecraft:falling_block" {
                    e.cancelled = true;
                }
            },
        );
        plugins.listen(
            EventPriority::Monitor,
            false,
            |_: &mut ServerLoadEvent, server| {
                server.log("Server loaded.");
            },
        );
        let muted = self.muted.clone();
        plugins.listen(
            EventPriority::Low,
            false,
            move |e: &mut PlayerChatEvent, server| {
                if muted.lock().unwrap().contains(&e.player.uuid) {
                    e.cancelled = true;
                    server.send_message(&e.player, "§cYou are muted.");
                    return;
                }
                if let Some(shout) = e.message.strip_prefix('!') {
                    e.message = shout.trim().to_owned();
                    e.format = "§6[Shout] {name}: {message}".into();
                }
            },
        );
        plugins.listen(
            EventPriority::Normal,
            false,
            |e: &mut PlayerDeathEvent, _| {
                if let Some(message) = &e.death_message {
                    e.death_message = Some(format!("§7☠ {message}"));
                }
            },
        );
        plugins
            .command("hello", "Says hello", Some("/<command> [name]"), |ctx| {
                let who = ctx
                    .args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ctx.sender.name().to_owned());
                ctx.reply(&format!("§eHello, {who}!"));
                true
            })
            .alias("hi")
            .complete(|_, _| vec!["world".into()]);
        plugins
            .command(
                "heal",
                "Feeds you and hands you a golden apple",
                None,
                |ctx| {
                    let Some(player) = ctx.sender.player().cloned() else {
                        ctx.reply("Only players can be healed.");
                        return true;
                    };
                    ctx.server.dispatch_command(&format!(
                        "effect give {} minecraft:saturation 1 20 true",
                        player.name
                    ));
                    ctx.server.give(&player, ItemStack::new("golden_apple", 1));
                    ctx.reply("§aHealed.");
                    true
                },
            )
            .op(2);
        let muted = self.muted.clone();
        plugins
            .command(
                "mute",
                "Mutes or unmutes a player's chat",
                Some("/<command> <player>"),
                move |ctx| {
                    let Some(name) = ctx.args.first() else {
                        return false;
                    };
                    let Some(target) = ctx.server.player(name) else {
                        ctx.reply(&format!("§c{name} is not online."));
                        return true;
                    };
                    let mut muted = muted.lock().unwrap();
                    let now_muted = muted.insert(target.uuid) || {
                        muted.remove(&target.uuid);
                        false
                    };
                    let state = if now_muted { "muted" } else { "unmuted" };
                    ctx.reply(&format!("§e{} is {state}.", target.name));
                    true
                },
            )
            .op(2)
            .complete(|_, _| Vec::new());
        plugins.run_timer(20 * 60 * 5, 20 * 60 * 5, |server| {
            server.broadcast("§7Tip: start a chat message with ! to shout.");
            true
        });
    }

    fn on_disable(&mut self) {
        self.muted.lock().unwrap().clear();
    }
}
