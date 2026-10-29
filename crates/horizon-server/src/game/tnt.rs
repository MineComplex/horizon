//! Primed TNT (`PrimedTnt`): 80-tick fuse, gravity and drag, explosion of
//! power 4, `TntRenderer` swell and white flash, and `TntBlock.useItemOn`
//! (flint and steel / fire charge).
use super::explosion::BlockInteraction;
use crate::hc::{Result, collisions::SceneCollisions, items::ItemStack};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::level::flags as level_flags;

#[derive(Clone, Debug)]
pub(super) struct PrimedTnt {
    /// The entity id a server tracks it by (0 until it is tracked).
    pub id: u32,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub fuse: i32,
    pub on_ground: bool,
}
impl PrimedTnt {
    const SIZE: f64 = 0.98;
    pub(super) fn bounds(&self) -> Aabb {
        let h = Self::SIZE / 2.0;
        Aabb::new(
            Vec3::new(self.position.x - h, self.position.y, self.position.z - h),
            Vec3::new(
                self.position.x + h,
                self.position.y + Self::SIZE,
                self.position.z + h,
            ),
        )
    }
    pub(super) fn tick(
        &mut self,
        world: &crate::hc::world::World,
        collision: &horizon_world::block_collision::BlockCollisionRegistry<'_>,
        assets: &crate::hc::assets::Assets,
        facts: &crate::hc::blocks::BlockFacts,
    ) -> Result<()> {
        self.previous = self.position;
        self.velocity.y -= 0.04;
        let mut scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let wanted = self.velocity;
        let moved = scene
            .resolve(self.bounds(), wanted, 0.0, self.on_ground)
            .unwrap_or(Vec3::ZERO);
        self.position = Vec3::new(
            self.position.x + moved.x,
            self.position.y + moved.y,
            self.position.z + moved.z,
        );
        self.on_ground = wanted.y < 0.0 && moved.y != wanted.y;
        if moved.x != wanted.x {
            self.velocity.x = 0.0;
        }
        if moved.z != wanted.z {
            self.velocity.z = 0.0;
        }
        if moved.y != wanted.y {
            self.velocity.y = 0.0;
        }
        self.velocity = Vec3::new(
            self.velocity.x * 0.98,
            self.velocity.y * 0.98,
            self.velocity.z * 0.98,
        );
        if self.on_ground {
            self.velocity = Vec3::new(
                self.velocity.x * 0.7,
                self.velocity.y * -0.5,
                self.velocity.z * 0.7,
            );
        }
        self.fuse -= 1;
        if self.fuse > 0 {
            // PrimedTnt updates immersion after moving and decrementing its fuse.
            let mut fluids =
                horizon_world::entity_fluid::EntityFluidInteraction::new([WATER, LAVA]);
            let mut fluid_world = TntFluidWorld {
                tnt: self,
                world,
                assets,
                facts,
            };
            fluids
                .update(&mut fluid_world, false, 4096)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            fluids.apply_current(&mut fluid_world, WATER, 0.014)?;
            fluids.apply_current(
                &mut fluid_world,
                LAVA,
                if world.fast_lava {
                    0.007
                } else {
                    0.0023333333333333335
                },
            )?;
        }
        Ok(())
    }
}

