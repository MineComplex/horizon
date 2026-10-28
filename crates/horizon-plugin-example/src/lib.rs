//! An example horizon-server plugin, and the one the plugin tests run:
//!
//! - a welcome: its own join message, and a line to the player;
//! - bedrock nobody breaks, and placed TNT taken back unless the player is an operator;
//! - chat: `!` at the start shouts (gold, to everyone), `/mute` silences a player;
//! - `/hello [name]`, `/heal` (operators: full food, a golden apple), `/mute <player>`;
//! - every 5 minutes, a tip to everyone.
use horizon_server_api::{
    ItemStack, Plugin, PluginDescription, Plugins,
    event::{
        BlockBreakEvent, BlockPlaceEvent, EventPriority, PlayerChatEvent, PlayerDeathEvent,
        PlayerJoinEvent,
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
