//! `FeaturePlacer`: runs a placed feature's modifier chain, depth first, and hands every position
//! that comes out of the last modifier to the feature.

use super::world::PlacementWorld;
use super::{PlacementContext, PlacementError, PlacementModifier};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::feature::{FeatureRegistry, PlacedFeatureId};

/// Why a placement stopped: a modifier, or the feature placed at a position.
#[derive(Debug, thiserror::Error)]
pub enum PlaceError<E> {
    #[error(transparent)]
    Placement(#[from] PlacementError),
    #[error("feature failed")]
    Feature(E),
}

/// The buffers vanilla keeps between placements, so a level reuses one placer.
#[derive(Debug, Default)]
pub struct FeaturePlacer {
    positions: Vec<(BlockPos, usize)>,
    modified: Vec<BlockPos>,
}

impl FeaturePlacer {
    /// `FeaturePlacer.place` (`biome_check` false) and `placeWithBiomeCheck`. `feature` places
    /// the configured feature at a position and says whether it did; it shares the world and the
    /// random with the modifiers, so later positions see what earlier ones placed.
    #[allow(clippy::too_many_arguments)]
    pub fn place<W: PlacementWorld, R: RandomSource, E>(
        &mut self,
        features: &FeatureRegistry,
        placed: PlacedFeatureId,
        biome_check: bool,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
        feature: impl FnMut(&mut W, &mut R, BlockPos) -> Result<bool, E>,
    ) -> Result<bool, PlaceError<E>> {
        let top = biome_check.then_some(placed);
        let modifiers = &features.placed(placed).placement;
        self.place_chain(features, modifiers, top, world, random, origin, feature)
    }

    /// The same for a modifier chain that is not a registered placed feature; `top` is the
    /// placed feature the biome filter checks.
    #[allow(clippy::too_many_arguments)]
    pub fn place_chain<W: PlacementWorld, R: RandomSource, E>(
        &mut self,
        features: &FeatureRegistry,
        modifiers: &[PlacementModifier],
        top: Option<PlacedFeatureId>,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
        mut feature: impl FnMut(&mut W, &mut R, BlockPos) -> Result<bool, E>,
    ) -> Result<bool, PlaceError<E>> {
        if modifiers.is_empty() {
            return feature(world, random, origin).map_err(PlaceError::Feature);
        }
        let mut placed_any = false;
        self.positions.clear();
        self.modified.clear();
        self.positions.push((origin, 0));
        while let Some((pos, index)) = self.positions.pop() {
            let cx = PlacementContext {
                world: &*world,
                features,
                top,
            };
            modifiers[index].modify(&cx, random, pos, &mut self.modified)?;
            let next = index + 1;
            if next < modifiers.len() {
                self.positions
                    .extend(self.modified.iter().rev().map(|&p| (p, next)));
            } else {
                for &p in &self.modified {
                    placed_any |= feature(world, random, p).map_err(PlaceError::Feature)?;
                }
            }
            self.modified.clear();
        }
        Ok(placed_any)
    }
}
