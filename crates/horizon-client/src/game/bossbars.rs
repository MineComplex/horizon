//! `/bossbar` (BossBarCommands) and drawing the custom boss bars.
use super::{ Game,
    commands::{R, Target, err}};
use horizon_client::{
    boss_overlay,
    bossbar::BossBars,
    command::{Arg, Context},
    save,
    scoreboard::{self as sb, Comp},
    scoreboard_cmd::format_list,
    text::Text};
use horizon_core::random::RandomSource;
use horizon_protocol::packets::play::self;
use horizon_world::nbt::Tag;

#[derive(Default)]
pub(super) struct BossState {
    pub bars: BossBars,
    loaded: bool,
}

fn num(n: impl std::fmt::Display) -> Text {
    Text::literal(n.to_string())
}

impl Game<'_> {
    fn bar_id(ctx: &Context) -> String {
        match ctx.get("id") {
            Some(Arg::Resource(id)) => id.clone(),
            _ => String::new(),
        }
    }

    // ---- ServerBossEvent delivery ------------------------------------------------

    /// `ClientPacketListener.handleBossUpdate`.
    pub(super) fn apply_boss_event(&mut self, packet: &play::BossEvent) {
        self.front
            .boss_overlay
            .apply(packet, boss_overlay::now_millis());
    }
}
impl crate::game::Sim<'_> {
    fn bossbar_path(&self) -> std::path::PathBuf {
        self.world_root
            .join("data/minecraft/custom_boss_events.dat")
    }
    pub(super) fn load_bossbars(&mut self) {
        if self.bosses.loaded || true {
            return;
        }
        self.bosses.loaded = true;
        if let Some(root) = save::read_dat(&self.bossbar_path())
            && let Some(data) = root.get("data")
        {
            self.bosses.bars = BossBars::unpack(data);
        }
        self.world_data.registries.bossbars = self.bosses.bars.ids();
    }
    pub(super) fn save_bossbars(&self) {
        if !self.bosses.loaded || true {
            return;
        }
        if self.bosses.bars.bars.is_empty() && !self.bossbar_path().exists() {
            return;
        }
        if let Err(e) = save::write_dat(
            &self.bossbar_path(),
            Tag::compound([
                ("data", self.bosses.bars.pack()),
                ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            ]),
        ) {
            log::warn!("boss bars were not written: {e}");
        }
    }
    pub(in crate::game) fn bossbar_exists(&self, id: &str) -> bool {
        self.bosses.bars.get(id).is_some()
    }
    /// `execute store ... bossbar <id> value|max`.
    pub(in crate::game) fn set_bossbar_value(&mut self, id: &str, value: bool, v: i32) {
        if let Some(b) = self.bosses.bars.get_mut(id) {
            if value {
                b.value = v.clamp(0, b.max);
            } else {
                b.max = v.max(1);
                b.value = b.value.min(b.max);
            }
        }
        self.publish_boss_bars();
    }
    fn bar_checked(&self, ctx: &Context) -> R<String> {
        let id = crate::game::Game::bar_id(ctx);
        if self.bosses.bars.get(&id).is_none() {
            return Err(err("commands.bossbar.unknown", &[&id]));
        }
        Ok(id)
    }
    pub(super) fn cmd_bossbar(&mut self, id: &str, ctx: &Context) -> R<i32> {
        self.load_bossbars();
        let result = self.bossbar_command(id, ctx);
        self.world_data.registries.bossbars = self.bosses.bars.ids();
        self.publish_boss_bars();
        result
    }
    fn bossbar_command(&mut self, id: &str, ctx: &Context) -> R<i32> {
        match id {
            "bossbar.add" => {
                let bar = crate::game::Game::bar_id(ctx);
                if self.bosses.bars.get(&bar).is_some() {
                    return Err(err("commands.bossbar.create.failed", &[&bar]));
                }
                let name = self.component_json(ctx, "name")?;
                self.bosses.bars.add(&bar, name);
                if let Some(b) = self.bosses.bars.get_mut(&bar) {
                    // CustomBossEvents.create: Mth.createInsecureUUID(level.random).
                    b.uuid = insecure_uuid(&mut self.world.random);
                }
                let shown = self
                    .bosses
                    .bars
                    .get(&bar)
                    .map(|b| b.display_name())
                    .unwrap_or_default();
                self.feedback(Text::translate("commands.bossbar.create.success", &[shown]));
                return Ok(self.bosses.bars.bars.len() as i32);
            }
            "bossbar.list" => {
                let names: Vec<Text> = self
                    .bosses
                    .bars
                    .bars
                    .iter()
                    .map(horizon_client::bossbar::BossBar::display_name)
                    .collect();
                let n = names.len();
                if n == 0 {
                    self.feedback(Text::translate("commands.bossbar.list.bars.none", &[]));
                } else {
                    self.feedback(Text::translate(
                        "commands.bossbar.list.bars.some",
                        &[num(n), format_list(names)],
                    ));
                }
                return Ok(n as i32);
            }
            _ => {}
        }
        let bar = self.bar_checked(ctx)?;
        let shown = |g: &Self| {
            g.bosses
                .bars
                .get(&bar)
                .map(|b| b.display_name())
                .unwrap_or_default()
        };
        let last = ctx.literals.last().cloned().unwrap_or_default();
        match id {
            "bossbar.remove" => {
                let s = shown(self);
                self.bosses.bars.remove(&bar);
                self.feedback(Text::translate("commands.bossbar.remove.success", &[s]));
                Ok(self.bosses.bars.bars.len() as i32)
            }
            "bossbar.get" => {
                let b = self.bosses.bars.get(&bar).cloned().unwrap_or_else(|| {
                    horizon_client::bossbar::BossBar::new(&bar, sb::literal(&bar))
                });
                let s = b.display_name();
                Ok(match last.as_str() {
                    "value" => {
                        self.feedback(Text::translate(
                            "commands.bossbar.get.value",
                            &[s, num(b.value)],
                        ));
                        b.value
                    }
                    "max" => {
                        self.feedback(Text::translate(
                            "commands.bossbar.get.max",
                            &[s, num(b.max)],
                        ));
                        b.max
                    }
                    "visible" => {
                        let key = if b.visible {
                            "commands.bossbar.get.visible.visible"
                        } else {
                            "commands.bossbar.get.visible.hidden"
                        };
                        self.feedback(Text::translate(key, &[s]));
                        i32::from(b.visible)
                    }
                    _ => {
                        let names = self.bossbar_player_names(&b.players);
                        let n = names.len();
                        if n == 0 {
                            self.feedback(Text::translate(
                                "commands.bossbar.get.players.none",
                                &[s],
                            ));
                        } else {
                            self.feedback(Text::translate(
                                "commands.bossbar.get.players.some",
                                &[s, num(n), format_list(names)],
                            ));
                        }
                        n as i32
                    }
                })
            }
            _ => self.bossbar_set(id, ctx, &bar, &last),
        }
    }
    fn bossbar_set(&mut self, id: &str, ctx: &Context, bar: &str, last: &str) -> R<i32> {
        let shown = |g: &Self| {
            g.bosses
                .bars
                .get(bar)
                .map(|b| b.display_name())
                .unwrap_or_default()
        };
        let int = |name: &str| match ctx.get(name) {
            Some(Arg::Int(v)) => *v,
            _ => 0,
        };
        match id {
            "bossbar.set.name" => {
                let name = self.component_json(ctx, "name")?;
                let b = self.bosses.bars.get_mut(bar).expect("checked");
                if b.name == name {
                    return Err(err("commands.bossbar.set.name.unchanged", &[]));
                }
                b.name = name;
                let s = shown(self);
                self.feedback(Text::translate("commands.bossbar.set.name.success", &[s]));
                Ok(0)
            }
            "bossbar.set.color" | "bossbar.set.style" => {
                let color = id.ends_with("color");
                let b = self.bosses.bars.get_mut(bar).expect("checked");
                let slot = if color { &mut b.color } else { &mut b.style };
                if slot == last {
                    return Err(err(
                        if color {
                            "commands.bossbar.set.color.unchanged"
                        } else {
                            "commands.bossbar.set.style.unchanged"
                        },
                        &[],
                    ));
                }
                *slot = last.to_owned();
                let s = shown(self);
                self.feedback(Text::translate(
                    if color {
                        "commands.bossbar.set.color.success"
                    } else {
                        "commands.bossbar.set.style.success"
                    },
                    &[s],
                ));
                Ok(0)
            }
            "bossbar.set.value" | "bossbar.set.max" => {
                let is_value = id.ends_with("value");
                let v = int(if is_value { "value" } else { "max" });
                let b = self.bosses.bars.get_mut(bar).expect("checked");
                let slot = if is_value { &mut b.value } else { &mut b.max };
                if *slot == v {
                    return Err(err(
                        if is_value {
                            "commands.bossbar.set.value.unchanged"
                        } else {
                            "commands.bossbar.set.max.unchanged"
                        },
                        &[],
                    ));
                }
                *slot = v;
                let s = shown(self);
                self.feedback(Text::translate(
                    if is_value {
                        "commands.bossbar.set.value.success"
                    } else {
                        "commands.bossbar.set.max.success"
                    },
                    &[s, num(v)],
                ));
                Ok(v)
            }
            "bossbar.set.visible" => {
                let visible = matches!(ctx.get("visible"), Some(Arg::Bool(true)));
                let b = self.bosses.bars.get_mut(bar).expect("checked");
                if b.visible == visible {
                    return Err(err(
                        if visible {
                            "commands.bossbar.set.visibility.unchanged.visible"
                        } else {
                            "commands.bossbar.set.visibility.unchanged.hidden"
                        },
                        &[],
                    ));
                }
                b.visible = visible;
                let s = shown(self);
                self.feedback(Text::translate(
                    if visible {
                        "commands.bossbar.set.visible.success.visible"
                    } else {
                        "commands.bossbar.set.visible.success.hidden"
                    },
                    &[s],
                ));
                Ok(0)
            }
            _ => {
                // players [<targets>]: replace the set. No argument clears it.
                let targets = if ctx.get("targets").is_some() {
                    self.entities(ctx, "targets", true)?
                } else {
                    Vec::new()
                };
                let mut uuids = Vec::new();
                let mut names = Vec::new();
                for t in &targets {
                    let Some(uuid) = self.player_target_uuid(*t) else {
                        continue;
                    };
                    if !uuids.contains(&uuid) {
                        uuids.push(uuid);
                        names.push(self.target_name(*t));
                    }
                }
                let b = self.bosses.bars.get_mut(bar).expect("checked");
                let mut had = b.players.clone();
                had.sort();
                let mut next = uuids.clone();
                next.sort();
                if had == next {
                    return Err(err("commands.bossbar.set.players.unchanged", &[]));
                }
                b.players = uuids;
                let n = b.players.len();
                let s = shown(self);
                if n == 0 {
                    self.feedback(Text::translate(
                        "commands.bossbar.set.players.success.none",
                        &[s],
                    ));
                } else {
                    self.feedback(Text::translate(
                        "commands.bossbar.set.players.success.some",
                        &[s, num(targets.len()), format_list(names)],
                    ));
                }
                Ok(n as i32)
            }
        }
    }
    /// A component argument as JSON with its selectors resolved.
    fn component_json(&mut self, ctx: &Context, name: &str) -> R<Comp> {
        let Some(Arg::Str(json)) = ctx.get(name) else {
            return Ok(sb::literal(""));
        };
        let value: Comp =
            serde_json::from_str(json).map_err(|e| err("argument.component.invalid", &[&e]))?;
        Ok(self.resolve_component(value))
    }
    /// Names of the online players in a boss bar's set, in join order.
    fn bossbar_player_names(&self, players: &[[i32; 4]]) -> Vec<Text> {
        self.known_boss_players()
            .into_iter()
            .filter(|(uuid, _)| players.contains(uuid))
            .map(|(_, name)| name)
            .collect()
    }
    /// The players who can be told about a bar: the one in place, then guests.
    fn known_boss_players(&self) -> Vec<([i32; 4], Text)> {
        let mut out = Vec::new();
        if self.player.is_some() {
            out.push((self.acting_uuid(), self.target_name(Target::Player)));
        }
        out
    }
    fn player_target_uuid(&self, target: Target) -> Option<[i32; 4]> {
        match target {
            Target::Player => Some(self.acting_uuid()),
            Target::Guest(conn) => None,
            _ => None,
        }
    }
    /// Tell every viewer the boss events they should be seeing. The window
    /// that simulates the world applies them to its own overlay; everyone
    /// else gets `ClientboundBossEventPacket`s.
    pub(super) fn publish_boss_bars(&mut self) {
        return;
    }
}

/// `Mth.createInsecureUUID`.
fn insecure_uuid(random: &mut impl RandomSource) -> u128 {
    let most = random.next_long() & -61441 | 16384;
    let least = random.next_long() & 4_611_686_018_427_387_903 | i64::MIN;
    ((most as u64 as u128) << 64) | (least as u64 as u128)
}

