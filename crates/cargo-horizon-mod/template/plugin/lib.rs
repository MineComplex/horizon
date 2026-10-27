//! {name}: a Horizon plugin (docs/plugins.md).
use horizon_mod_sdk::plugin::*;

struct MyPlugin {
    greeting: String,
}

impl Plugin for MyPlugin {
    fn enable() -> Result<Self, String> {
        // `defaults/config.yml` is written to plugins/{id}/ the first time.
        let config = data::simple_config(&data::config("config.yml").unwrap_or_default());
        let greeting = config
            .get("greeting")
            .cloned()
            .unwrap_or_else(|| "&aWelcome, {player}!".into());
        listen(EventKind::PlayerJoin, Priority::Normal);
        Ok(MyPlugin { greeting })
    }

    fn on_event(&mut self, _priority: Priority, event: &mut Event) {
        if let Event::PlayerJoin(e) = event {
            e.player.send(&color(&self.greeting.replace("{player}", &e.player.name)));
        }
    }

    fn on_command(&mut self, sender: &Sender, _command: &str, _label: &str, args: &[String]) -> bool {
        let who = args.first().map_or(sender.name(), |a| a.as_str());
        sender.send(&color(&format!("&eHello, {who}!")));
        true
    }
}

horizon_mod_sdk::export_plugin!(MyPlugin);
