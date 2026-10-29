//! `/data` (DataCommands), `/tag` (TagCommand) and `/summon` with NBT: entity,
//! block entity and storage data accessors over the game state.
use super::{Game, Target};
use horizon_world::nbt::{JavaString, Tag};

fn set_key(tag: &mut Tag, key: &str, value: Tag) {
    if let Tag::Compound(c) = tag {
        c.insert(JavaString::from(key), value);
    }
}
impl Game<'_> {
    // ---- entity data ---------------------------------------------------

    // ---- accessors -----------------------------------------------------

    // ---- /data -----------------------------------------------------------

    // ---- /tag ------------------------------------------------------------

    // ---- /summon ---------------------------------------------------------
}
impl crate::game::Sim<'_> {
    /// `NbtPredicate.getEntityTagToCompare`: the entity as saved. An entity
    /// in another level is read in the level that holds it.
    pub(in crate::game) fn entity_nbt(&mut self, t: Target) -> Option<Tag> {
        self.in_target_level(t, |g| g.entity_nbt_in_level(t))
            .flatten()
    }
    fn entity_nbt_in_level(&mut self, t: Target) -> Option<Tag> {
        let player_uuid = self.level.player_uuid;
        Some(match t {
            Target::Player => {
                self.player.as_ref()?;
                self.sync_level();
                let mut tag = self.level.player_tag();
                if let Some(u) = player_uuid {
                    set_key(&mut tag, "UUID", Tag::IntArray(u.to_vec()));
                }
                // NbtPredicate.getEntityTagToCompare: a player's held stack.
                let selected = self.level.selected_slot;
                let held = match tag.get("Inventory") {
                    Some(Tag::List(items)) => items
                        .iter()
                        .find(|i| matches!(i.get("Slot"), Some(Tag::Byte(b)) if i32::from(*b) == selected))
                        .cloned(),
                    _ => None,
                };
                if let Some(mut stack) = held {
                    if let Tag::Compound(c) = &mut stack {
                        c.remove(&"Slot".into());
                    }
                    set_key(&mut tag, "SelectedItem", stack);
                }
                tag
            }
            Target::Mob(id) => {
                // `saveWithoutId` writes the entity as saved, riders under
                // `Passengers`: `/data get` reads the same tree persistence does (#1519).
                let i = self.mobs.iter().position(|m| m.id == id)?;
                self.mob_tree_nbt(i)
            }
        })
    }
    pub(in crate::game) fn data_block_name(&self, p: [i32; 3]) -> String {
        let state = self.world.block(p[0], p[1], p[2]);
        self.pack
            .assets
            .registry
            .state(state)
            .map_or_else(|| "minecraft:air".into(), |s| s.name.to_string())
    }
}
