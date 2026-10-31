//! `huge_brown_mushroom` and `huge_red_mushroom` (`AbstractHugeMushroomFeature`).

use serde_json::value::RawValue;

use super::simple::FeatureWorld;
use crate::block::BlockState;
use crate::block::flags::FLAG_AIR;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, int_range, object, optional, required};
use crate::worldgen::placement::{BlockPredicate, PlacementWorld};
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext, ProviderError};
use crate::worldgen::structure::BiomeSet;

#[derive(Clone, Debug)]
pub struct HugeMushroom {
    /// `HugeRedMushroomFeature`; false for the brown one.
    red: bool,
    cap: BlockStateProvider,
    stem: BlockStateProvider,
    foliage_radius: i32,
    can_place_on: BlockPredicate,
    /// `BlockTags.REPLACEABLE_BY_MUSHROOMS` and `BlockTags.LEAVES`.
    replaceable: BiomeSet,
    leaves: BiomeSet,
}

impl HugeMushroom {
    pub fn decode(
        red: bool,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        Ok(Self {
            red,
            cap: BlockStateProvider::decode(required(&fields, "cap_provider")?, ctx)?,
            stem: BlockStateProvider::decode(required(&fields, "stem_provider")?, ctx)?,
            foliage_radius: optional(&fields, "foliage_radius").map_or(Ok(2), |r| {
                int_range(r, "foliage_radius", i32::MIN, i32::MAX)
            })?,
            can_place_on: BlockPredicate::decode(required(&fields, "can_place_on")?, ctx.names)?,
            replaceable: ctx.names.block_tag("minecraft:replaceable_by_mushrooms")?,
            leaves: ctx.names.block_tag("minecraft:leaves")?,
        })
    }

    /// `getTreeRadiusForHeight(-1, -1, foliageRadius, yo)` with the tree's height.
    fn radius_for_height(&self, tree_height: i32, yo: i32) -> i32 {
        if self.red {
            if (yo < tree_height && yo >= tree_height - 3) || yo == tree_height {
                self.foliage_radius
            } else {
                0
            }
        } else if yo <= 3 {
            0
        } else {
            self.foliage_radius
        }
    }

    /// `AbstractHugeMushroomFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let mut tree_height = random.next_int_bound(3)? + 4;
        if random.next_int_bound(12)? == 0 {
            tree_height *= 2;
        }
        if !self.valid_position(world, origin, tree_height)? {
            return Ok(false);
        }
        if self.red {
            self.red_cap(world, random, origin, tree_height)?;
        } else {
            self.brown_cap(world, random, origin, tree_height)?;
        }
        for dy in 0..tree_height {
            let state = self.stem.state(world, random, origin)?;
            self.place_block(world, origin.offset(0, dy, 0), state)?;
        }
        Ok(true)
    }

    fn valid_position(
        &self,
        world: &impl PlacementWorld,
        origin: BlockPos,
        tree_height: i32,
    ) -> Result<bool, ProviderError> {
        let level = world.level_generation();
        let max_y = level.min_y.wrapping_add(level.height).wrapping_sub(1);
        if origin.y < level.min_y + 1 || origin.y + tree_height + 1 > max_y {
            return Ok(false);
        }
        if !self.can_place_on.test(world, origin.offset(0, -1, 0))? {
            return Ok(false);
        }
        for dy in 0..=tree_height {
            // `isValidPosition` passes (-1, -1) as the trunk and tree heights.
            let radius = self.radius_for_height(-1, dy);
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let state = world.block_state(origin.offset(dx, dy, dz))?;
                    let block = world.blocks().block(state).0;
                    if !world.state_data().has(state, FLAG_AIR) && !self.leaves.contains(block) {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    fn place_block<W: FeatureWorld>(
        &self,
        world: &mut W,
        pos: BlockPos,
        state: BlockState,
    ) -> Result<(), ProviderError> {
        let current = world.block_state(pos)?;
        let block = world.blocks().block(current).0;
        if world.state_data().has(current, FLAG_AIR) || self.replaceable.contains(block) {
            world.set_block(pos, state, 3)?;
        }
        Ok(())
    }

    /// The cap state with its faces set (`hasProperty` first: a provider may give another block).
    fn with_faces(
        world: &impl PlacementWorld,
        state: BlockState,
        faces: &[(&str, bool)],
    ) -> BlockState {
        let blocks = world.blocks();
        let block = blocks.block(state);
        if faces
            .iter()
            .any(|(name, _)| blocks.property(block, name).is_none())
        {
            return state;
        }
        faces.iter().fold(state, |state, (name, on)| {
            blocks
                .with_property(state, name, if *on { "true" } else { "false" })
                .unwrap_or(state)
        })
    }

    fn brown_cap<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        tree_height: i32,
    ) -> Result<(), ProviderError> {
        let r = self.foliage_radius;
        for dx in -r..=r {
            for dz in -r..=r {
                let (min_x, max_x) = (dx == -r, dx == r);
                let (min_z, max_z) = (dz == -r, dz == r);
                let x_edge = min_x || max_x;
                let z_edge = min_z || max_z;
                if !x_edge || !z_edge {
                    let pos = origin.offset(dx, tree_height, dz);
                    let west = min_x || z_edge && dx == 1 - r;
                    let east = max_x || z_edge && dx == r - 1;
                    let north = min_z || x_edge && dz == 1 - r;
                    let south = max_z || x_edge && dz == r - 1;
                    let state = self.cap.state(world, random, origin)?;
                    let state = Self::with_faces(
                        world,
                        state,
                        &[
                            ("west", west),
                            ("east", east),
                            ("north", north),
                            ("south", south),
                        ],
                    );
                    self.place_block(world, pos, state)?;
                }
            }
        }
        Ok(())
    }

    fn red_cap<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
        tree_height: i32,
    ) -> Result<(), ProviderError> {
        for dy in tree_height - 3..=tree_height {
            let radius = if dy < tree_height {
                self.foliage_radius
            } else {
                self.foliage_radius - 1
            };
            let center = self.foliage_radius - 2;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let x_edge = dx == -radius || dx == radius;
                    let z_edge = dz == -radius || dz == radius;
                    if dy >= tree_height || x_edge != z_edge {
                        let pos = origin.offset(dx, dy, dz);
                        let state = self.cap.state(world, random, origin)?;
                        let state = Self::with_faces(
                            world,
                            state,
                            &[
                                ("up", dy >= tree_height - 1),
                                ("west", dx < -center),
                                ("east", dx > center),
                                ("north", dz < -center),
                                ("south", dz > center),
                            ],
                        );
                        self.place_block(world, pos, state)?;
                    }
                }
            }
        }
        Ok(())
    }
}
