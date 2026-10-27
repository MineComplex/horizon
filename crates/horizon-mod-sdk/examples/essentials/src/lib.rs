//! Example Horizon plugin (docs/plugins.md): the plugin API in one place.
//!
//! - Commands from the manifest (`/spawn`, `/setspawn`, `/heal`, `/fly`,
//!   `/gamemode` alias `/gm`, `/broadcast` alias `/bc`, `/menu`, `/sethome`,
//!   `/home`, `/tps`) with their permissions, usage and tab completion.
//! - Events: join and quit messages, the chat format, spawn protection
//!   (block breaking and placing), no fall damage at spawn, a menu whose
//!   clicks are cancelled and answered.
//! - A config (`defaults/config.yml`), homes kept in the data folder, a
//!   sidebar and a boss bar, and an announcement on a timer.
use horizon_mod_sdk::plugin::*;
use std::collections::BTreeMap;

struct Essentials {
    config: BTreeMap<String, String>,
    spawn: Location,
    homes: BTreeMap<String, Location>,
    /// The menu each player has open.
    menus: BTreeMap<String, Gui>,
    welcome_bar: Option<u32>,
}

/// A location as one line: `world x y z yaw pitch`.
fn location_line(l: &Location) -> String {
    format!("{} {} {} {} {} {}", l.world, l.x, l.y, l.z, l.yaw, l.pitch)
}
fn parse_location(line: &str) -> Option<Location> {
    let mut w = line.split_whitespace();
    let world = w.next()?.to_owned();
    let mut n = || w.next()?.parse::<f64>().ok();
    let (x, y, z, yaw, pitch) = (n()?, n()?, n()?, n()?, n()?);
    Some(Location::new(&world, x, y, z).with_rotation(yaw as f32, pitch as f32))
}

impl Essentials {
    fn get(&self, key: &str) -> &str {
        self.config.get(key).map(String::as_str).unwrap_or_default()
    }
    fn radius(&self) -> f64 {
        self.get("protection-radius").parse().unwrap_or(16.0)
    }
    /// Whether a block is inside the spawn's protection.
    fn protected(&self, block: &Block) -> bool {
        let center = block.pos.center(&block.world);
        center.world == self.spawn.world
            && (center.x - self.spawn.x).abs() <= self.radius()
            && (center.z - self.spawn.z).abs() <= self.radius()
    }
    fn save_homes(&self) {
        let text: String = self
            .homes
            .iter()
            .map(|(id, l)| format!("{id} {}\n", location_line(l)))
            .collect();
        let _ = data::write("homes.txt", &text);
    }
    fn sidebar(&self, p: &Player) {
        let online = players::online().len();
        scoreboards::set_sidebar(
            &p.id,
            &color("&6&lHorizon"),
            &[
                color(&format!("&fPlayer: &e{}", p.name)),
                color(&format!("&fOnline: &a{online}")),
                String::new(),
                color("&7/menu for help"),
            ],
        );
    }
    fn open_menu(&mut self, p: &Player) {
        let gui = Gui::chest(3, &color("&8Server menu"));
        gui.set(
            11,
            Some(
                &ItemStack::of("compass", 1)
                    .item_named(&color("&aTeleport to spawn"))
                    .lore(&[&color("&7Click to go to the spawn")])
                    .with_tag("action", "spawn"),
            ),
        );
        gui.set(
            13,
            Some(
                &ItemStack::of("red_bed", 1)
                    .item_named(&color("&bGo home"))
                    .with_tag("action", "home"),
            ),
        );
        gui.set(
            15,
            Some(
                &ItemStack::of("golden_apple", 1)
                    .item_named(&color("&6Heal"))
                    .glint(true)
                    .with_tag("action", "heal"),
            ),
        );
        gui.fill_empty(&ItemStack::of("gray_stained_glass_pane", 1).item_named(" "));
        if let Some(old) = self.menus.insert(p.id.clone(), gui) {
            old.destroy();
        }
        gui.open(&p.id);
    }
    fn menu_action(&mut self, p: &Player, action: &str) {
        players::close_inventory(&p.id);
        if let Some(gui) = self.menus.remove(&p.id) {
            gui.destroy();
        }
        match action {
            "spawn" => {
                p.teleport(&self.spawn);
                p.send(&color("&aWelcome back to the spawn."));
            }
            "home" => self.go_home(p),
            "heal" if p.has_permission("essentials.heal") => {
                players::set_health(&p.id, p.max_health);
                players::set_food(&p.id, 20);
                p.send(&color("&aHealed."));
            }
            "heal" => p.send(&color("&cYou may not heal.")),
            _ => {}
        }
    }
    fn go_home(&self, p: &Player) {
        match self.homes.get(&p.id) {
            Some(home) => {
                p.teleport(home);
                p.send(&color("&aWelcome home."));
            }
            None => p.send(&color("&cYou have no home: /sethome first.")),
        }
    }
}

