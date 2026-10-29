//! `ComposterBlock`: compostable items go in by hand (`useItemOn`) or
//! through its input container (hoppers, droppers, crafters above), and the
//! fill feedback of level event 1500 (`handleFill`). Emptying a full
//! composter by hand is `useWithoutItem` in horizon-world.
use super::commands::compute::ComputeContext;
use horizon_client::{
    items::ItemStack,
    mobs::villager,
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::{position::BlockPos, random::RandomSource};

impl crate::game::Sim<'_> {
    /// The composter's `level` at `pos`, None when it isn't a composter.
    pub(super) fn composter_level(&self, pos: [i32; 3]) -> Option<i32> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let s = self.pack.assets.registry.state(state)?;
        if &*s.name != "minecraft:composter" {
            return None;
        }
        s.properties.get("level")?.parse().ok()
    }
    /// The item's `compostable` layers provider.
    fn compostable(&self, stack: &ItemStack) -> Option<serde_json::Value> {
        self.res.items().get(&stack.item)?.compostable.clone()
    }
    /// `WorkAtComposter.compostItems` for the villager at `mob`: a full
    /// composter is emptied, then up to 20 wheat or beetroot seeds go in
    /// (keeping 10 of each kind) until the composter reaches level 7. Level
    /// event 1500 plays once, at the end.
    pub(super) fn villager_compost_items(&mut self, mob: usize, pos: [i32; 3]) {
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        let Some(mut before) = self.composter_level(pos) else {
            return;
        };
        if before == 8 {
            self.with_level(|b, level, ticks| b.composter_extract(level, ticks, at));
            let _ = self.process_level_events();
            before = 0;
        }
        let mut remaining = 20;
        let mut amounts: Vec<(std::sync::Arc<str>, i32)> = Vec::new();
        let slots = villager::ext(&self.mobs[mob]).map_or(0, |e| e.inventory.len());
        'slots: for slot in (0..slots).rev() {
            if remaining <= 0 {
                break;
            }
            let stack = villager::ext_mut(&mut self.mobs[mob]).inventory[slot].clone();
            if !matches!(
                &*stack.item,
                "minecraft:wheat_seeds" | "minecraft:beetroot_seeds"
            ) {
                continue;
            }
            let known = amounts.iter().position(|(item, _)| *item == stack.item);
            let total = known.map_or(0, |k| amounts[k].1) + stack.count as i32;
            match known {
                Some(k) => amounts[k].1 = total,
                None => amounts.push((stack.item.clone(), total)),
            }
            let uses = (total - 10).min(remaining).min(stack.count as i32);
            if uses <= 0 {
                continue;
            }
            remaining -= uses;
            for _ in 0..uses {
                self.villager_insert_item(mob, slot, &stack, pos);
                if self.composter_level(pos) == Some(7) {
                    break 'slots;
                }
            }
        }
        let after = self.composter_level(pos).unwrap_or(before);
        self.with_level(|b, level, ticks| {
            b.composter_fill_effects(level, ticks, at, after != before)
        });
        let _ = self.process_level_events();
    }
    /// `ComposterBlock.insertItem` with a villager's stack: below level 7 a
    /// compostable item is used up and may add a layer.
    fn villager_insert_item(&mut self, mob: usize, slot: usize, stack: &ItemStack, pos: [i32; 3]) {
        if self.composter_level(pos).is_none_or(|level| level >= 7) {
            return;
        }
        let Some(layers) = self.compostable(stack) else {
            return;
        };
        let context = ComputeContext { block: Some(pos) };
        let layers = self.provider_int(&layers, &context, 0).unwrap_or(0);
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| b.composter_layer(level, ticks, at, layers));
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        let _ = self.process_level_events();
        let inventory = &mut villager::ext_mut(&mut self.mobs[mob]).inventory;
        inventory[slot].count -= 1;
        if inventory[slot].count == 0 {
            inventory.remove(slot);
        }
    }
    /// `ComposterBlock.handleFill` (level event 1500) on the client: the
    /// fill sound and ten composter particles above the compost.
    pub(super) fn composter_fill_event(&mut self, pos: BlockPos, success: bool) {
        let center = [
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        ];
        let sound = if success {
            "minecraft:block.composter.fill_success"
        } else {
            "minecraft:block.composter.fill"
        };
        self.sounds.play(sound, Some(center), 1.0, 1.0);
        // getShape(..).max(Y, 0.5, 0.5): the compost surface (SHAPES[8] = SHAPES[7]).
        let level = self.composter_level([pos.x, pos.y, pos.z]).unwrap_or(0);
        let height = f64::from((1 + level.min(7) * 2).clamp(2, 16)) / 16.0 + 0.03125;
        let options = ParticleOptions::simple(ParticleType::Composter);
        for _ in 0..10 {
            let r = self.particles.random();
            let v = [
                r.next_gaussian() * 0.02,
                r.next_gaussian() * 0.02,
                r.next_gaussian() * 0.02,
            ];
            let p = [
                f64::from(pos.x) + 0.1875 + 0.625 * f64::from(r.next_float()),
                f64::from(pos.y) + height + f64::from(r.next_float()) * (1.0 - height),
                f64::from(pos.z) + 0.1875 + 0.625 * f64::from(r.next_float()),
            ];
            self.particles.add(&options, p, v);
        }
    }
}
