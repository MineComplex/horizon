//! Example Horizon plugin (docs/plugins.md): every event a plugin can hear,
//! logged to the console as `EVENT <kind> <details>` (a monitor listener),
//! and `/eventlog cancel <kind>` cancelling that kind at normal priority.
//! `/eventlog rewrite on` turns chat to upper case and `/hi` into `/help`.
use horizon_mod_sdk::plugin::*;
use std::collections::BTreeSet;

const KINDS: [(&str, EventKind); 32] = [
    ("player-join", EventKind::PlayerJoin),
    ("player-quit", EventKind::PlayerQuit),
    ("player-login", EventKind::PlayerLogin),
    ("player-chat", EventKind::PlayerChat),
    ("player-command", EventKind::PlayerCommand),
    ("player-move", EventKind::PlayerMove),
    ("player-interact", EventKind::PlayerInteract),
    ("player-interact-entity", EventKind::PlayerInteractEntity),
    ("player-toggle-sneak", EventKind::PlayerToggleSneak),
    ("player-toggle-sprint", EventKind::PlayerToggleSprint),
    ("player-toggle-flight", EventKind::PlayerToggleFlight),
    ("player-item-held", EventKind::PlayerItemHeld),
    ("player-drop-item", EventKind::PlayerDropItem),
    ("player-swap-hands", EventKind::PlayerSwapHands),
    ("player-respawn", EventKind::PlayerRespawn),
    ("player-death", EventKind::PlayerDeath),
    ("player-game-mode-change", EventKind::PlayerGameModeChange),
    ("player-changed-world", EventKind::PlayerChangedWorld),
    ("sign-change", EventKind::SignChange),
    ("plugin-message", EventKind::PluginMessage),
    ("player-resource-pack", EventKind::PlayerResourcePack),
    ("block-break", EventKind::BlockBreak),
    ("block-place", EventKind::BlockPlace),
    ("entity-damage", EventKind::EntityDamage),
    ("entity-death", EventKind::EntityDeath),
    ("entity-spawn", EventKind::EntitySpawn),
    ("entity-explode", EventKind::EntityExplode),
    ("inventory-click", EventKind::InventoryClick),
    ("inventory-close", EventKind::InventoryClose),
    ("server-command", EventKind::ServerCommand),
    ("plugin-state", EventKind::PluginState),
    ("plugin-call", EventKind::PluginCall),
];

struct EventLog {
    cancel: BTreeSet<String>,
    rewrite: bool,
}

fn pos(p: &BlockPos) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}

/// The kind's name and what the log line says of the event.
fn describe(e: &Event) -> (&'static str, String) {
    match e {
        Event::PlayerJoin(e) => ("player-join", format!("{} first={}", e.player.name, e.first_join)),
        Event::PlayerQuit(e) => ("player-quit", format!("{} {}", e.player.name, e.reason)),
        Event::PlayerLogin(e) => ("player-login", e.player.name.clone()),
        Event::PlayerChat(e) => ("player-chat", format!("{}: {}", e.player.name, e.message)),
        Event::PlayerCommand(e) => ("player-command", format!("{}: /{}", e.player.name, e.line)),
        Event::PlayerMove(e) => (
            "player-move",
            format!("{} {:.1} {:.1} {:.1}", e.player.name, e.to.x, e.to.y, e.to.z),
        ),
        Event::PlayerInteract(e) => (
            "player-interact",
            format!(
                "{} {:?} {}",
                e.player.name,
                e.action,
                e.block.as_ref().map(|b| pos(&b.pos)).unwrap_or_default()
            ),
        ),
        Event::PlayerInteractEntity(e) => (
            "player-interact-entity",
            format!("{} {}", e.player.name, e.entity.kind),
        ),
        Event::PlayerToggleSneak(e) => ("player-toggle-sneak", format!("{} {}", e.player.name, e.sneaking)),
        Event::PlayerToggleSprint(e) => ("player-toggle-sprint", format!("{} {}", e.player.name, e.sprinting)),
        Event::PlayerToggleFlight(e) => ("player-toggle-flight", format!("{} {}", e.player.name, e.flying)),
        Event::PlayerItemHeld(e) => (
            "player-item-held",
            format!("{} {} -> {}", e.player.name, e.previous, e.current),
        ),
        Event::PlayerDropItem(e) => ("player-drop-item", format!("{} {}", e.player.name, e.item.id)),
        Event::PlayerSwapHands(e) => ("player-swap-hands", e.player.name.clone()),
        Event::PlayerRespawn(e) => ("player-respawn", e.player.name.clone()),
        Event::PlayerDeath(e) => (
            "player-death",
            format!("{} keep-inventory={}", e.player.name, e.keep_inventory),
        ),
        Event::PlayerGameModeChange(e) => ("player-game-mode-change", format!("{} {:?}", e.player.name, e.mode)),
        Event::PlayerChangedWorld(e) => ("player-changed-world", format!("{} from {}", e.player.name, e.from)),
        Event::SignChange(e) => ("sign-change", format!("{} {}", e.player.name, e.lines.join("|"))),
        Event::PluginMessage(e) => ("plugin-message", format!("{} {}", e.player.name, e.channel)),
        Event::PlayerResourcePack(e) => ("player-resource-pack", format!("{} {}", e.player.name, e.status)),
        Event::BlockBreak(e) => ("block-break", format!("{} {} {}", e.player.name, e.block.state, pos(&e.block.pos))),
        Event::BlockPlace(e) => ("block-place", format!("{} {} {}", e.player.name, e.block.state, pos(&e.block.pos))),
        Event::EntityDamage(e) => (
            "entity-damage",
            format!(
                "{} {} {:.1} by {}",
                e.entity.kind,
                e.cause,
                e.damage,
                e.damager.as_ref().map_or("-".into(), |d| d.kind.clone())
            ),
        ),
        Event::EntityDeath(e) => ("entity-death", e.entity.kind.clone()),
        Event::EntitySpawn(e) => ("entity-spawn", format!("{} {}", e.entity.kind, e.reason)),
        Event::EntityExplode(e) => ("entity-explode", format!("{} blocks", e.blocks.len())),
        Event::InventoryClick(e) => ("inventory-click", format!("{} {} slot {}", e.player.name, e.view.kind, e.slot)),
        Event::InventoryClose(e) => ("inventory-close", format!("{} {}", e.player.name, e.view.kind)),
        Event::ServerCommand(e) => ("server-command", e.line.clone()),
        Event::PluginState(e) => ("plugin-state", format!("{} {}", e.plugin, e.enabled)),
        Event::PluginCall(e) => ("plugin-call", e.from.clone()),
    }
}

