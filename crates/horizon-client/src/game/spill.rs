//! Block entities removed by the block behaviour (`LevelChunk.setBlockState`
//! running `BlockEntity.preRemoveSideEffects`) and the resources a destroyed
//! block drops (`Block.dropResources`), drawn from the level random at the
//! moment vanilla draws them. The item entities they make are added to the
//! world as soon as the behaviour returns, within the same tick.
use super::{GameLevel, Screen, dispenser::new_item_motion};
use horizon_client::{containers::BlockEntity, furnace::FurnaceCooking, items::ItemStack};
use horizon_core::position::BlockPos;
use horizon_core::random::RandomSource;
use horizon_world::{
    level::{
        LevelEvent,
        removal::{self, PreRemove},
    },
    registry::BlockStateId,
};

/// An item entity to add: where, its motion (None: `new ItemEntity`'s own
/// random motion) and its pickup delay.
pub(super) struct Spawn {
    pub(super) stack: ItemStack,
    pub(super) pos: [f64; 3],
    pub(super) motion: Option<[f64; 3]>,
    pub(super) delay: u32,
}

#[derive(Default)]
pub(super) struct Spill {
    pub(super) spawns: Vec<Spawn>,
    /// Experience orbs (`ExperienceOrb.award`): where and how much, each
    /// and the merge id it drew from the level random.
    pub(super) orbs: Vec<([f64; 3], u32, i32)>,
    /// Block entities the behaviour removed, for the game's own follow-ups
    /// (open screens, playing songs).
    pub(super) removed: Vec<([i32; 3], BlockEntity)>,
}

/// The block entity at `pos` Horizon keeps as it was loaded, if any
/// (campfires, decorated pots, chiseled bookshelves, shelves).
pub(super) fn saved_entity(
    world: &horizon_client::world::World,
    pos: [i32; 3],
) -> Option<horizon_world::nbt::Tag> {
    use horizon_world::nbt::Tag;
    let saved = world
        .chunks
        .get(&(pos[0] >> 4, pos[2] >> 4))?
        .saved
        .as_ref()?;
    saved
        .block_entities
        .iter()
        .find(|t| {
            t.get("x") == Some(&Tag::Int(pos[0]))
                && t.get("y") == Some(&Tag::Int(pos[1]))
                && t.get("z") == Some(&Tag::Int(pos[2]))
        })
        .cloned()
}

