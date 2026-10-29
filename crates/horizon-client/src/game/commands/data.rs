//! `/data` (DataCommands), `/tag` (TagCommand) and `/summon` with NBT: entity,
//! block entity and storage data accessors over the game state.
use super::{Game, Target};
use horizon_client::{entity_nbt, save::random_uuid};
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
            Target::Guest(_) => {
                return self
                    .for_player(t, |g| g.entity_nbt(Target::Player))
                    .flatten();
            }
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
            Target::Item(id) => {
                let uuid = self.uuid_for(id);
                let e = self.dropped.iter().find(|e| e.id == id)?;
                entity_nbt::item_entity_to_nbt(e, uuid, self.persist.raw.get(&id))
            }
            Target::Stand(id) => {
                let uuid = self.uuid_for(id);
                let a = self.armor_stands.iter().find(|a| a.id == id)?;
                entity_nbt::armor_stand_to_nbt(a, uuid, self.persist.raw.get(&id))
            }
            Target::Frame(id) => {
                let uuid = self.uuid_for(id);
                let f = self.item_frames.iter().find(|f| f.id == id)?;
                entity_nbt::item_frame_to_nbt(f, uuid, self.persist.raw.get(&id))
            }
            Target::Painting(id) => {
                let uuid = self.uuid_for(id);
                let p = self.paintings.iter().find(|p| p.id == id)?;
                entity_nbt::painting_to_nbt(p, uuid, self.persist.raw.get(&id))
            }
            Target::Boat(id) => {
                let uuid = self.uuid_for(id);
                let b = self.boats.iter().find(|b| b.id == id)?;
                entity_nbt::boat_to_nbt(
                    &b.entity_type(),
                    b.position,
                    b.velocity,
                    b.yaw,
                    uuid,
                    self.persist.raw.get(&id),
                    b.chest.then_some(&b.items[..]),
                )
            }
            Target::Cart(id) => {
                let uuid = self.uuid_for(id);
                let c = self.carts.iter().find(|c| c.id == id)?;
                c.to_nbt(uuid, self.persist.raw.get(&id))
            }
            Target::Other(id) => {
                let uuid = self.uuid_for(id);
                let o = self.others.iter().find(|o| o.id == id)?.clone();
                self.other_to_nbt(&o, uuid)
            }
            Target::Orb(id) => {
                entity_nbt::orb_to_nbt(self.orbs.iter().find(|o| o.id == id)?, random_uuid())
            }
            Target::Projectile(id) => {
                let p = self.creatures.projectiles.iter().find(|p| p.id == id)?;
                let owner = if p.owner == Some(horizon_client::mobs::Who::Player) {
                    player_uuid
                } else {
                    None
                };
                let in_block = p
                    .arrow
                    .as_ref()
                    .and_then(|a| a.last_state)
                    .map(|s| self.state_to_nbt(s));
                entity_nbt::arrow_to_nbt(p, random_uuid(), owner, in_block, None)
                    .or_else(|| entity_nbt::fireball_to_nbt(p, random_uuid(), owner, None))?
            }
            Target::Crystal(id) => {
                let c = self.crystals.iter().find(|c| c.id == id)?;
                let (uuid, raw) = self.persist_anon("minecraft:end_crystal", c.position);
                c.to_nbt(uuid.unwrap_or_else(random_uuid), raw)
            }
            Target::Dragon(id) => {
                let d = self.dragon.as_ref().filter(|d| d.id == id)?;
                let (uuid, raw) = self.persist_anon("minecraft:ender_dragon", d.position);
                d.to_nbt(uuid.unwrap_or_else(random_uuid), raw)
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
