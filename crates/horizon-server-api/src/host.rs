//! The plugin manager (`SimplePluginManager`, `CraftScheduler`): plugins in
//! their load order, listeners by priority, commands and tasks.
use crate::{
    Plugin, PluginDescription,
    command::{CommandContext, CommandSender, Completer, Executor, Registered},
    event::{Event, EventPriority},
    server::{Action, Server, View},
};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
};

type Handler = Box<dyn FnMut(&mut dyn Any, &mut Server<'_>) + Send>;

struct Listener {
    plugin: usize,
    priority: EventPriority,
    ignore_cancelled: bool,
    handler: Handler,
}

enum Repeat {
    Once(Box<dyn FnOnce(&mut Server<'_>) + Send>),
    Every(u64, Box<dyn FnMut(&mut Server<'_>) -> bool + Send>),
}

struct Task {
    id: u64,
    plugin: usize,
    due: u64,
    run: Repeat,
}

/// What plugins registered.
#[derive(Default)]
struct Registry {
    listeners: HashMap<TypeId, Vec<Listener>>,
    commands: Vec<Registered>,
    tasks: Vec<Task>,
    next_task: u64,
    /// The tick tasks count from.
    now: u64,
}

/// A plugin's handle on the server while it enables: its listeners,
/// commands and tasks (`getServer().getPluginManager()`, `getCommand`,
/// `getScheduler()`).
pub struct Plugins<'a> {
    registry: &'a mut Registry,
    plugin: usize,
    name: &'a str,
}
impl Plugins<'_> {
    /// This plugin's name.
    pub fn name(&self) -> &str {
        self.name
    }
    /// `registerEvent`: `handler` runs for every `E`, in `priority`'s turn;
    /// with `ignore_cancelled` not once a listener before it cancelled the event.
    pub fn listen<E: Event>(
        &mut self,
        priority: EventPriority,
        ignore_cancelled: bool,
        mut handler: impl FnMut(&mut E, &mut Server<'_>) + Send + 'static,
    ) {
        let listeners = self
            .registry
            .listeners
            .entry(TypeId::of::<E>())
            .or_default();
        listeners.push(Listener {
            plugin: self.plugin,
            priority,
            ignore_cancelled,
            handler: Box::new(move |event, server| {
                if let Some(event) = event.downcast_mut::<E>() {
                    handler(event, server);
                }
            }),
        });
        // Stable: same priority, registration order.
        listeners.sort_by_key(|l| l.priority);
    }
    /// A command anyone may run (`plugin.yml`'s `commands`, `setExecutor`).
    /// The executor returns false to show the usage.
    pub fn command(
        &mut self,
        name: &str,
        description: &str,
        usage: Option<&str>,
        executor: impl FnMut(&mut CommandContext<'_, '_>) -> bool + Send + 'static,
    ) -> CommandBuilder<'_> {
        self.registry.commands.push(Registered {
            plugin: self.plugin,
            name: name.to_ascii_lowercase(),
            aliases: Vec::new(),
            description: description.to_owned(),
            op_level: None,
            usage: usage.map(str::to_owned),
            executor: Box::new(executor) as Executor,
            completer: None,
        });
        let last = self.registry.commands.len() - 1;
        CommandBuilder {
            command: &mut self.registry.commands[last],
        }
    }
    /// `runTaskLater`: `task` runs once, `delay` ticks from now. Its id.
    pub fn run_later(
        &mut self,
        delay: u64,
        task: impl FnOnce(&mut Server<'_>) + Send + 'static,
    ) -> u64 {
        self.schedule(delay, Repeat::Once(Box::new(task)))
    }
    /// `runTaskTimer`: `task` runs `delay` ticks from now, then every
    /// `period` ticks while it returns true.
    pub fn run_timer(
        &mut self,
        delay: u64,
        period: u64,
        task: impl FnMut(&mut Server<'_>) -> bool + Send + 'static,
    ) -> u64 {
        self.schedule(delay, Repeat::Every(period.max(1), Box::new(task)))
    }
    fn schedule(&mut self, delay: u64, run: Repeat) -> u64 {
        let r = &mut *self.registry;
        r.next_task += 1;
        r.tasks.push(Task {
            id: r.next_task,
            plugin: self.plugin,
            due: r.now + delay,
            run,
        });
        r.next_task
    }
}

/// A command being registered: aliases, permission, completions.
pub struct CommandBuilder<'a> {
    command: &'a mut Registered,
}
impl CommandBuilder<'_> {
    pub fn alias(self, alias: &str) -> Self {
        self.command.aliases.push(alias.to_ascii_lowercase());
        self
    }
    /// Only operators of at least this level (2 to 4) may run it.
    pub fn op(self, level: u8) -> Self {
        self.command.op_level = Some(level);
        self
    }
    /// `TabCompleter.onTabComplete`: the last argument's completions.
    pub fn complete(
        self,
        completer: impl FnMut(&CommandSender, &[String]) -> Vec<String> + Send + 'static,
    ) -> Self {
        self.command.completer = Some(Box::new(completer) as Completer);
        self
    }
}

struct Slot {
    plugin: Box<dyn Plugin>,
    description: PluginDescription,
    enabled: bool,
}

/// The server's plugins.
#[derive(Default)]
pub struct PluginHost {
    slots: Vec<Slot>,
    registry: Registry,
    /// What the host itself has to say (errors, enable lines), drained by
    /// the server into its log.
    log: Vec<String>,
}

/// Bukkit's reply to a command its sender may not run.
pub const NO_PERMISSION: &str = "§cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.";

impl PluginHost {
    /// The plugins in load order: a plugin after those it `depend`s on; one
    /// whose dependency is missing is left out (and logged).
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Self {
        let mut host = Self::default();
        let mut pending: Vec<(Box<dyn Plugin>, PluginDescription)> = plugins
            .into_iter()
            .map(|p| {
                let d = p.description();
                (p, d)
            })
            .collect();
        let names: Vec<String> = pending.iter().map(|(_, d)| d.name.clone()).collect();
        loop {
            let ready = pending.iter().position(|(_, d)| {
                d.depend
                    .iter()
                    .all(|dep| host.slots.iter().any(|s| s.description.name == *dep))
            });
            let Some(i) = ready else { break };
            let (plugin, description) = pending.remove(i);
            host.slots.push(Slot {
                plugin,
                description,
                enabled: false,
            });
        }
        for (_, d) in pending {
            let missing: Vec<&String> =
                d.depend.iter().filter(|dep| !names.contains(dep)).collect();
            host.log.push(format!(
                "Could not load '{}': unknown dependencies {missing:?} (or a dependency loop)",
                d.name
            ));
        }
        host
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    /// The plugins: name, version, enabled.
    pub fn list(&self) -> Vec<(&str, &str, bool)> {
        self.slots
            .iter()
            .map(|s| {
                (
                    s.description.name.as_str(),
                    s.description.version.as_str(),
                    s.enabled,
                )
            })
            .collect()
    }
    /// `/plugins`' line: `Plugins (2): §aA§f, §aB`.
    pub fn list_text(&self) -> String {
        let names: Vec<String> = self
            .slots
            .iter()
            .map(|s| {
                let colour = if s.enabled { "§a" } else { "§c" };
                format!("{colour}{}§f", s.description.name)
            })
            .collect();
        format!("Plugins ({}): {}", names.len(), names.join(", "))
    }
    /// What the host has logged since the last call.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
    /// `onLoad`, in load order.
    pub fn load(&mut self) {
        for s in &mut self.slots {
            self.log.push(format!(
                "[{}] Loading {} v{}",
                s.description.name, s.description.name, s.description.version
            ));
            let plugin = &mut s.plugin;
            if catch_unwind(AssertUnwindSafe(|| plugin.on_load())).is_err() {
                self.log.push(format!(
                    "Error occurred while loading {}",
                    s.description.name
                ));
            }
        }
    }
    /// `onEnable`, in load order.
    pub fn enable(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate() {
            self.log.push(format!(
                "[{}] Enabling {} v{}",
                s.description.name, s.description.name, s.description.version
            ));
            let mut plugins = Plugins {
                registry: &mut self.registry,
                plugin: i,
                name: &s.description.name,
            };
            let plugin = &mut s.plugin;
            match catch_unwind(AssertUnwindSafe(|| plugin.on_enable(&mut plugins))) {
                Ok(()) => s.enabled = true,
                Err(_) => {
                    self.log.push(format!(
                        "Error occurred while enabling {} (Is it up to date?)",
                        s.description.name
                    ));
                    unregister(&mut self.registry, i);
                }
            }
        }
    }
    /// `onDisable`, in reverse load order; their listeners, commands and tasks go.
    pub fn disable(&mut self) {
        for (i, s) in self.slots.iter_mut().enumerate().rev() {
            if !s.enabled {
                continue;
            }
            self.log.push(format!(
                "[{}] Disabling {} v{}",
                s.description.name, s.description.name, s.description.version
            ));
            let plugin = &mut s.plugin;
            if catch_unwind(AssertUnwindSafe(|| plugin.on_disable())).is_err() {
                self.log.push(format!(
                    "Error occurred while disabling {}",
                    s.description.name
                ));
            }
            s.enabled = false;
            unregister(&mut self.registry, i);
        }
    }
    /// Whether any listener waits for `E` (the server skips building it otherwise).
    pub fn listens<E: Event>(&self) -> bool {
        self.registry
            .listeners
            .get(&TypeId::of::<E>())
            .is_some_and(|l| !l.is_empty())
    }
    /// `callEvent`: every listener of `E` in priority order. The changes
    /// they asked for, for the server to apply.
    pub fn call<E: Event>(&mut self, event: &mut E, view: &dyn View) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(listeners) = self.registry.listeners.get_mut(&TypeId::of::<E>()) else {
            return actions;
        };
        for l in listeners.iter_mut() {
            if l.ignore_cancelled && event.cancelled() {
                continue;
            }
            let name = &self.slots[l.plugin].description.name;
            let mut server = Server::new(view, name, &mut actions);
            let handler = &mut l.handler;
            let any: &mut dyn Any = event;
            if catch_unwind(AssertUnwindSafe(|| handler(any, &mut server))).is_err() {
                self.log
                    .push(format!("Could not pass event {} to {name}", E::NAME));
            }
        }
        actions
    }
    /// One server tick: the tasks that are due. The changes they asked for.
    pub fn tick(&mut self, view: &dyn View) -> Vec<Action> {
        let mut actions = Vec::new();
        let r = &mut self.registry;
        r.now += 1;
        let now = r.now;
        let mut i = 0;
        while i < r.tasks.len() {
            if r.tasks[i].due > now {
                i += 1;
                continue;
            }
            let task = r.tasks.remove(i);
            let name = &self.slots[task.plugin].description.name;
            let mut server = Server::new(view, name, &mut actions);
            match task.run {
                Repeat::Once(run) => {
                    if catch_unwind(AssertUnwindSafe(|| run(&mut server))).is_err() {
                        self.log.push(format!(
                            "Plugin {name} generated an exception while executing task {}",
                            task.id
                        ));
                    }
                }
                Repeat::Every(period, mut run) => {
                    match catch_unwind(AssertUnwindSafe(|| run(&mut server))) {
                        Ok(true) => r.tasks.insert(
                            i,
                            Task {
                                id: task.id,
                                plugin: task.plugin,
                                due: now + period,
                                run: Repeat::Every(period, run),
                            },
                        ),
                        Ok(false) => {}
                        Err(_) => self.log.push(format!(
                            "Plugin {name} generated an exception while executing task {}",
                            task.id
                        )),
                    }
                    i += 1;
                }
            }
        }
        actions
    }
    /// `cancelTask`.
    pub fn cancel_task(&mut self, id: u64) {
        self.registry.tasks.retain(|t| t.id != id);
    }
    /// The plugins' commands: name, aliases, description, and the operator
    /// level they need (for the command tree and `/help`).
    pub fn commands(&self) -> Vec<(String, Vec<String>, String, Option<u8>)> {
        self.registry
            .commands
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.aliases.clone(),
                    c.description.clone(),
                    c.op_level,
                )
            })
            .collect()
    }
    /// `dispatchCommand` for a plugin's command (without the slash), or
    /// `/plugins`: None when neither has it. The changes it asked for.
    pub fn command(
        &mut self,
        sender: CommandSender,
        line: &str,
        view: &dyn View,
    ) -> Option<Vec<Action>> {
        let line = line.trim_start_matches('/');
        let mut words = line.split(' ').filter(|w| !w.is_empty());
        let label = words.next()?.to_ascii_lowercase();
        let args: Vec<String> = words.map(str::to_owned).collect();
        let mut actions = Vec::new();
        // Paper's `plugin:command` form names the plugin too.
        let bare = label.rsplit(':').next().unwrap_or(&label).to_owned();
        if matches!(bare.as_str(), "plugins" | "pl") && !self.has_command(&bare) {
            let text = self.list_text();
            reply(
                &sender,
                &mut Server::new(view, "Server", &mut actions),
                &text,
            );
            return Some(actions);
        }
        let i = self
            .registry
            .commands
            .iter()
            .position(|c| c.name == bare || c.aliases.contains(&bare))?;
        let c = &mut self.registry.commands[i];
        let name = &self.slots[c.plugin].description.name;
        if label.contains(':') && !label.starts_with(&format!("{}:", name.to_ascii_lowercase())) {
            return None;
        }
        let mut server = Server::new(view, name, &mut actions);
        let level = match &sender {
            CommandSender::Console => 4,
            CommandSender::Player(p) => p.op_level,
        };
        if c.op_level.is_some_and(|needed| level < needed) {
            reply(&sender, &mut server, NO_PERMISSION);
            return Some(actions);
        }
        let usage = c.usage.clone();
        let executor = &mut c.executor;
        let mut ctx = CommandContext {
            sender: sender.clone(),
            label: bare.clone(),
            args,
            server: &mut server,
        };
        match catch_unwind(AssertUnwindSafe(|| executor(&mut ctx))) {
            Ok(true) => {}
            Ok(false) => {
                if let Some(usage) = usage {
                    reply(&sender, &mut server, &usage.replace("<command>", &bare));
                }
            }
            Err(_) => {
                reply(
                    &sender,
                    &mut server,
                    "§cAn internal error occurred while attempting to perform this command",
                );
                self.log.push(format!(
                    "Unhandled exception executing command '{bare}' in plugin {name}"
                ));
            }
        }
        Some(actions)
    }
    fn has_command(&self, name: &str) -> bool {
        self.registry
            .commands
            .iter()
            .any(|c| c.name == name || c.aliases.iter().any(|a| a == name))
    }
    /// `onTabComplete` for a plugin's command: the last argument's
    /// completions, or None when no plugin has the command.
    pub fn complete(&mut self, sender: &CommandSender, line: &str) -> Option<Vec<String>> {
        let line = line.trim_start_matches('/');
        let mut words: Vec<String> = line.split(' ').map(str::to_owned).collect();
        let label = words.remove(0).to_ascii_lowercase();
        let c = self
            .registry
            .commands
            .iter_mut()
            .find(|c| c.name == label || c.aliases.contains(&label))?;
        let last = words.last().cloned().unwrap_or_default();
        let all = match c.completer.as_mut() {
            Some(complete) => complete(sender, &words),
            None => Vec::new(),
        };
        Some(
            all.into_iter()
                .filter(|s| s.to_lowercase().starts_with(&last.to_lowercase()))
                .collect(),
        )
    }
}

