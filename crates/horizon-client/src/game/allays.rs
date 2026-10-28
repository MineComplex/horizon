//! Allays on the game side: spotting and picking up matching drops,
//! throwing their load to the player or note block, dancing to jukeboxes,
//! and the hand-over interaction (`Allay.mobInteract`).
use super::Game;
use horizon_client::{ items::ItemStack, mobs::Species};

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// `Allay.mobInteract`: duplicate when dancing with an amethyst shard,
    /// take the held item, or hand back what it holds.
    pub(super) fn interact_allay(&mut self, index: usize, item: &str) -> bool {
        let m = &self.mobs[index];
        if m.species() != Species::Allay || !m.alive() {
            return false;
        }
        let at = [m.position.x, m.position.y, m.position.z];
        if m.extra.flag && item == "minecraft:amethyst_shard" && m.extra.egg_time == 0 {
            let pos = at;
            self.mobs[index].extra.egg_time = 6000;
            if let Some(id) = self.spawn_mob("minecraft:allay", pos, true)
                && let Some(c) = self.mobs.iter_mut().find(|m| m.id == id)
            {
                c.extra.egg_time = 6000;
            }
            // Allay.duplicateAllay: entity event 18, three spawnHeartParticle.
            let id = self.mobs[index].id;
            self.mob_event_particles(id, 18);
            self.sounds
                .play("minecraft:block.amethyst_block.chime", Some(at), 2.0, 1.0);
            self.consume_held();
            return true;
        }
        let acting = self.acting_uuid();
        let holds = m.extra.main_hand.is_some();
        if !holds && !item.is_empty() {
            let m = &mut self.mobs[index];
            m.extra.main_hand = Some(item.into());
            m.extra.tame = true;
            // `Allay.mobInteract` remembers LIKED_PLAYER by UUID.
            m.extra.owner = Some(acting);
            self.sounds
                .play("minecraft:entity.allay.item_given", Some(at), 2.0, 1.0);
            self.consume_held();
            return true;
        }
        if holds && item.is_empty() {
            let m = &mut self.mobs[index];
            let held = m
                .extra
                .main_hand
                .take()
                .unwrap_or_else(|| "minecraft:air".into());
            let carried = std::mem::take(&mut m.extra.ints[1]);
            m.extra.tame = false;
            m.extra.owner = None;
            self.sounds
                .play("minecraft:entity.allay.item_taken", Some(at), 2.0, 1.0);
            if carried > 0 {
                self.spawn_item(
                    ItemStack::new(&held, carried as u32),
                    at,
                    [0.0, 0.2, 0.0],
                    40,
                );
            }
            if let Some(rest) = self.give(ItemStack::new(&held, 1))
                && let Some(eye) = self.eye_position()
            {
                self.spawn_item(rest, eye, [0.0; 3], 0);
            }
            return true;
        }
        false
    }
}
