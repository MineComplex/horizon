//! The underground and vegetation features of WP4's second part: dispatch over the types of
//! [`multiface`](super::multiface) and the files beside it.

use serde_json::value::RawValue;

use super::dripstone::{LargeDripstone, Speleothem, SpeleothemCluster};
use super::fossil::Fossil;
use super::geode::Geode;
use super::monster_room::MonsterRoom;
use super::multiface::MultifaceGrowth;
use super::root_system::RootSystem;
use super::sculk::SculkPatch;
use super::select::PlacedRefs;
use super::simple::FeatureWorld;
use super::{FeatureRegistry, PlacedFeatureId};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::DecodeError;
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::processor::ProcessorLists;
use crate::worldgen::structure::template::StructureTemplates;

/// A feature of this module.
#[derive(Clone, Debug)]
pub enum Cave {
    Multiface(Box<MultifaceGrowth>),
    Geode(Box<Geode>),
    MonsterRoom(Box<MonsterRoom>),
    Speleothem(Box<Speleothem>),
    SpeleothemCluster(Box<SpeleothemCluster>),
    LargeDripstone(Box<LargeDripstone>),
    RootSystem(Box<RootSystem>),
    SculkPatch(Box<SculkPatch>),
    Fossil(Box<Fossil>),
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
            "minecraft:geode" => Self::Geode(Box::new(Geode::decode(raw, ctx)?)),
            "minecraft:monster_room" => Self::MonsterRoom(Box::new(MonsterRoom::new(ctx.names)?)),
            "minecraft:speleothem" => Self::Speleothem(Box::new(Speleothem::decode(raw, ctx)?)),
            "minecraft:speleothem_cluster" => {
                Self::SpeleothemCluster(Box::new(SpeleothemCluster::decode(raw, ctx)?))
            }
            "minecraft:large_dripstone" => {
                Self::LargeDripstone(Box::new(LargeDripstone::decode(raw, ctx)?))
            }
            "minecraft:fossil" => Self::Fossil(Box::new(Fossil::decode(raw, ctx)?)),
            "minecraft:sculk_patch" => Self::SculkPatch(Box::new(SculkPatch::decode(raw, ctx)?)),
            "minecraft:root_system" => Self::RootSystem(Box::new(RootSystem::decode(raw, ctx)?)),
            _ => return Ok(None),
        }))
    }

    /// The placed feature the feature names, to resolve once the registry is loaded.
    pub(super) fn refs_mut(&mut self) -> Option<&mut PlacedRefs> {
        match self {
            Self::RootSystem(f) => Some(f.refs_mut()),
            _ => None,
        }
    }

    /// Gives a fossil its templates.
    pub(super) fn attach_templates(
        &mut self,
        templates: &StructureTemplates,
        processors: &ProcessorLists,
    ) {
        if let Self::Fossil(f) = self {
            f.attach_templates(templates, processors);
        }
    }

    /// Whether everything the feature needs is there (a fossil needs its templates).
    pub fn ready(&self) -> bool {
        match self {
            Self::Fossil(f) => f.has_templates(),
            _ => true,
        }
    }

    /// The placed features the feature places.
    pub fn placed(&self) -> &[PlacedFeatureId] {
        match self {
            Self::RootSystem(f) => f.placed(),
            _ => &[],
        }
    }

    pub fn place<W: FeatureWorld>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::Multiface(f) => f.place(world, random, origin),
            Self::Geode(f) => f.place(world, random, origin),
            Self::MonsterRoom(f) => f.place(world, random, origin),
            Self::Speleothem(f) => f.place(world, random, origin),
            Self::SpeleothemCluster(f) => f.place(world, random, origin),
            Self::LargeDripstone(f) => f.place(world, random, origin),
            Self::RootSystem(f) => f.place(registry, world, random, origin),
            Self::SculkPatch(f) => f.place(world, random, origin),
            Self::Fossil(f) => f.place(world, random, origin),
        }
    }
}
