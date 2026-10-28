//! Trial spawners in the game: `TrialSpawner.tickServer` for every trial
//! spawner (`mobs::trial_spawner`), the block's state as it changes, the mobs
//! it adds and the rewards it ejects.
use super::{Game, natural_spawning::GameLevel};
use horizon_client::{
    Result,
    items::ItemStack,
    mobs::{
        trial_spawner::{
            Omen, TrialPlayer,
            TrialSpawnerLevel, TrialState, Uuid}}};
use horizon_core::{
    geometry::Vec3,
    random::LegacyRandom};

type Pos = (i32, i32, i32);

impl TrialSpawnerLevel for GameLevel<'_, '_> {
    fn game_time(&self) -> i64 {
        self.game.world.game_time
    }
    fn mob_spawning(&self) -> bool {
        self.game.world_data.rules.bool("spawn_mobs")
    }
    fn players(&self) -> Vec<TrialPlayer> {
        let guests = self.game.lan_present().into_iter().map(|g| TrialPlayer {
            uuid: g.uuid,
            block: g.position.map(|v| v.floor() as i32),
            eye: g.eye,
            creative: g.creative,
            spectator: g.spectator,
            position: g.position,
            height: g.height,
            alive: g.alive,
            // Guests carry no effects yet.
            omen: None,
        });
        let (Some(p), Some(view)) = (self.game.player.as_ref(), self.game.player_view()) else {
            return guests.collect();
        };
        let host = TrialPlayer {
            uuid: self.game.acting_uuid(),
            block: [p.position.x, p.position.y, p.position.z].map(|v| v.floor() as i32),
            eye: view.eye,
            creative: self.game.creative,
            spectator: self.game.spectator(),
            position: [p.position.x, p.position.y, p.position.z],
            height: p.dimensions.height,
            alive: self.game.level.health > 0.0,
            omen: if self.game.potions.effects.get("trial_omen").is_some() {
                Some(Omen::Trial)
            } else {
                self.game.potions.effects.get("bad_omen").map(|_| Omen::Bad)
            },
        };
        std::iter::once(host).chain(guests).collect()
    }
    fn visual_hit(&self, from: [f64; 3], to: [f64; 3]) -> Option<[i32; 3]> {
        let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
        self.w.visual_hit(v(from), v(to))
    }
    fn entity_block(&self, uuid: Uuid) -> Option<[i32; 3]> {
        let (id, _) = self.game.persist.uuids.iter().find(|(_, u)| **u == uuid)?;
        let m = self.game.mobs.iter().find(|m| m.id == *id)?;
        (!m.removed && m.alive())
            .then(|| [m.position.x, m.position.y, m.position.z].map(|v| v.floor() as i32))
    }
    fn entity_box(&self, uuid: Uuid) -> Option<([f64; 3], f32)> {
        let (id, _) = self.game.persist.uuids.iter().find(|(_, u)| **u == uuid)?;
        let m = self.game.mobs.iter().find(|m| m.id == *id)?;
        let b = m.bounds();
        (!m.removed && m.alive()).then(|| {
            (
                [m.position.x, m.position.y, m.position.z],
                (b.max_position().y - b.min_position().y) as f32,
            )
        })
    }
    fn no_block_collision(&self, block: [i32; 3]) -> bool {
        self.w
            .block_collision_boxes(block[0], block[1], block[2])
            .is_empty()
    }
    fn loot_items(&self, table: &str, seed: i64) -> Vec<ItemStack> {
        let Some(loot) = horizon_client::structure_blocks::loot_data(&self.game.options.resources)
        else {
            return Vec::new();
        };
        loot.roll_split(table, &mut LegacyRandom::new(seed))
            .iter()
            .map(horizon_client::structure_blocks::loot_item_with_components)
            .collect()
    }
    fn seed(&self) -> i64 {
        self.game.level.seed
    }
}

impl Game<'_> {}
impl crate::game::Sim<'_> {
    /// The trial spawner block at `key`: its state and ominous flag.
    pub(super) fn trial_spawner_block(&self, key: Pos) -> Option<(TrialState, bool)> {
        let block = self
            .pack
            .assets
            .registry
            .state(self.world.block(key.0, key.1, key.2))?;
        if block.name != "minecraft:trial_spawner" {
            return None;
        }
        let state = block
            .properties
            .get("trial_spawner_state")
            .and_then(|s| TrialState::from_name(s))
            .unwrap_or_default();
        Some((
            state,
            block.properties.get("ominous").is_some_and(|v| v == "true"),
        ))
    }
    /// `setState` (and `removeOminous`): the block's state and ominous flag.
    pub(super) fn set_trial_spawner_state(
        &mut self,
        key: Pos,
        state: TrialState,
        ominous: bool,
    ) -> Result<()> {
        let current = self.world.block(key.0, key.1, key.2);
        let Some(mut block) = self.pack.assets.registry.state(current).cloned() else {
            return Ok(());
        };
        block
            .properties
            .insert("trial_spawner_state".into(), state.name().into());
        block
            .properties
            .insert("ominous".into(), ominous.to_string());
        let id = self.pack.assets.registry.resolve(&block)?;
        self.level_set([key.0, key.1, key.2], id)
    }
    /// `sendBlockUpdated` of a block entity: the clients load its new update tag.
    pub(super) fn mark_block_entity_updated(&mut self, key: Pos) {
        if let Some(log) = self.world.block_log.as_mut() {
            log.push(key);
        }
        self.world.modified.insert((key.0 >> 4, key.2 >> 4));
    }
}
