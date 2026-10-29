//! Saving and loading the game state that lives outside chunk blocks:
//! entities (`entities/*.mca`, per chunk like vanilla's `EntityStorage`) and
//! scheduled block/fluid ticks (chunk `block_ticks`/`fluid_ticks`).
use horizon_client::{entity_nbt, save::random_uuid};
use horizon_core::geometry::Vec3;
use horizon_world::{nbt::Tag, registry::BlockStateId};
use std::collections::HashMap;

/// What Horizon remembers about loaded entities to save them faithfully.
#[derive(Default)]
pub struct EntityMemory {
    /// UUIDs of mobs and item entities by Horizon entity ID.
    pub(super) uuids: HashMap<u32, [i32; 4]>,
    /// The compound each mob or item entity was loaded with.
    pub(super) raw: HashMap<u32, Tag>,
    /// Entities without a Horizon entity ID (orbs, arrows, falling blocks, crystals,
    /// the dragon): the UUID and compound they were loaded with, found again by
    /// type and position (an entity that has moved since loading is saved afresh).
    anon: HashMap<(String, [u64; 3]), (Vec3, Option<[i32; 4]>, Tag)>,
}

fn anon_key(kind: &str, p: Vec3) -> (String, [u64; 3]) {
    (
        kind.to_owned(),
        [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()],
    )
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
    /// The compound of an entity Horizon does not simulate, with its current
    /// position, rotation and UUID.
    pub(super) fn other_to_nbt(&self, o: &super::commands::OtherEntity, uuid: [i32; 4]) -> Tag {
        let mut tag = self
            .persist
            .raw
            .get(&o.id)
            .cloned()
            .unwrap_or_else(|| Tag::Compound(Default::default()));
        if let Tag::Compound(c) = &mut tag {
            c.insert("id".into(), Tag::String(o.kind.as_str().into()));
            c.insert(
                "Pos".into(),
                Tag::List(
                    [o.position.x, o.position.y, o.position.z]
                        .map(|v| Tag::Double(v.to_bits()))
                        .to_vec(),
                ),
            );
            c.insert(
                "Rotation".into(),
                Tag::List(vec![
                    Tag::Float(o.yaw.to_bits()),
                    Tag::Float(o.pitch.to_bits()),
                ]),
            );
            c.insert("UUID".into(), Tag::IntArray(uuid.to_vec()));
        }
        tag
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
    /// The compound an entity without a Horizon entity ID was loaded with
    /// (the crystals and the dragon), found again by type and position.
    pub(super) fn persist_anon(&self, kind: &str, p: Vec3) -> (Option<[i32; 4]>, Option<&Tag>) {
        self.persist
            .anon
            .get(&anon_key(kind, p))
            .map_or((None, None), |(_, uuid, raw)| (*uuid, Some(raw)))
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
    pub(in crate::game) fn state_to_nbt(&self, state: BlockStateId) -> Tag {
        let registry = self.pack.assets.registry;
        match registry.state(state) {
            Some(s) if registry.default_state(&s.name) == Some(state) => {
                Tag::String(s.name.as_str().into())
            }
            Some(s) => s.to_nbt(),
            None => Tag::String("minecraft:air".into()),
        }
    }
}
