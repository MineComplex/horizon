//! `RespawnAnchorBlock` in the game: glowstone charges it (`useItemOn`), a
//! right click on a charged anchor sets the respawn point in the Nether and
//! blows it up anywhere else (`useWithoutItem`), and respawning goes to it
//! (`ServerPlayer.findRespawnAndUseSpawnBlock`, `PlayerList.respawn`),
//! using a charge.
use super::{Dim, Game};
use horizon_client::first_person::Hand;
use horizon_client::{
    Result,
    blocks::Fluid,
    
    items::ItemStack,
    mobs::Who,
    text::Text};
use horizon_core::geometry::Vec3;
use horizon_world::registry::BlockStateId;

/// `ServerPlayer.RespawnConfig`: a respawn point other than the world spawn
/// that the game resolves through its block (a respawn anchor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RespawnConfig {
    pub dim: Dim,
    pub pos: [i32; 3],
    pub yaw: f32,
    pub forced: bool,
}

/// A respawn point as a player's save keeps it (`respawn`): dimension,
/// block, yaw and whether it is forced.
pub(super) type SavedRespawn = (String, [i32; 3], f32, bool);

impl RespawnConfig {
    /// `RespawnData.of`: the yaw wrapped to [-180, 180).
    pub(super) fn wrapped(dim: Dim, pos: [i32; 3], yaw: f32, forced: bool) -> Self {
        Self {
            dim,
            pos,
            yaw: wrap_degrees(f64::from(yaw)) as f32,
            forced,
        }
    }
    pub(super) fn from_saved((dim, pos, yaw, forced): &SavedRespawn) -> Self {
        Self {
            dim: Dim::from_id(dim),
            pos: *pos,
            yaw: *yaw,
            forced: *forced,
        }
    }
    pub(super) fn saved(&self) -> SavedRespawn {
        (self.dim.id().to_owned(), self.pos, self.yaw, self.forced)
    }
}


/// What a right click with an item does to an anchor before `useWithoutItem`.
pub(super) enum AnchorUse {
    /// Glowstone charged it.
    Charged,
    /// `PASS`: the off hand's glowstone charges it instead.
    Pass,
    /// `TRY_WITH_EMPTY_HAND`, or not an anchor.
    Other,
}