fn reply(sender: &CommandSender, server: &mut Server<'_>, text: &str) {
    match sender {
        CommandSender::Console => server.log(text),
        CommandSender::Player(p) => server.send_message(p, text),
    }
}

fn unregister(r: &mut Registry, plugin: usize) {
    for l in r.listeners.values_mut() {
        l.retain(|l| l.plugin != plugin);
    }
    r.commands.retain(|c| c.plugin != plugin);
    r.tasks.retain(|t| t.plugin != plugin);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::{BlockBreakEvent, PlayerJoinEvent},
        server::{BlockPos, Location, PlayerRef},
    };

    struct Nobody;
    impl View for Nobody {
        fn players(&self) -> Vec<PlayerRef> {
            Vec::new()
        }
        fn block(&self, _: &str, _: BlockPos) -> Option<String> {
            None
        }
        fn tick(&self) -> u64 {
            0
        }
        fn world_name(&self) -> String {
            "world".into()
        }
        fn max_players(&self) -> usize {
            20
        }
        fn time(&self) -> i64 {
            0
        }
    }

    fn player(name: &str, op_level: u8) -> PlayerRef {
        PlayerRef {
            name: name.into(),
            uuid: 7,
            location: Location::default(),
            health: 20.0,
            game_mode: 0,
            op_level,
        }
    }

    struct Guard {
        name: &'static str,
        depend: Vec<String>,
    }
    impl Plugin for Guard {
        fn description(&self) -> PluginDescription {
            let mut d = PluginDescription::new(self.name, "1.0");
            d.depend = self.depend.clone();
            d
        }
        fn on_enable(&mut self, plugins: &mut Plugins) {
            if self.name != "Guard" {
                return;
            }
            plugins.listen(EventPriority::Low, false, |e: &mut BlockBreakEvent, _| {
                e.cancelled = e.block == "minecraft:bedrock";
            });
            // Watches only what went through.
            plugins.listen(
                EventPriority::Monitor,
                true,
                |e: &mut BlockBreakEvent, s| {
                    let p = e.player.clone();
                    s.send_message(&p, &format!("broke {}", e.block));
                },
            );
            plugins.listen(
                EventPriority::Lowest,
                false,
                |e: &mut PlayerJoinEvent, _| {
                    e.join_message = None;
                },
            );
            plugins
                .command("heal", "Heals you", Some("/<command> <player>"), |ctx| {
                    let Some(name) = ctx.args.first().cloned() else {
                        return false;
                    };
                    ctx.reply(&format!("healed {name}"));
                    true
                })
                .alias("h")
                .op(2)
                .complete(|_, _| vec!["Alex".into(), "Steve".into()]);
            plugins.run_timer(2, 3, {
                let mut runs = 0;
                move |s| {
                    runs += 1;
                    s.broadcast(&format!("run {runs}"));
                    runs < 2
                }
            });
        }
    }

    fn host() -> PluginHost {
        // Listed before what it depends on: it loads after it.
        let mut host = PluginHost::new(vec![
            Box::new(Guard {
                name: "Addon",
                depend: vec!["Guard".into()],
            }),
            Box::new(Guard {
                name: "Guard",
                depend: vec![],
            }),
            Box::new(Guard {
                name: "Orphan",
                depend: vec!["Missing".into()],
            }),
        ]);
        host.load();
        host.enable();
        host
    }

    #[test]
    fn plugins_load_after_their_dependencies() {
        let host = host();
        let names: Vec<&str> = host.list().iter().map(|p| p.0).collect();
        assert_eq!(names, ["Guard", "Addon"]);
        assert_eq!(host.list_text(), "Plugins (2): §aGuard§f, §aAddon§f");
    }

    #[test]
    fn listeners_run_by_priority_and_monitors_skip_cancelled_events() {
        let mut host = host();
        let event = |block: &str| BlockBreakEvent {
            player: player("Alex", 0),
            world: "minecraft:overworld".into(),
            pos: BlockPos::new(0, 0, 0),
            block: block.into(),
            drop_items: true,
            cancelled: false,
        };
        let mut bedrock = event("minecraft:bedrock");
        assert!(host.call(&mut bedrock, &Nobody).is_empty());
        assert!(bedrock.cancelled);
        let mut stone = event("minecraft:stone");
        let actions = host.call(&mut stone, &Nobody);
        assert!(!stone.cancelled);
        assert_eq!(
            actions,
            [Action::Message {
                to: 7,
                text: "broke minecraft:stone".into()
            }]
        );
        let mut join = PlayerJoinEvent {
            player: player("Alex", 0),
            join_message: Some("§eAlex joined the game".into()),
            first_join: true,
        };
        host.call(&mut join, &Nobody);
        assert_eq!(join.join_message, None);
        assert!(!host.listens::<crate::event::PlayerQuitEvent>());
    }

    #[test]
    fn commands_check_operators_show_usage_and_complete() {
        let mut host = host();
        let msg = |a: &[Action]| match a {
            [Action::Message { text, .. }] => text.clone(),
            other => format!("{other:?}"),
        };
        let not_op = CommandSender::Player(player("Alex", 0));
        assert_eq!(
            msg(&host.command(not_op, "heal Bob", &Nobody).unwrap()),
            NO_PERMISSION
        );
        let op = CommandSender::Player(player("Alex", 2));
        assert_eq!(
            msg(&host.command(op.clone(), "/h Bob", &Nobody).unwrap()),
            "healed Bob"
        );
        assert_eq!(
            msg(&host.command(op.clone(), "guard:heal", &Nobody).unwrap()),
            "/heal <player>"
        );
        assert!(host.command(op.clone(), "other:heal", &Nobody).is_none());
        assert!(
            host.command(op.clone(), "gamemode creative", &Nobody)
                .is_none()
        );
        assert_eq!(
            host.complete(&op, "heal St"),
            Some(vec!["Steve".to_owned()])
        );
        assert_eq!(
            msg(&host.command(op, "plugins", &Nobody).unwrap()),
            "Plugins (2): §aGuard§f, §aAddon§f"
        );
    }

    #[test]
    fn timers_run_when_due_until_they_stop() {
        let mut host = host();
        let mut runs = Vec::new();
        for _ in 0..10 {
            runs.extend(host.tick(&Nobody));
        }
        assert_eq!(
            runs,
            [
                Action::Broadcast("run 1".into()),
                Action::Broadcast("run 2".into())
            ]
        );
    }

    #[test]
    fn a_panicking_listener_is_logged_and_the_others_still_run() {
        struct Bad;
        impl Plugin for Bad {
            fn description(&self) -> PluginDescription {
                PluginDescription::new("Bad", "0")
            }
            fn on_enable(&mut self, plugins: &mut Plugins) {
                plugins.listen(
                    EventPriority::Normal,
                    false,
                    |_: &mut PlayerJoinEvent, _| panic!("boom"),
                );
                plugins.listen(EventPriority::High, false, |e: &mut PlayerJoinEvent, _| {
                    e.first_join = false;
                });
            }
        }
        let mut host = PluginHost::new(vec![Box::new(Bad)]);
        host.enable();
        host.take_log();
        let mut join = PlayerJoinEvent {
            player: player("Alex", 0),
            join_message: None,
            first_join: true,
        };
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        host.call(&mut join, &Nobody);
        std::panic::set_hook(hook);
        assert!(!join.first_join);
        assert_eq!(
            host.take_log(),
            ["Could not pass event PlayerJoinEvent to Bad"]
        );
    }
}
