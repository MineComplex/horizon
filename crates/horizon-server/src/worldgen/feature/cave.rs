//! The underground and vegetation features of WP4's second part: dispatch over the types of
//! [`multiface`](super::multiface) and the files beside it.

use serde_json::value::RawValue;

use super::FeatureRegistry;
use super::multiface::MultifaceGrowth;
use super::simple::FeatureWorld;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::DecodeError;
use crate::worldgen::state_provider::{ProviderContext, ProviderError};

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum Cave {
    Multiface(Box<MultifaceGrowth>),
}

impl Cave {
    pub fn decode(
        kind: &str,
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
    ) -> Result<Option<Self>, DecodeError> {
        Ok(Some(match kind {
            "minecraft:multiface_growth" => {
                Self::Multiface(Box::new(MultifaceGrowth::decode(raw, ctx)?))
            }
            _ => return Ok(None),
        }))
    }

    pub fn place<W: FeatureWorld>(
        &self,
        _registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::Multiface(growth) => growth.place(world, random, origin),
        }
    }
}