fn cancel(e: &mut Event) {
    match e {
        Event::PlayerLogin(e) => {
            e.allowed = false;
            e.kick_message = "§cEvent Log turned you away".into();
        }
        Event::PlayerDeath(e) => e.keep_inventory = true,
        other => {
            other.set_cancelled(true);
        }
    }
}

impl Plugin for EventLog {
    fn enable() -> Result<Self, String> {
        for (_, kind) in KINDS {
            listen(kind, Priority::Normal);
            listen(kind, Priority::Monitor);
        }
        Ok(EventLog {
            cancel: BTreeSet::new(),
            rewrite: false,
        })
    }

    fn on_event(&mut self, priority: Priority, event: &mut Event) {
        let (kind, details) = describe(event);
        if priority == Priority::Monitor {
            // The player's own moves come every tick: only those that matter.
            server::console(&format!(
                "EVENT {kind} {details}{}",
                if event.is_cancelled() { " (cancelled)" } else { "" }
            ));
            return;
        }
        if self.rewrite {
            match event {
                Event::PlayerChat(e) => e.message = e.message.to_uppercase(),
                Event::PlayerCommand(e) if e.line == "hi" => e.line = "help".into(),
                _ => {}
            }
        }
        if self.cancel.contains(kind) {
            cancel(event);
        }
    }

    fn on_command(&mut self, sender: &Sender, _command: &str, _label: &str, args: &[String]) -> bool {
        match (args.first().map(String::as_str), args.get(1)) {
            (Some("cancel"), Some(kind)) => {
                self.cancel.insert(kind.clone());
                sender.send(&format!("cancelling {kind}"));
            }
            (Some("allow"), Some(kind)) => {
                self.cancel.remove(kind);
                sender.send(&format!("allowing {kind}"));
            }
            (Some("rewrite"), Some(on)) => {
                self.rewrite = on == "on";
                sender.send(&format!("rewrite {}", self.rewrite));
            }
            (Some("list"), _) => {
                let names: Vec<&str> = KINDS.iter().map(|(n, _)| *n).collect();
                sender.send(&names.join(" "));
            }
            _ => return false,
        }
        true
    }

    fn on_tab_complete(&mut self, _sender: &Sender, _command: &str, args: &[String]) -> Vec<String> {
        let last = args.last().cloned().unwrap_or_default();
        match args.len() {
            1 => ["cancel", "allow", "rewrite", "list"]
                .iter()
                .filter(|w| w.starts_with(&last))
                .map(|w| (*w).to_owned())
                .collect(),
            2 => KINDS
                .iter()
                .map(|(n, _)| (*n).to_owned())
                .filter(|n| n.starts_with(&last))
                .collect(),
            _ => Vec::new(),
        }
    }
}

horizon_mod_sdk::export_plugin!(EventLog);