impl Game<'_> {
    // ---- respawning -------------------------------------------------------
}
impl crate::game::Sim<'_> {
    pub(super) fn is_anchor(&self, state: BlockStateId) -> bool {
        &*self.res.facts().get(state).name == "minecraft:respawn_anchor"
    }
    fn anchor_charges(&self, state: BlockStateId) -> i32 {
        self.pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("charges"))
            .and_then(|c| c.parse().ok())
            .unwrap_or(0)
    }
    fn anchor_with_charges(&self, state: BlockStateId, charges: i32) -> Option<BlockStateId> {
        let mut s = self.pack.assets.registry.state(state)?.clone();
        s.properties.insert("charges".into(), charges.to_string());
        self.pack.assets.registry.resolve(&s).ok()
    }
    /// `RespawnAnchorBlock.canSetSpawn`: the `respawn_anchor_works` attribute
    /// (only the Nether's dimension type sets it).
    pub(super) fn anchor_works(&self) -> bool {
        self.dim == Dim::Nether
    }
    /// `RespawnAnchorBlock.charge`: one more charge, the block change and the sound.
    fn charge_anchor(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        who: Option<Who>,
    ) -> Result<()> {
        let Some(next) = self.anchor_with_charges(state, self.anchor_charges(state) + 1) else {
            return Ok(());
        };
        self.level_set(pos, next)?;
        self.post_block_game_event("minecraft:block_change", pos, who, Some(next));
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.sounds
            .play("minecraft:block.respawn_anchor.charge", Some(at), 1.0, 1.0);
        Ok(())
    }
    /// `RespawnAnchorBlock.useItemOn` with `held` in the hand that acts.
    pub(super) fn use_respawn_anchor(
        &mut self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
    ) -> Result<AnchorUse> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !self.is_anchor(state) {
            return Ok(AnchorUse::Other);
        }
        let fuel = |s: Option<&ItemStack>| s.is_some_and(|s| &*s.item == "minecraft:glowstone");
        let chargeable = self.anchor_charges(state) < 4;
        if fuel(held) && chargeable {
            let used = held.cloned();
            self.charge_anchor(pos, state, Some(Who::Player))?;
            // itemStack.consume(1, player); the criterion sees the stack as used.
            self.consume_held();
            if let Some(stack) = used {
                self.trigger_block("minecraft:item_used_on_block", pos, &stack);
            }
            return Ok(AnchorUse::Charged);
        }
        if self.hand == Hand::Main && fuel(self.offhand.as_ref()) && chargeable {
            return Ok(AnchorUse::Pass);
        }
        Ok(AnchorUse::Other)
    }
    /// `RespawnAnchorBlock.useWithoutItem` on a charged anchor: outside the
    /// Nether it explodes; inside, it becomes the respawn point.
    pub(super) fn respawn_anchor_used(&mut self, pos: [i32; 3]) -> Result<()> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !self.anchor_works() {
            return self.explode_anchor(pos, state);
        }
        let config = RespawnConfig {
            dim: self.dim,
            pos,
            yaw: 0.0,
            forced: false,
        };
        // RespawnConfig.isSamePosition: dimension and block.
        let same = self
            .respawn_config
            .is_some_and(|c| c.dim == config.dim && c.pos == config.pos);
        if !same {
            // setRespawnPosition(config, true): the message, the point; the
            // anchor replaces a bed.
            self.system_message(Text::translate("block.minecraft.set_spawn", &[]));
            self.respawn_config = Some(config);
            if let Some([x, y, z]) = self.level.world_spawn {
                self.spawn = Some(Vec3::new(
                    f64::from(x) + 0.5,
                    f64::from(y),
                    f64::from(z) + 0.5,
                ));
            }
            let at = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ];
            self.sounds.play(
                "minecraft:block.respawn_anchor.set_spawn",
                Some(at),
                1.0,
                1.0,
            );
        }
        Ok(())
    }
    /// `RespawnAnchorBlock.isWaterThatWouldFlow`.
    fn water_would_flow(&self, p: [i32; 3]) -> bool {
        let f = self.res.facts().get(self.world.block(p[0], p[1], p[2]));
        if f.fluid != Fluid::Water {
            return false;
        }
        if f.fluid_level == 8 && !f.fluid_falling {
            return true;
        }
        if f.fluid_level < 2 {
            return false;
        }
        self.res
            .facts()
            .get(self.world.block(p[0], p[1] - 1, p[2]))
            .fluid
            != Fluid::Water
    }
    /// `RespawnAnchorBlock.explode`: the anchor goes, then a power 5 fiery
    /// explosion (`badRespawnPointExplosion`, `ExplosionInteraction.BLOCK`)
    /// whose centre resists like water when water would flow into it.
    fn explode_anchor(&mut self, pos: [i32; 3], _state: BlockStateId) -> Result<()> {
        self.level_set(pos, self.world.air)?;
        let horizontal = [[0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]]
            .into_iter()
            .any(|d| self.water_would_flow([pos[0] + d[0], pos[1], pos[2] + d[2]]));
        let above = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1] + 1, pos[2]))
            .fluid
            == Fluid::Water;
        let in_water = horizontal || above;
        let center = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        self.block_explosion(center, in_water.then_some(pos))
    }
    /// `BedRule.destroyOnUse` of the dimension's `bed_rule` and
    /// `straw_bed_rule`: true in the Nether and the End.
    pub(super) fn beds_destroy_on_use(&self) -> bool {
        self.dim != Dim::Overworld
    }
    /// `AbstractBedBlock.useWithoutItem` where the bed rule destroys on use
    /// (the bed's head at `head`): `BedBlock.destroyOnUse` removes both
    /// halves and explodes; `StrawBedBlock` just falls apart.
    pub(super) fn destroy_bed_on_use(&mut self, head: [i32; 3]) -> Result<()> {
        let state = self.world.block(head[0], head[1], head[2]);
        let name = self.res.facts().get(state).name.clone();
        let center = [
            f64::from(head[0]) + 0.5,
            f64::from(head[1]) + 0.5,
            f64::from(head[2]) + 0.5,
        ];
        if &*name == "minecraft:straw_bed" {
            // StrawBedBlock.destroyBed.
            self.sounds.play(
                "minecraft:block.straw_bed.break_leave",
                Some(center),
                1.0,
                1.0,
            );
            return self.level_set(head, self.world.air);
        }
        // removeBlock(head), then the foot behind it if it is still this bed.
        let facing = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("facing").cloned())
            .unwrap_or_default();
        let back = match facing.as_str() {
            "north" => [0, 0, 1],
            "south" => [0, 0, -1],
            "west" => [1, 0, 0],
            _ => [-1, 0, 0],
        };
        self.level_set(head, self.world.air)?;
        let foot = [head[0] + back[0], head[1], head[2] + back[2]];
        if self
            .res
            .facts()
            .get(self.world.block(foot[0], foot[1], foot[2]))
            .name
            == name
        {
            self.level_set(foot, self.world.air)?;
        }
        self.block_explosion(center, None)
    }
    /// `Level.explode(null, badRespawnPointExplosion(center), calculator,
    /// center, 5, true, ExplosionInteraction.BLOCK)` (beds and anchors):
    /// `water_at` is the block that resists like water.
    pub(super) fn block_explosion(
        &mut self,
        center: [f64; 3],
        water_at: Option<[i32; 3]>,
    ) -> Result<()> {
        let interaction = if self.world_data.rules.bool("block_explosion_drop_decay") {
            super::explosion::BlockInteraction::DestroyWithDecay
        } else {
            super::explosion::BlockInteraction::Destroy
        };
        self.server_explode_as(
            center,
            5.0,
            interaction,
            true,
            "minecraft:bad_respawn_point",
            water_at.map(|p| (p, 100.0)),
        )
    }
    pub(super) fn block_in_tag(&self, tag: &str, block: &str) -> bool {
        self.res
            .items()
            .block_tags
            .get(tag)
            .is_some_and(|s| s.contains(block))
    }
    /// `BedRule.canSetSpawn` of the bed's rule attribute (`bed_rule`,
    /// `straw_bed_rule`): only the Overworld's beds set the spawn, straw
    /// beds never.
    pub(super) fn bed_sets_spawn(&self, state: BlockStateId) -> bool {
        self.dim == Dim::Overworld && &*self.res.facts().get(state).name != "minecraft:straw_bed"
    }
}

/// `Mth.wrapDegrees(double)`.
fn wrap_degrees(degrees: f64) -> f64 {
    let mut d = degrees % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}
