//! A test plugin for `tests/plugins.rs`: what it does depends on the role
//! its package carries in `defaults/role.txt` (`alpha`, `beta` or `gamma`),
//! so one component tests listeners' priorities, cancellation, plugins
//! calling each other, dependencies, commands, permissions, tasks, files and
//! a plugin that crashes.
use horizon_mod_sdk::plugin::*;

struct Probe {
    role: String,
    timer_runs: u32,
    timer: Option<u32>,
}

impl Probe {
    fn alpha(&self) -> bool {
        self.role == "alpha"
    }
}

impl Plugin for Probe {
    fn enable() -> Result<Self, String> {
        let role = files::bundled("role.txt")
            .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
            .unwrap_or_default();
        match role.as_str() {
            "alpha" => {
                listen(EventKind::PlayerChat, Priority::Lowest);
                listen(EventKind::PlayerChat, Priority::Monitor);
                listen(EventKind::BlockBreak, Priority::Normal);
                listen(EventKind::PluginCall, Priority::Normal);
                for name in [
                    "alpha-ping",
                    "alpha-call",
                    "alpha-perm",
                    "alpha-has",
                    "alpha-task",
                    "alpha-file",
                    "alpha-panic",
                    "alpha-spin",
                    "alpha-unregister",
                ] {
                    commands::register(&commands::CommandSpec {
                        name: name.into(),
                        description: String::new(),
                        usage: String::new(),
                        aliases: if name == "alpha-ping" {
                            vec!["ap".into()]
                        } else {
                            Vec::new()
                        },
                        permission: None,
                        permission_message: None,
                    })?;
                }
                permissions::declare(
                    "probe.parent",
                    permissions::PermissionDefault::Never,
                    "",
                    &[("probe.child".into(), true)],
                );
            }
            "beta" => {
                listen_uncancelled(EventKind::PlayerChat, Priority::Normal);
                listen(EventKind::PlayerChat, Priority::High);
                listen_uncancelled(EventKind::BlockBreak, Priority::High);
                listen(EventKind::PluginCall, Priority::Normal);
                server::console(&format!(
                    "beta enabled after alpha={}",
                    server::plugin_enabled("alpha")
                ));
                // Taken by alpha: refused.
                let taken = commands::register(&commands::CommandSpec {
                    name: "alpha-ping".into(),
                    description: String::new(),
                    usage: String::new(),
                    aliases: Vec::new(),
                    permission: None,
                    permission_message: None,
                });
                server::console(&format!("beta alpha-ping: {}", taken.is_err()));
            }
            other => return Err(format!("no role {other:?}")),
        }
        Ok(Probe {
            role,
            timer_runs: 0,
            timer: None,
        })
    }

    fn on_event(&mut self, priority: Priority, event: &mut Event) {
        match event {
            Event::PlayerChat(e) if self.alpha() && priority == Priority::Lowest => {
                e.message.push_str("[a]");
            }
            Event::PlayerChat(e) if self.alpha() && priority == Priority::Monitor => {
                server::console(&format!(
                    "monitor saw {} cancelled={}",
                    e.message, e.cancelled
                ));
                // A monitor's change is ignored.
                e.message = "changed by monitor".into();
                e.cancelled = false;
            }
            Event::PlayerChat(e) if priority == Priority::Normal => e.message.push_str("[b]"),
            Event::PlayerChat(e) if priority == Priority::High => {
                if e.message.contains("secret") {
                    e.cancelled = true;
                }
            }
            Event::BlockBreak(e) if self.alpha() => {
                if e.block.state.contains("diamond") {
                    e.cancelled = true;
                }
            }
            Event::BlockBreak(e) => {
                server::console(&format!("beta saw break of {}", e.block.state))
            }
            Event::PluginCall(e) if self.alpha() => {
                server::console(&format!(
                    "alpha got {} from {}: {}",
                    e.channel,
                    e.from,
                    String::from_utf8_lossy(&e.data)
                ));
            }
            Event::PluginCall(e) => {
                let upper = String::from_utf8_lossy(&e.data).to_uppercase();
                server::send_to_plugin(&e.from, "reply", upper.as_bytes());
            }
            _ => {}
        }
    }

    fn on_command(&mut self, sender: &Sender, command: &str, label: &str, args: &[String]) -> bool {
        let id = sender.player().map(|p| p.id.clone()).unwrap_or_default();
        match command {
            "alpha-ping" => sender.send(&format!("pong {label} {}", args.len())),
            "alpha-call" => {
                let sent = server::send_to_plugin("beta", "echo", args.join(" ").as_bytes());
                let missing = server::send_to_plugin("nobody", "echo", b"x");
                sender.send(&format!("sent {sent} {missing}"));
            }
            "alpha-perm" => {
                permissions::set(&id, &args[0], args.get(1).is_none_or(|v| v == "true"));
                sender.send("set");
            }
            "alpha-has" => sender.send(&format!("{} {}", args[0], permissions::has(&id, &args[0]))),
            "alpha-task" => {
                later::<Probe>(5, |_| server::console("task once"));
                let t = timer::<Probe>(2, 2, |p| {
                    p.timer_runs += 1;
                    server::console(&format!("timer {}", p.timer_runs));
                    if p.timer_runs == 3
                        && let Some(t) = p.timer
                    {
                        cancel(t);
                    }
                });
                self.timer = Some(t);
                let id = scheduler::run_later(1, None);
                sender.send(&format!("queued {}", scheduler::is_queued(id)));
            }
            "alpha-file" => {
                let escape = files::write("../escape.txt", b"no").is_err();
                let nested = files::write("a/b.txt", b"yes").is_ok();
                let _ = files::append("a/b.txt", b"!");
                let read = data::read("a/b.txt").unwrap_or_default();
                let listed = files::list("a").join(",");
                sender.send(&format!("{escape} {nested} {read} {listed}"));
            }
            "alpha-panic" => panic!("on purpose"),
            "alpha-spin" => {
                let mut n: u64 = 0;
                loop {
                    n = n.wrapping_add(std::hint::black_box(1));
                    if n == u64::MAX {
                        break;
                    }
                }
            }
            "alpha-unregister" => {
                commands::unregister("alpha-ping");
                sender.send("gone");
            }
            _ => return false,
        }
        true
    }

    fn on_task(&mut self, task: u32) {
        server::console(&format!("raw task {}", task > 0));
    }
}

horizon_mod_sdk::export_plugin!(Probe);