const GAME_MODES: [(&str, GameMode); 4] = [
    ("survival", GameMode::Survival),
    ("creative", GameMode::Creative),
    ("adventure", GameMode::Adventure),
    ("spectator", GameMode::Spectator),
];

impl Plugin for Essentials {
    fn enable() -> Result<Self, String> {
        let config = data::simple_config(&data::config("config.yml").unwrap_or_default());
        let spawn = data::read("spawn.txt")
            .and_then(|l| parse_location(&l))
            .unwrap_or_else(|| Location::new("minecraft:overworld", 0.5, 64.0, 0.5));
        let homes = data::read("homes.txt")
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (id, rest) = l.split_once(' ')?;
                Some((id.to_owned(), parse_location(rest)?))
            })
            .collect();
        for kind in [
            EventKind::PlayerJoin,
            EventKind::PlayerQuit,
            EventKind::PlayerChat,
            EventKind::InventoryClick,
            EventKind::InventoryClose,
        ] {
            listen(kind, Priority::Normal);
        }
        // Protection decides early; others may still cancel later.
        listen(EventKind::BlockBreak, Priority::Low);
        listen(EventKind::BlockPlace, Priority::Low);
        listen(EventKind::EntityDamage, Priority::Normal);
        let every: u64 = config
            .get("announce-every")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if every > 0 {
            timer::<Essentials>(every, every, |p| {
                server::broadcast(&color(p.get("announcement")), None);
            });
        }
        // The sidebars follow who is on, once a second.
        timer::<Essentials>(20, 20, |p| {
            for player in players::online() {
                p.sidebar(&player);
            }
        });
        server::console(&format!(
            "Essentials enabled: spawn at {}",
            location_line(&spawn)
        ));
        Ok(Essentials {
            config,
            spawn,
            homes,
            menus: BTreeMap::new(),
            welcome_bar: None,
        })
    }

    fn disable(&mut self) {
        self.save_homes();
        for gui in self.menus.values() {
            gui.destroy();
        }
        if let Some(bar) = self.welcome_bar {
            bossbars::remove(bar);
        }
    }

    fn on_event(&mut self, _priority: Priority, event: &mut Event) {
        match event {
            Event::PlayerJoin(e) => {
                e.message = Some(color(
                    &self.get("join-message").replace("{player}", &e.player.name),
                ));
                if e.first_join {
                    e.player.teleport(&self.spawn);
                }
                e.player
                    .title(&color("&6Welcome"), &color(&format!("&e{}", e.player.name)));
                let bar = *self.welcome_bar.get_or_insert_with(|| {
                    bossbars::create(
                        &color("&eWelcome to Horizon"),
                        bossbars::BarColor::Yellow,
                        bossbars::BarStyle::Solid,
                        1.0,
                    )
                });
                bossbars::add_player(bar, &e.player.id);
                let id = e.player.id.clone();
                later::<Essentials>(100, move |p| {
                    if let Some(bar) = p.welcome_bar {
                        bossbars::remove_player(bar, &id);
                    }
                });
                self.sidebar(&e.player);
            }
            Event::PlayerQuit(e) => {
                e.message = Some(color(
                    &self.get("quit-message").replace("{player}", &e.player.name),
                ));
                if let Some(gui) = self.menus.remove(&e.player.id) {
                    gui.destroy();
                }
            }
            Event::PlayerChat(e) => {
                e.format = color(self.get("chat-format"));
            }
            Event::BlockBreak(e) => {
                if self.protected(&e.block) && !e.player.has_permission("essentials.build") {
                    e.cancelled = true;
                    e.player.action_bar(&color("&cThe spawn is protected."));
                }
            }
            Event::BlockPlace(e) => {
                if self.protected(&e.block) && !e.player.has_permission("essentials.build") {
                    e.cancelled = true;
                    e.player.action_bar(&color("&cThe spawn is protected."));
                }
            }
            Event::EntityDamage(e) => {
                // No fall damage at the spawn.
                if e.cause == "minecraft:fall"
                    && let Some(id) = &e.entity.player
                    && let Some(p) = players::get(id)
                    && p.location.distance(&self.spawn) <= self.radius()
                {
                    e.cancelled = true;
                }
            }
            Event::InventoryClick(e) => {
                let Some(gui) = self.menus.get(&e.player.id).copied() else {
                    return;
                };
                if !gui.is(&e.view) {
                    return;
                }
                e.cancelled = true;
                let action = e.current.as_ref().and_then(|s| s.tag("action"));
                if let Some(action) = action {
                    let player = e.player.clone();
                    self.menu_action(&player, &action);
                }
            }
            Event::InventoryClose(e) => {
                if let Some(gui) = self.menus.get(&e.player.id).copied()
                    && gui.is(&e.view)
                {
                    self.menus.remove(&e.player.id);
                    gui.destroy();
                }
            }
            _ => {}
        }
    }

    fn on_command(&mut self, sender: &Sender, command: &str, label: &str, args: &[String]) -> bool {
        let player = sender.player().cloned();
        let need_player = |sender: &Sender| {
            sender.send(&color("&cOnly players can do that."));
            true
        };
        match command {
            "spawn" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                p.teleport(&self.spawn);
                p.send(&color("&aTeleported to the spawn."));
                true
            }
            "setspawn" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                self.spawn = p.location.clone();
                let _ = data::write("spawn.txt", &location_line(&self.spawn));
                worlds::set_spawn(&self.spawn);
                p.send(&color("&aSpawn set."));
                true
            }
            "heal" => {
                let target = match args.first() {
                    Some(name) => players::by_name(name),
                    None => player,
                };
                let Some(t) = target else {
                    sender.send(&color("&cNo such player."));
                    return true;
                };
                players::set_health(&t.id, t.max_health);
                players::set_food(&t.id, 20);
                players::clear_effects(&t.id);
                t.send(&color("&aYou were healed."));
                if sender.name() != t.name {
                    sender.send(&color(&format!("&aHealed {}.", t.name)));
                }
                true
            }
            "fly" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                let on = !p.allow_flight;
                players::set_allow_flight(&p.id, on);
                p.send(&color(if on {
                    "&aYou can fly."
                } else {
                    "&cFlight off."
                }));
                true
            }
            "gamemode" => {
                let mode = args
                    .first()
                    .and_then(|a| match a.to_ascii_lowercase().as_str() {
                        "0" | "s" | "survival" => Some(GameMode::Survival),
                        "1" | "c" | "creative" => Some(GameMode::Creative),
                        "2" | "a" | "adventure" => Some(GameMode::Adventure),
                        "3" | "sp" | "spectator" => Some(GameMode::Spectator),
                        _ => None,
                    });
                let Some(mode) = mode else {
                    return false;
                };
                let target = match args.get(1) {
                    Some(name) => players::by_name(name),
                    None => player,
                };
                let Some(t) = target else {
                    sender.send(&color("&cNo such player."));
                    return true;
                };
                t.set_game_mode(mode);
                sender.send(&color(&format!("&aSet {}'s game mode ({label}).", t.name)));
                true
            }
            "broadcast" => {
                if args.is_empty() {
                    return false;
                }
                let message = color(&format!("&6[Broadcast] &f{}", args.join(" ")));
                server::broadcast(&message, None);
                true
            }
            "menu" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                self.open_menu(&p);
                true
            }
            "sethome" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                self.homes.insert(p.id.clone(), p.location.clone());
                self.save_homes();
                p.send(&color("&aHome set."));
                true
            }
            "home" => {
                let Some(p) = player else {
                    return need_player(sender);
                };
                self.go_home(&p);
                true
            }
            "tps" => {
                let (a, b, c) = server::tps();
                sender.send(&color(&format!(
                    "&6TPS from last 1m, 5m, 15m: &a{a:.1}, {b:.1}, {c:.1} &7({:.2} ms)",
                    server::mspt()
                )));
                true
            }
            _ => false,
        }
    }

    fn on_tab_complete(&mut self, _sender: &Sender, command: &str, args: &[String]) -> Vec<String> {
        let last = args
            .last()
            .map(|a| a.to_ascii_lowercase())
            .unwrap_or_default();
        match (command, args.len()) {
            ("gamemode", 1) => GAME_MODES
                .iter()
                .map(|(n, _)| (*n).to_owned())
                .filter(|n| n.starts_with(&last))
                .collect(),
            // The default: names of the players online.
            _ => Vec::new(),
        }
    }
}

horizon_mod_sdk::export_plugin!(Essentials);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_round_trip_through_their_line() {
        let l = Location::new("minecraft:the_nether", 1.5, 70.0, -3.25).with_rotation(90.0, -10.0);
        assert_eq!(parse_location(&location_line(&l)), Some(l));
        assert_eq!(parse_location("nonsense"), None);
    }
}
