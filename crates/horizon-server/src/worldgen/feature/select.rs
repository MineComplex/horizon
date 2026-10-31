//! The features that choose other placed features: `random_selector`, `simple_random_selector`,
//! `random_boolean_selector`, `weighted_random_selector`, `sequence` and `overlay`.
//!
//! A configured feature names placed features, and placed features name configured ones, so the
//! references are decoded as JSON first ([`PlacedRefs::pending`]) and resolved to ids once the
//! registries are loaded ([`FeatureRegistry::load`](super::FeatureRegistry::load)).

use serde_json::value::RawValue;

use super::simple::FeatureWorld;
use super::{FeatureRegistry, PlacedFeatureId};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, float, int_range, object, required};
use crate::worldgen::state_provider::ProviderError;

/// A `Holder<PlacedFeature>` or `HolderSet<PlacedFeature>` of a configuration: its JSON until the
/// registries are loaded, then the ids (one for a holder).
#[derive(Clone, Debug)]
pub struct PlacedRefs {
    pending: Option<Box<RawValue>>,
    ids: Vec<PlacedFeatureId>,
}

impl PlacedRefs {
    pub(super) fn new(raw: &RawValue) -> Self {
        Self {
            pending: Some(raw.to_owned()),
            ids: Vec::new(),
        }
    }
    /// The JSON still to resolve.
    pub(super) fn take_pending(&mut self) -> Option<Box<RawValue>> {
        self.pending.take()
    }
    pub(super) fn resolve(&mut self, ids: Vec<PlacedFeatureId>) {
        self.ids = ids;
    }
    pub fn ids(&self) -> &[PlacedFeatureId] {
        &self.ids
    }
    /// A holder is one placed feature.
    fn single(&self) -> Result<PlacedFeatureId, ProviderError> {
        match self.ids.as_slice() {
            [id] => Ok(*id),
            _ => Err(ProviderError::Invalid(
                "a placed feature holder is not one feature",
            )),
        }
    }
}

/// A selector feature.
#[derive(Clone, Debug)]
pub enum Selector {
    /// `RandomSelectorFeature`: the first entry whose chance the next float is below, else the default.
    Random {
        features: Vec<(PlacedRefs, f32)>,
        default: PlacedRefs,
    },
    /// `SimpleRandomSelectorFeature`.
    Simple(PlacedRefs),
    /// `RandomBooleanSelectorFeature`.
    Boolean {
        on_true: PlacedRefs,
        on_false: PlacedRefs,
    },
    /// `WeightedRandomSelectorFeature`.
    Weighted {
        features: Vec<(PlacedRefs, i32)>,
        total: i32,
    },
    /// `SequenceFeature`: all, stopping at the first that places nothing.
    Sequence(PlacedRefs),
    /// `OverlayFeature`: all, true when any placed.
    Overlay(PlacedRefs),
}

impl Selector {
    pub fn decode(kind: &str, raw: &RawValue) -> Result<Option<Self>, DecodeError> {
        let fields = object(raw)?;
        Ok(Some(match kind {
            "minecraft:random_selector" => {
                let list: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "features")?.get())?;
                let features = list
                    .iter()
                    .map(|entry| {
                        let entry = object(entry)?;
                        let chance = float(required(&entry, "chance")?)?;
                        if !(0.0..=1.0).contains(&chance) {
                            return Err(DecodeError::Field("chance"));
                        }
                        Ok((PlacedRefs::new(required(&entry, "feature")?), chance))
                    })
                    .collect::<Result<_, DecodeError>>()?;
                Self::Random {
                    features,
                    default: PlacedRefs::new(required(&fields, "default")?),
                }
            }
            "minecraft:simple_random_selector" => {
                Self::Simple(PlacedRefs::new(required(&fields, "features")?))
            }
            "minecraft:random_boolean_selector" => Self::Boolean {
                on_true: PlacedRefs::new(required(&fields, "feature_true")?),
                on_false: PlacedRefs::new(required(&fields, "feature_false")?),
            },
            "minecraft:weighted_random_selector" => {
                let list: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "features")?.get())?;
                let mut total = 0_i32;
                let mut features = Vec::with_capacity(list.len());
                for entry in &list {
                    let entry = object(entry)?;
                    let weight = int_range(required(&entry, "weight")?, "weight", 0, i32::MAX)?;
                    total = total
                        .checked_add(weight)
                        .ok_or(DecodeError::Field("total weight"))?;
                    features.push((PlacedRefs::new(required(&entry, "data")?), weight));
                }
                Self::Weighted { features, total }
            }
            "minecraft:sequence" => Self::Sequence(PlacedRefs::new(required(&fields, "features")?)),
            "minecraft:overlay" => Self::Overlay(PlacedRefs::new(required(&fields, "features")?)),
            _ => return Ok(None),
        }))
    }

    /// Every reference, for the registry to resolve.
    pub(super) fn refs_mut(&mut self) -> Vec<&mut PlacedRefs> {
        match self {
            Self::Random { features, default } => features
                .iter_mut()
                .map(|(refs, _)| refs)
                .chain(std::iter::once(default))
                .collect(),
            Self::Simple(refs) | Self::Sequence(refs) | Self::Overlay(refs) => vec![refs],
            Self::Boolean { on_true, on_false } => vec![on_true, on_false],
            Self::Weighted { features, .. } => features.iter_mut().map(|(refs, _)| refs).collect(),
        }
    }

    /// Every placed feature it may place.
    pub fn placed(&self) -> Vec<PlacedFeatureId> {
        match self {
            Self::Random { features, default } => features
                .iter()
                .flat_map(|(refs, _)| refs.ids())
                .chain(default.ids())
                .copied()
                .collect(),
            Self::Simple(refs) | Self::Sequence(refs) | Self::Overlay(refs) => refs.ids().to_vec(),
            Self::Boolean { on_true, on_false } => on_true
                .ids()
                .iter()
                .chain(on_false.ids())
                .copied()
                .collect(),
            Self::Weighted { features, .. } => features
                .iter()
                .flat_map(|(refs, _)| refs.ids())
                .copied()
                .collect(),
        }
    }

    pub fn place<W: FeatureWorld, R: RandomSource>(
        &self,
        registry: &FeatureRegistry,
        world: &mut W,
        random: &mut R,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        match self {
            Self::Random { features, default } => {
                for (refs, chance) in features {
                    if random.next_float() < *chance {
                        return registry.place_placed(refs.single()?, world, random, origin);
                    }
                }
                registry.place_placed(default.single()?, world, random, origin)
            }
            Self::Simple(refs) => {
                let index = random.next_int_bound(refs.ids().len() as i32)?;
                registry.place_placed(refs.ids()[index as usize], world, random, origin)
            }
            Self::Boolean { on_true, on_false } => {
                let chosen = if random.next_boolean() {
                    on_true
                } else {
                    on_false
                };
                registry.place_placed(chosen.single()?, world, random, origin)
            }
            Self::Weighted { features, total } => {
                if *total == 0 {
                    return Ok(false);
                }
                let mut selection = random.next_int_bound(*total)?;
                for (refs, weight) in features {
                    selection -= weight;
                    if selection < 0 {
                        return registry.place_placed(refs.single()?, world, random, origin);
                    }
                }
                Ok(false)
            }
            Self::Sequence(refs) => {
                for &id in refs.ids() {
                    if !registry.place_placed(id, world, random, origin)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Overlay(refs) => {
                let mut placed_any = false;
                for &id in refs.ids() {
                    placed_any |= registry.place_placed(id, world, random, origin)?;
                }
                Ok(placed_any)
            }
        }
    }
}
