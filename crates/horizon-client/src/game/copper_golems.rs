//! Copper golems on the game side: what their brain (`mobs::transport_items`)
//! does to chests (opener counts, the lid block events, slots), the openers
//! recheck, turning into statues, and the hand-over, waxing, scraping and
//! shearing interactions.
use super::Game;
use horizon_client::{
    items::ItemStack,
    mobs::{
        Mob,
        transport_items::{held, set_held}}};
use horizon_core::random::RandomSource;
use horizon_world::chunk::BlockState;


impl crate::game::Sim<'_> {
    /// A copper golem opened or closed a chest half: the counter changes and
    /// the clients see the block event (`openerCountChanged`).
    pub(super) fn golem_chest_open(&mut self, chest: [i32; 3], open: bool) {
        let key = (chest[0], chest[1], chest[2]);
        let entry = self.world.chest_openers.entry(key).or_insert((0, 0.0));
        if open {
            entry.0 += 1;
            entry.1 = entry.1.max(3.0);
        } else {
            entry.0 -= 1;
            if entry.0 == 0 {
                entry.1 = 0.0;
            }
        }
        let count = entry.0;
        if count <= 0 {
            self.world.chest_openers.remove(&key);
        }
        self.block_entity_event(chest, 1, count.max(0));
    }
    /// A copper golem took from or filled a chest slot.
    pub(super) fn golem_chest_slot(
        &mut self,
        chest: [i32; 3],
        slot: usize,
        stack: Option<ItemStack>,
    ) {
        let key = (chest[0], chest[1], chest[2]);
        if let Some(e) = self.world.block_entities.get_mut(&key)
            && let Some(s) = e.slots_mut().get_mut(slot)
        {
            *s = stack;
            // TransportItemsBetweenContainers calls Container.setChanged.
            self.container_changed(key);
        }
    }
    /// `CopperGolem.turnToStatue`, which its weathering asked for.
    pub(super) fn golem_become_statue(&mut self, id: u32) {
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        let mut m = self.mobs[i].clone();
        self.golem_turn_to_statue(&mut m);
        if let Some(i) = self.mobs.iter().position(|x| x.id == id) {
            self.mobs[i] = m;
        }
    }
    fn golem_turn_to_statue(&mut self, m: &mut Mob) {
        let pos = [
            m.position.x.floor() as i32,
            m.position.y.floor() as i32,
            m.position.z.floor() as i32,
        ];
        // The pose comes from the golem's own random (`this.random.nextInt(0, 4)`).
        let pose = ["standing", "sitting", "running", "star"]
            [m.random.next_int_bound(4).unwrap_or(0) as usize];
        // Direction.fromYRot.
        let facing = ["south", "west", "north", "east"]
            [((f64::from(m.yaw) / 90.0 + 0.5).floor() as i32 & 3) as usize];
        let state = BlockState {
            name: "minecraft:oxidized_copper_golem_statue".into(),
            properties: [
                ("facing".to_string(), facing.to_string()),
                ("copper_golem_pose".to_string(), pose.to_string()),
                ("waterlogged".to_string(), "false".to_string()),
            ]
            .into(),
        };
        let registry = self.pack.assets.registry;
        let Some(id) = registry.id(&state) else {
            return;
        };
        if self.level_set(pos, id).is_err() {
            return;
        }
        self.golem_drop_equipment(m);
        m.extra.opened_chest = None;
        if m.extra.leash.is_some() {
            self.break_leash(m.id, true, false);
            m.extra.leash = None;
        }
        m.removed = true;
        self.sounds.play(
            "minecraft:entity.copper_golem_become_statue",
            Some([m.position.x, m.position.y, m.position.z]),
            1.0,
            1.0,
        );
    }
    /// `Mob.dropPreservedEquipment`: the guaranteed drops in `EquipmentSlot`
    /// order, the carried stack (main hand) then the antenna flower (saddle),
    /// each `spawnAtLocation` at the golem's feet.
    pub(super) fn golem_drop_equipment(&mut self, m: &mut Mob) {
        let at = m.position;
        if let Some(stack) = held(m).cloned() {
            set_held(m, None);
            self.spawn_at_location(at, 0.0, stack);
        }
        if let Some(antenna) = m.saddle_item() {
            m.set_saddle(None);
            self.spawn_at_location(at, 0.0, antenna);
        }
    }
    // ---- interactions -----------------------------------------------------
    /// `readyForShearing`: the antenna holds a flower shears take.
    pub(super) fn golem_shearable(&self, index: usize) -> bool {
        self.mobs[index].saddle_item().is_some_and(|a| {
            self.res
                .items()
                .item_tags
                .get("minecraft:shearable_from_copper_golem")
                .is_some_and(|t| t.contains(&*a.item))
        })
    }
}