impl GameLevel<'_> {
    /// `ExperienceOrb.award`: orbs of the largest sizes, each drawing the id
    /// `tryMergeToExisting` compares from the level random.
    pub(super) fn award_orbs(&mut self, at: [f64; 3], mut amount: u32) {
        while amount > 0 {
            let value = removal::orb_value(amount);
            amount -= value;
            let merge_id = self.world.random.next_int_bound(40).unwrap_or(0);
            self.spill.orbs.push((at, value, merge_id));
        }
    }
    /// `BlockEntity.preRemoveSideEffects` and `removeBlockEntity` for the block
    /// entity of `old` at `pos`. A block entity Horizon never created reads as
    /// vanilla's fresh one: an empty container still draws a spot per slot.
    pub(super) fn remove_entity(
        &mut self,
        pos: BlockPos,
        old: BlockStateId,
        side_effects: bool,
    ) -> u8 {
        let key = (pos.x, pos.y, pos.z);
        if self.remove_saved_entity(pos, old, side_effects) {
            return 0;
        }
        // Sculk blocks and creaking hearts notice their removal on their own tick.
        if matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Sculk(_) | BlockEntity::CreakingHeart(_))
        ) {
            return 0;
        }
        let entity = self.world.block_entities.remove(&key);
        self.world.block_names.remove(&key);
        let name = self.facts.get(old).name.clone();
        let kind = removal::pre_remove(&name);
        let mut neighbour_updates = 0;
        if side_effects {
            match kind {
                PreRemove::Contents { slots, furnace } => {
                    let items = entity.as_ref().map_or(&[][..], BlockEntity::slots);
                    for slot in 0..slots {
                        let stack = items.get(slot).cloned().flatten();
                        self.drop_item_stack(pos, stack);
                    }
                    // AbstractFurnaceBlockEntity.preRemoveSideEffects: then the
                    // experience of what it made, at the block's centre.
                    if furnace && let Some(BlockEntity::Furnace(f)) = &entity {
                        let at = [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 0.5,
                            f64::from(pos.z) + 0.5,
                        ];
                        for (count, experience) in f.recipe_experience(self.recipes) {
                            let xp = horizon_client::containers::create_experience(
                                count,
                                experience,
                                &mut self.world.random,
                            );
                            self.award_orbs(at, xp);
                        }
                    }
                }
                PreRemove::Campfire => {
                    let items = match &entity {
                        Some(BlockEntity::Campfire(c)) => c.items.clone(),
                        _ => Default::default(),
                    };
                    for stack in items {
                        self.drop_item_stack(pos, stack);
                    }
                }
                PreRemove::Jukebox => {
                    // popOutTheItem: removeTheItem stops a playing song (its
                    // events and a neighbour update), the record pops, and
                    // onSongChanged updates the neighbours again.
                    if let Some(BlockEntity::Jukebox(j)) = &entity
                        && let Some(record) = j.record.clone()
                    {
                        if j.playing {
                            self.jukebox_stopped(pos, old);
                            neighbour_updates += 1;
                        }
                        let at = removal::jukebox_pop_position(&mut self.world.random, pos);
                        self.spill.spawns.push(Spawn {
                            stack: record,
                            pos: at,
                            motion: None,
                            delay: 10,
                        });
                        neighbour_updates += 1;
                    }
                }
                PreRemove::Lectern => {
                    let props = self.registry.state(old).map(|s| &s.properties);
                    let has_book = props
                        .and_then(|p| p.get("has_book"))
                        .is_some_and(|v| v == "true");
                    let step = match props.and_then(|p| p.get("facing")).map(String::as_str) {
                        Some("north") => [0, -1],
                        Some("south") => [0, 1],
                        Some("west") => [-1, 0],
                        _ => [1, 0],
                    };
                    if has_book
                        && let Some(BlockEntity::Lectern(l)) = &entity
                        && let Some(book) = l.book.clone()
                    {
                        self.spill.spawns.push(Spawn {
                            stack: book,
                            pos: removal::lectern_book_position(pos, step),
                            motion: None,
                            delay: 10,
                        });
                    }
                }
                PreRemove::ShulkerBox
                | PreRemove::SculkShrieker
                | PreRemove::CreakingHeart
                | PreRemove::Beacon
                | PreRemove::Nothing => {}
            }
        }
        // BlockEntity.setRemoved: a jukebox always reports its song stopping,
        // a beacon plays its deactivation.
        match kind {
            PreRemove::Jukebox => self.jukebox_stopped(pos, old),
            PreRemove::Beacon => self.events.push(LevelEvent::Sound {
                pos,
                sound: "minecraft:block.beacon.deactivate".into(),
                volume: 1.0,
                pitch: 1.0,
            }),
            _ => {}
        }
        if let Some(entity) = entity {
            self.spill.removed.push(([pos.x, pos.y, pos.z], entity));
        }
        neighbour_updates
    }
    /// The jukebox's `JUKEBOX_STOP_PLAY` game event and level event 1011.
    fn jukebox_stopped(&mut self, pos: BlockPos, state: BlockStateId) {
        self.events.push(LevelEvent::GameEvent {
            pos,
            event: "minecraft:jukebox_stop_play",
            state: Some(state),
        });
        self.events.push(LevelEvent::LevelEvent {
            id: 1011,
            pos,
            data: 0,
        });
    }
    /// A block entity Horizon keeps as it was loaded (campfires, decorated
    /// pots, chiseled bookshelves, shelves): its items spill as vanilla's do.
    fn remove_saved_entity(
        &mut self,
        pos: BlockPos,
        old: BlockStateId,
        side_effects: bool,
    ) -> bool {
        use horizon_world::nbt::Tag;
        let chunk_key = (pos.x >> 4, pos.z >> 4);
        let at = |t: &Tag| {
            let int = |k: &str| match t.get(k) {
                Some(Tag::Int(v)) => Some(*v),
                _ => None,
            };
            int("x") == Some(pos.x) && int("y") == Some(pos.y) && int("z") == Some(pos.z)
        };
        let Some(chunk) = self.world.chunks.get_mut(&chunk_key) else {
            return false;
        };
        let Some(saved) = chunk.saved.as_ref() else {
            return false;
        };
        let Some(index) = saved.block_entities.iter().position(at) else {
            return false;
        };
        let chunk = std::sync::Arc::make_mut(chunk);
        let Some(saved) = chunk.saved.as_mut() else {
            return false;
        };
        let tag = std::sync::Arc::make_mut(saved).block_entities.remove(index);
        if !side_effects {
            return true;
        }
        let name = self.facts.get(old).name.clone();
        let items = match removal::pre_remove(&name) {
            PreRemove::Contents { slots, .. } if &*name == "minecraft:decorated_pot" => {
                // Containers.dropContents reads getTheItem, which unpacks the loot table.
                let mut tag = tag;
                super::decorated_pot::unpack_loot(
                    &mut tag,
                    self.loot.as_deref(),
                    &mut self.world.random,
                    self.rand_sequences,
                    self.seed,
                );
                let mut items = vec![None; slots];
                items[0] = tag
                    .get("item")
                    .and_then(horizon_client::save::item_from_nbt);
                items
            }
            PreRemove::Contents { slots, .. } => {
                horizon_client::save::decode_items(tag.get("Items"), slots)
            }
            PreRemove::Campfire => horizon_client::save::decode_items(tag.get("Items"), 4),
            _ => return true,
        };
        for stack in items {
            self.drop_item_stack(pos, stack);
        }
        true
    }
    /// `Containers.dropItemStack` of one slot (an empty one still draws its spot).
    fn drop_item_stack(&mut self, pos: BlockPos, stack: Option<ItemStack>) {
        let count = stack.as_ref().map_or(0, |s| s.count);
        for piece in removal::drop_item_stack(&mut self.world.random, pos, count) {
            let Some(stack) = &stack else { break };
            self.spill.spawns.push(Spawn {
                stack: ItemStack {
                    count: piece.count,
                    ..stack.clone()
                },
                pos: piece.pos,
                motion: Some(piece.motion),
                delay: 0,
            });
        }
    }
}

impl crate::game::Sim<'_> {
    /// Add the item entities the block behaviour spawned and follow up on the
    /// block entities it removed.
    pub(super) fn flush_spill(&mut self) {
        if self.spill.spawns.is_empty()
            && self.spill.removed.is_empty()
            && self.spill.orbs.is_empty()
        {
            return;
        }
        let spill = std::mem::take(&mut self.spill);
        for s in spill.spawns {
            let motion = s.motion.unwrap_or_else(new_item_motion);
            self.spawn_item(s.stack, s.pos, motion, s.delay);
        }
        for (at, value, merge_id) in spill.orbs {
            self.award_orb(at, value, merge_id);
        }
        for (pos, entity) in spill.removed {
            let key = (pos[0], pos[1], pos[2]);
            if matches!(entity, BlockEntity::Jukebox(_)) {
                self.jukebox_removed(pos);
            }
            if self.container == Some(key) {
                self.container_opener_event(key, false);
                self.container = None;
                self.screen = Screen::Playing;
            }
        }
    }
}
