//! What the client knows of its entities' identity and saved form: their
//! UUIDs and the raw data they arrived with, for F3 + I's copied command.
use horizon_client::{entity_nbt, save::random_uuid};
use horizon_world::nbt::Tag;
use std::collections::HashMap;

/// What Horizon remembers about loaded entities to save them faithfully.
#[derive(Default)]
pub struct EntityMemory {
    /// UUIDs of mobs and item entities by Horizon entity ID.
    pub(super) uuids: HashMap<u32, [i32; 4]>,
    /// The compound each mob or item entity was loaded with.
    pub(super) raw: HashMap<u32, Tag>,
}

impl crate::game::Sim<'_> {
    /// The mobs riding entity `id` under `tag`'s `Passengers`, after the
    /// passengers it kept from loading.
    fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) {
        let riders: Vec<usize> = (0..self.mobs.len())
            .filter(|&j| self.mobs[j].extra.mount == Some(id) && self.mobs[j].alive())
            .collect();
        let riders: Vec<Tag> = riders.into_iter().map(|j| self.mob_tree_nbt(j)).collect();
        if let Tag::Compound(c) = tag {
            let mut list = match c.get(&"Passengers".into()) {
                Some(Tag::List(l)) => l.clone(),
                _ => Vec::new(),
            };
            list.extend(riders);
            if list.is_empty() {
                c.remove(&"Passengers".into());
            } else {
                c.insert("Passengers".into(), Tag::List(list));
            }
        }
    }
    /// A mob's entity tag with the mobs riding it under `Passengers`.
    pub(super) fn mob_tree_nbt(&mut self, i: usize) -> Tag {
        let id = self.mobs[i].id;
        let uuid = self.uuid_for(id);
        let mut tag = entity_nbt::mob_to_nbt(&self.mobs[i], uuid, self.persist.raw.get(&id));
        self.write_leash(&mut tag, i);
        self.put_mob_passengers(&mut tag, id);
        tag
    }
    /// The compound an entity was loaded with (and commands have edited).
    pub(super) fn persist_raw(&self, id: u32) -> Option<&Tag> {
        self.persist.raw.get(&id)
    }
    /// A mob's UUID, made up for new ones.
    pub(super) fn mob_uuid(&mut self, id: u32) -> [i32; 4] {
        self.uuid_for(id)
    }
    pub(super) fn uuid_for(&mut self, id: u32) -> [i32; 4] {
        *self.persist.uuids.entry(id).or_insert_with(random_uuid)
    }
}
