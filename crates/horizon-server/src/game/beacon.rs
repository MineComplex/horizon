//! Beacons (`BeaconBlockEntity`, `BeaconMenu`, `BeaconScreen`,
//! `BeaconRenderer`): the beam sections tinted by stained glass, the pyramid
//! levels every 80 ticks, the primary/secondary effects for players in
//! range, the activate/ambient/deactivate sounds, the 230x219 menu with its
//! power buttons and payment slot, and the rotating, scrolling beam.
use super::stations::{INPUT, Kind};
use crate::hc::{containers::{Beacon, BlockEntity}, effects::EffectInstance, items::ItemStack, mobs::render::{DYE, DYE_NAMES}};
use horizon_protocol::packets::play::{self, Serverbound as S};

/// The beacon screen is 54 px wider than the 176-px menus it shares an origin with.
pub(super) const SHIFT: f32 = -27.0;
pub(super) const HEIGHT: f32 = 219.0;
/// `BEACON_EFFECTS` in order, with the pyramid level each needs.
pub(super) const EFFECTS: [(&str, i32); 6] = [
    ("speed", 1),
    ("haste", 1),
    ("resistance", 2),
    ("jump_boost", 2),
    ("strength", 3),
    ("regeneration", 4),
];
const PAYMENT: [&str; 5] = [
    "minecraft:iron_ingot",
    "minecraft:gold_ingot",
    "minecraft:emerald",
    "minecraft:diamond",
    "minecraft:netherite_ingot",
];
/// A power button: (effect index, x, y, primary) relative to the beacon's left edge.
fn buttons() -> Vec<(usize, f32, f32, bool)> {
    let mut out = Vec::new();
    let tiers: [&[usize]; 3] = [&[0, 1], &[2, 3], &[4]];
    for (tier, effects) in tiers.iter().enumerate() {
        let count = effects.len() as i32;
        let total = count * 22 + (count - 1) * 2;
        for (c, &e) in effects.iter().enumerate() {
            let x = 76 + c as i32 * 24 - total / 2;
            out.push((e, x as f32, 22.0 + tier as f32 * 25.0, true));
        }
    }
    // Tier 4: regeneration, then the upgrade (primary II) button.
    let total = 2 * 22 + 2;
    out.push((5, (167 - total / 2) as f32, 47.0, false));
    out
}
fn upgrade_button() -> (f32, f32) {
    let total = 2 * 22 + 2;
    ((167 + 24 - total / 2) as f32, 47.0)
}
fn inside(p: (f32, f32), x: f32, y: f32, w: f32, h: f32) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}
fn effect_id(name: &str) -> Option<&'static str> {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    EFFECTS.iter().map(|(e, _)| *e).find(|e| *e == n)
}
/// `ARGB.average`.
fn average(a: u32, b: u32) -> u32 {
    let ch = |c: u32, s: u32| (c >> s) & 255;
    [16, 8, 0]
        .iter()
        .fold(0, |acc, &s| acc | ((ch(a, s) + ch(b, s)) / 2) << s)
}
impl crate::game::Sim<'_> {
    /// The beam color of a `BeaconBeamBlock` (stained glass and panes, the beacon).
    fn beam_color(&self, name: &str) -> Option<u32> {
        if name == "minecraft:beacon" {
            return Some(DYE[0]);
        }
        let n = name.strip_prefix("minecraft:")?;
        let dye = n
            .strip_suffix("_stained_glass")
            .or_else(|| n.strip_suffix("_stained_glass_pane"))?;
        DYE_NAMES.iter().position(|d| *d == dye).map(|i| DYE[i])
    }
    /// `getHeight(WORLD_SURFACE)`: above the highest non-air block.
    pub(super) fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.world
            .world_surface(x, z, |id| self.res.facts().is_air(id))
    }
    /// `BeaconBlockEntity.tick` of the beacon at `key`.
    pub(super) fn tick_beacon_at(&mut self, key: (i32, i32, i32)) {
        let Some(BlockEntity::Beacon(mut b)) = self.world.block_entities.take(&key) else {
            return;
        };
        self.tick_beacon(key, &mut b);
        self.world.block_entities.put(key, BlockEntity::Beacon(b));
    }
    fn tick_beacon(&mut self, (x, y, z): (i32, i32, i32), b: &mut Beacon) {
        let mut check = if b.last_check_y < y {
            b.checking.clear();
            b.last_check_y = y - 1;
            y
        } else {
            b.last_check_y + 1
        };
        let surface = self.world_surface(x, z);
        let mut i = 0;
        while i < 10 && check <= surface {
            let state = self.world.block(x, check, z);
            let f = self.res.facts().get(state);
            if let Some(color) = self.beam_color(&f.name) {
                if b.checking.len() <= 1 {
                    b.checking.push((color, 1));
                } else if let Some(last) = b.checking.last_mut() {
                    if color == last.0 {
                        last.1 += 1;
                    } else {
                        let mixed = average(last.0, color);
                        b.checking.push((mixed, 1));
                    }
                }
            } else {
                let Some(last) = b
                    .checking
                    .last_mut()
                    .filter(|_| f.dampening < 15 || &*f.name == "minecraft:bedrock")
                else {
                    b.checking.clear();
                    b.last_check_y = surface;
                    break;
                };
                last.1 += 1;
            }
            check += 1;
            b.last_check_y += 1;
            i += 1;
        }
        let previous = b.levels;
        if self.world.game_time % 80 == 0 {
            if !b.sections.is_empty() {
                b.levels = self.beacon_base(x, y, z);
            }
            if b.levels > 0 && !b.sections.is_empty() {
                self.beacon_effects([x, y, z], b);
                self.beacon_sound([x, y, z], "minecraft:block.beacon.ambient");
            }
        }
        if b.last_check_y >= surface {
            b.last_check_y = super::MIN_Y - 1;
            b.sections = std::mem::take(&mut b.checking);
            let (was, is) = (previous > 0, b.levels > 0);
            if !was && is {
                self.beacon_sound([x, y, z], "minecraft:block.beacon.activate");
                let levels = b.levels;
                self.for_each_player(|g| g.beacon_constructed([x, y, z], levels));
            } else if was && !is {
                self.beacon_sound([x, y, z], "minecraft:block.beacon.deactivate");
            }
        }
    }
    /// `BeaconBlockEntity.tick` after activating: `CONSTRUCT_BEACON` for a
    /// player in the box `AABB(x, y, z, x, y - 4, z).inflate(10, 5, 10)`.
    fn beacon_constructed(&mut self, p: [i32; 3], levels: i32) {
        let Some(bb) = self
            .player
            .as_ref()
            .map(|pl| pl.dimensions.bounds(pl.position))
        else {
            return;
        };
        let (lo, hi) = (bb.min_position(), bb.max_position());
        let [x, y, z] = p.map(f64::from);
        if lo.x < x + 10.0
            && hi.x > x - 10.0
            && lo.y < y + 5.0
            && hi.y > y - 9.0
            && lo.z < z + 10.0
            && hi.z > z - 10.0
        {
            self.trigger_construct_beacon(levels);
        }
    }
    pub(super) fn beacon_sound(&mut self, p: [i32; 3], sound: &str) {
        let at = [
            f64::from(p[0]) + 0.5,
            f64::from(p[1]) + 0.5,
            f64::from(p[2]) + 0.5,
        ];
        self.sounds.play(sound, Some(at), 1.0, 1.0);
    }
    /// `updateBase`: complete layers of `beacon_base_blocks` below.
    fn beacon_base(&self, x: i32, y: i32, z: i32) -> i32 {
        let base = self
            .res
            .items()
            .block_tags
            .get("minecraft:beacon_base_blocks");
        let mut levels = 0;
        for step in 1..=4 {
            let ly = y - step;
            if ly < super::MIN_Y {
                break;
            }
            let ok = (x - step..=x + step).all(|lx| {
                (z - step..=z + step).all(|lz| {
                    let name = &self.res.facts().get(self.world.block(lx, ly, lz)).name;
                    base.is_some_and(|t| t.contains(&**name))
                })
            });
            if !ok {
                break;
            }
            levels = step;
        }
        levels
    }
    /// `applyEffects`: the powers for players within `levels * 10 + 10` (any height above).
    /// `BeaconBlockEntity.applyEffects`: every player in range.
    fn beacon_effects(&mut self, p: [i32; 3], b: &Beacon) {
        self.for_each_player(|g| g.beacon_effects_on_player(p, b));
    }
    fn beacon_effects_on_player(&mut self, p: [i32; 3], b: &Beacon) {
        let Some(primary) = b.primary.as_deref().and_then(effect_id) else {
            return;
        };
        let range = f64::from(b.levels * 10 + 10);
        let Some(pp) = self.player.as_ref().map(|p| p.position) else {
            return;
        };
        let inside = pp.x >= f64::from(p[0]) - range
            && pp.x <= f64::from(p[0] + 1) + range
            && pp.z >= f64::from(p[2]) - range
            && pp.z <= f64::from(p[2] + 1) + range
            && pp.y >= f64::from(p[1]) - range;
        if !inside {
            return;
        }
        let secondary = b.secondary.as_deref().and_then(effect_id);
        let amplifier = i32::from(b.levels >= 4 && secondary == Some(primary));
        let duration = (9 + b.levels * 2) * 20;
        let ambient = |id, amp| EffectInstance {
            ambient: true,
            ..EffectInstance::new(id, duration, amp)
        };
        self.potions.effects.add(ambient(primary, amplifier));
        if let Some(s) = secondary.filter(|&s| b.levels >= 4 && s != primary) {
            self.potions.effects.add(ambient(s, 0));
        }
    }
    /// Opening the menu shows the beacon's current powers.
    pub(super) fn beacon_opened(&mut self) {
        let Some(s) = self.station.as_ref().filter(|s| s.kind == Kind::Beacon) else {
            return;
        };
        let pos = s.pos;
        let (p, sec) = match self.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
            Some(BlockEntity::Beacon(b)) => (b.primary.clone(), b.secondary.clone()),
            _ => (None, None),
        };
        let index =
            |e: Option<String>| e.and_then(|e| EFFECTS.iter().position(|(n, _)| e.ends_with(n)));
        if let Some(s) = self.station.as_mut() {
            s.beacon = [index(p), index(sec)];
        }
    }
    fn beacon_levels(&self) -> i32 {
        let Some(s) = self.station.as_ref() else {
            return 0;
        };
        if let Some(levels) = s.beacon_levels {
            return levels;
        }
        let pos = s.pos;
        match self.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
            Some(BlockEntity::Beacon(b)) => b.levels,
            _ => 0,
        }
    }
    /// Clicks on the power, confirm and cancel buttons.
    /// The effect, upgrade, confirm and cancel buttons a controller can
    /// focus (GUI origin relative).
    pub(super) fn beacon_targets(&self) -> Vec<[f32; 4]> {
        let (ux, uy) = upgrade_button();
        buttons()
            .into_iter()
            .map(|(_, x, y, _)| (x, y))
            .chain([(ux, uy), (164.0, 107.0), (190.0, 107.0)])
            .map(|(x, y)| [x + SHIFT, y, 22.0, 22.0])
            .collect()
    }
    pub(super) fn beacon_click(&mut self, local: (f32, f32)) -> bool {
        let l = (local.0 - SHIFT, local.1);
        let levels = self.beacon_levels();
        let Some(sel) = self.station.as_ref().map(|s| s.beacon) else {
            return false;
        };
        for (e, x, y, primary) in buttons() {
            if !inside(l, x, y, 22.0, 22.0) {
                continue;
            }
            let tier = EFFECTS[e].1;
            let selected = if primary {
                sel[0] == Some(e)
            } else {
                sel[1] == Some(e)
            };
            if tier <= levels.min(if primary { 3 } else { 4 })
                && !selected
                && (primary || levels >= 4)
            {
                if let Some(s) = self.station.as_mut() {
                    if primary {
                        s.beacon[0] = Some(e);
                        if s.beacon[1] != Some(e) {
                            s.beacon[1] = None;
                        }
                    } else {
                        s.beacon[1] = Some(e);
                    }
                }
                self.sounds
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
            }
            return true;
        }
        let (ux, uy) = upgrade_button();
        if inside(l, ux, uy, 22.0, 22.0) {
            if levels >= 4
                && let Some(p) = sel[0]
                && sel[1] != Some(p)
                && let Some(s) = self.station.as_mut()
            {
                s.beacon[1] = Some(p);
                self.sounds
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
            }
            return true;
        }
        if inside(l, 164.0, 107.0, 22.0, 22.0) {
            let paid = self.station.as_ref().is_some_and(|s| s.inputs[0].is_some());
            if paid && sel[0].is_some() {
                self.sounds
                    .play("minecraft:ui.button.click", None, 0.25, 1.0);
                self.confirm_beacon(sel);
                self.close_beacon_screen();
            }
            return true;
        }
        if inside(l, 190.0, 107.0, 22.0, 22.0) {
            self.sounds
                .play("minecraft:ui.button.click", None, 0.25, 1.0);
            self.close_beacon_screen();
            return true;
        }
        false
    }
    fn close_beacon_screen(&mut self) {
        self.close_container();
        self.screen = super::Screen::Playing;
        self.capture(true);
    }
    /// `BeaconMenu.updateEffects`: spend the payment and set the powers. On
    /// a server, `BeaconScreen`'s confirm button sends them
    /// (`ServerboundSetBeaconPacket`) and the server's menu does it.
    fn confirm_beacon(&mut self, sel: [Option<usize>; 2]) {
        let Some(s) = self.station.as_mut() else {
            return;
        };
        let pos = s.pos;
        s.inputs[0] = None;
        let name = |e: Option<usize>| e.map(|e| format!("minecraft:{}", EFFECTS[e].0));
        let mut select_sound = false;
        if let Some(BlockEntity::Beacon(b)) =
            self.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
        {
            b.primary = name(sel[0]);
            b.secondary = name(sel[1]);
            select_sound = !b.sections.is_empty();
        }
        if select_sound {
            self.beacon_sound(pos, "minecraft:block.beacon.power_select");
        }
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    }
}

/// `BeaconMenu.PaymentSlot` and the player inventory, relative to the 176 origin.
pub(super) fn slot_position(i: usize) -> Option<(f32, f32)> {
    Some(match i {
        0..9 => (36.0 + SHIFT + i as f32 * 18.0, 195.0),
        9..36 => (
            36.0 + SHIFT + ((i - 9) % 9) as f32 * 18.0,
            137.0 + ((i - 9) / 9) as f32 * 18.0,
        ),
        INPUT => (136.0 + SHIFT, 110.0),
        _ => return None,
    })
}