impl crate::game::Server<'_> {
    /// `new PrimedTnt(level, x + 0.5, y, z + 0.5, owner)`; `short` applies
    /// `wasExploded`'s random short fuse. Plays `entity.tnt.primed` for primes.
    pub(super) fn spawn_primed_tnt(&mut self, pos: BlockPos, short: bool) {
        // PrimedTnt: the fuse is lit.
        let who = self.sculk_actor();
        self.post_block_game_event("minecraft:prime_fuse", [pos.x, pos.y, pos.z], who, None);
        let r = &mut self.world.random;
        let rot = r.next_double() * f64::from(std::f32::consts::TAU);
        let fuse = if short {
            // PrimedTnt.getRandomShortFuse
            r.next_int_bound(20).unwrap_or(0) + 10
        } else {
            self.sounds.play(
                "minecraft:entity.tnt.primed",
                Some([
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y),
                    f64::from(pos.z) + 0.5,
                ]),
                1.0,
                1.0,
            );
            80
        };
        self.add_primed_tnt(pos, rot, fuse);
    }
    /// Adds primed TNT at the block's bottom centre, pushed along `rot`.
    pub(super) fn add_primed_tnt(&mut self, pos: BlockPos, rot: f64, fuse: i32) {
        let position = Vec3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        );
        self.tnts.push(PrimedTnt {
            id: 0,
            position,
            previous: position,
            velocity: Vec3::new(-rot.sin() * 0.02, f64::from(0.2f32), -rot.cos() * 0.02),
            fuse,
            on_ground: false,
        });
    }
    /// `PrimedTnt.tick` for every primed TNT, in order; an expired fuse
    /// explodes immediately (later TNT in the list feel its knockback this
    /// tick). TNT primed by an explosion starts ticking next tick.
    pub(super) fn tick_tnt(&mut self) -> Result<()> {
        let mut count = self.tnts.len();
        let mut i = 0;
        // ServerLevel.tick: only entities in entity-ticking chunks tick.
        let ticking = self.entity_ticking_area();
        while i < count {
            let t = &mut self.tnts[i];
            if !ticking(t.position.x.floor() as i32, t.position.z.floor() as i32) {
                i += 1;
                continue;
            }
            t.tick(
                &self.world,
                self.collision,
                &self.pack.assets,
                self.res.facts(),
            )?;
            if t.fuse <= 0 {
                let center = [
                    t.position.x,
                    t.position.y + PrimedTnt::SIZE * 0.0625,
                    t.position.z,
                ];
                self.tnts.remove(i);
                count -= 1;
                self.with_hive_emptying_blast(|g| {
                    g.server_explode(center, 4.0, BlockInteraction::Destroy, false)
                })?;
            } else {
                i += 1;
            }
        }
        Ok(())
    }
    /// `TntBlock.useItemOn`: flint and steel or a fire charge primes TNT.
    pub(super) fn ignite_tnt(&mut self, stack: &ItemStack) -> Result<bool> {
        let item = &*stack.item;
        if item != "minecraft:flint_and_steel" && item != "minecraft:fire_charge" {
            return Ok(false);
        }
        let Some(hit) = self.pick() else {
            return Ok(false);
        };
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        if &*self.res.facts().get(state).name != "minecraft:tnt" {
            return Ok(false);
        }
        let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
        self.spawn_primed_tnt(pos, false);
        let air = self.world.air;
        self.with_level(|b, level, ticks| {
            b.set_block(level, ticks, pos, air, level_flags::UPDATE_ALL_IMMEDIATE)
        });
        self.process_level_events()?;
        self.swing_arm();
        if item == "minecraft:flint_and_steel" {
            self.damage_held(1);
        } else {
            self.consume_held();
        }
        Ok(true)
    }
}

use horizon_world::{
    entity_fluid::{
        CurrentTarget, FluidInteractionWorld, FluidMembership, FluidSections, FluidTagId,
    },
    fluid::{FluidHeightWorld, FluidKind, FluidState},
    fluid_flow::{FluidFlowWorld, flow_at},
};
const WATER: FluidTagId = FluidTagId(0);
const LAVA: FluidTagId = FluidTagId(1);
struct TntFluidWorld<'a> {
    tnt: &'a mut PrimedTnt,
    world: &'a crate::hc::world::World,
    assets: &'a crate::hc::assets::Assets,
    facts: &'a crate::hc::blocks::BlockFacts,
}
impl TntFluidWorld<'_> {
    fn state(&self, p: BlockPos) -> horizon_world::registry::BlockStateId {
        self.world.block(p.x, p.y, p.z)
    }
    fn fluid_at(&self, p: BlockPos) -> std::io::Result<FluidState> {
        FluidState::from_block(
            self.assets
                .registry
                .state(self.state(p))
                .ok_or_else(|| std::io::Error::other("missing TNT fluid state"))?,
        )
    }
}
impl FluidMembership for TntFluidWorld<'_> {
    fn fluid_in_tag(&self, kind: FluidKind, tag: FluidTagId) -> bool {
        match tag {
            WATER => matches!(kind, FluidKind::Water | FluidKind::FlowingWater),
            LAVA => matches!(kind, FluidKind::Lava | FluidKind::FlowingLava),
            _ => false,
        }
    }
}
impl CurrentTarget for TntFluidWorld<'_> {
    type Error = std::io::Error;
    fn is_player_class(&self) -> bool {
        false
    }
    fn velocity(&mut self) -> std::io::Result<Vec3> {
        Ok(self.tnt.velocity)
    }
    fn add_delta_movement(&mut self, v: Vec3) -> std::io::Result<()> {
        self.tnt.velocity = Vec3::new(
            self.tnt.velocity.x + v.x,
            self.tnt.velocity.y + v.y,
            self.tnt.velocity.z + v.z,
        );
        Ok(())
    }
}
impl FluidInteractionWorld for TntFluidWorld<'_> {
    type Error = std::io::Error;
    type Chunk = (i32, i32);
    fn interaction_box(&mut self) -> std::io::Result<Option<Aabb>> {
        Ok(Some(
            self.tnt.bounds().deflate(Vec3::new(0.001, 0.001, 0.001)),
        ))
    }
    fn loaded_chunk(&mut self, x: i32, z: i32) -> std::io::Result<Option<Self::Chunk>> {
        Ok(self.world.chunks.contains_key(&(x, z)).then_some((x, z)))
    }
    fn chunk_sections(&mut self, _: Self::Chunk) -> std::io::Result<FluidSections<'_>> {
        static SECTIONS: [bool; crate::hc::chunk::SECTIONS] = [true; crate::hc::chunk::SECTIONS];
        Ok(FluidSections {
            minimum_section_y: crate::hc::chunk::MIN_Y / 16,
            has_fluid: &SECTIONS,
        })
    }
    fn bounding_box_min_y(&self) -> f64 {
        self.tnt.position.y
    }
    fn block_x(&self) -> i32 {
        self.tnt.position.x.floor() as i32
    }
    fn block_z(&self) -> i32 {
        self.tnt.position.z.floor() as i32
    }
    fn eye_y(&mut self) -> std::io::Result<f64> {
        Ok(self.tnt.position.y + f64::from(0.98f32 * 0.85f32))
    }
    fn fluid_state(&mut self, p: BlockPos) -> std::io::Result<FluidState> {
        self.fluid_at(p)
    }
    fn fluid_height(&mut self, s: FluidState, p: BlockPos) -> std::io::Result<f32> {
        s.height_at(self, [p.x, p.y, p.z])
    }
    fn camera_height(&mut self, s: FluidState, p: BlockPos) -> std::io::Result<f32> {
        s.camera_height_at(self, [p.x, p.y, p.z])
    }
    fn flow(&mut self, s: FluidState, p: BlockPos) -> std::io::Result<Vec3> {
        flow_at(self, s, p)
    }
}
impl FluidHeightWorld for TntFluidWorld<'_> {
    type Error = std::io::Error;
    fn fluid(&mut self, p: [i32; 3]) -> std::io::Result<FluidState> {
        self.fluid_at(BlockPos::new(p[0], p[1], p[2]))
    }
    fn sturdy_down(&mut self, p: [i32; 3]) -> std::io::Result<bool> {
        Ok(self
            .facts
            .get(self.state(BlockPos::new(p[0], p[1], p[2])))
            .sturdy_full
            & 1
            != 0)
    }
}
impl FluidFlowWorld for TntFluidWorld<'_> {
    type Error = std::io::Error;
    type Block = horizon_world::registry::BlockStateId;
    fn fluid(&mut self, p: BlockPos) -> std::io::Result<FluidState> {
        self.fluid_at(p)
    }
    fn block(&mut self, p: BlockPos) -> std::io::Result<Self::Block> {
        Ok(self.state(p))
    }
    fn blocks_fluid_flow(&self, s: Self::Block) -> bool {
        self.assets
            .blocks_fluid_flow
            .contains(&self.assets.registry.state(s).unwrap().name)
    }
    fn is_ice_class(&self, s: Self::Block) -> bool {
        self.facts
            .get(s)
            .classes
            .iter()
            .any(|c| c.as_ref() == "IceBlock")
    }
    fn is_face_sturdy(
        &mut self,
        s: Self::Block,
        _: BlockPos,
        d: horizon_core::geometry::Direction,
    ) -> std::io::Result<bool> {
        Ok(self.facts.get(s).sturdy_full & (1 << d as u8) != 0)
    }
}
