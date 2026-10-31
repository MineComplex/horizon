//! `fossil` (`FossilFeature`): a fossil template and its overlay, placed deep underground with
//! the processors' rot.
//!
//! The templates come from `StructureTemplateManager`, which loads after the registries:
//! [`FeatureRegistry::attach_templates`](super::FeatureRegistry::attach_templates) copies the
//! few the fossils name into the feature. A feature without them places nothing and is skipped
//! by the decoration ([`Fossil::has_templates`]).

use std::borrow::Cow;

use serde_json::value::RawValue;

use super::simple::FeatureWorld;
use crate::block::flags::FLAG_AIR;
use crate::block::transform::{Mirror, Rotation};
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{DecodeError, identifier, int_range, object, required, string};
use crate::worldgen::density::StructureBounds;
use crate::worldgen::placement::PlacementWorld;
use crate::worldgen::state_provider::{ProviderContext, ProviderError};
use crate::worldgen::structure::place::{PlaceContext, PlaceSettings, place_in_world};
use crate::worldgen::structure::pool::template_of;
use crate::worldgen::structure::processor::{KnownBlocks, ProcessorList, ProcessorLists};
use crate::worldgen::structure::template::{StructureTemplate, StructureTemplates};

#[derive(Clone, Debug)]
pub struct Fossil {
    fossil_structures: Vec<String>,
    overlay_structures: Vec<String>,
    fossil_processors: String,
    overlay_processors: String,
    max_empty_corners_allowed: i32,
    loaded: Option<Loaded>,
}

/// The templates and processors the fossil names, copied from the managers.
#[derive(Clone, Debug)]
struct Loaded {
    fossils: Vec<StructureTemplate>,
    overlays: Vec<StructureTemplate>,
    fossil_processors: ProcessorList,
    overlay_processors: ProcessorList,
    known: KnownBlocks,
}

impl Fossil {
    pub fn decode(raw: &RawValue, _: &ProviderContext<'_>) -> Result<Self, DecodeError> {
        let fields = object(raw)?;
        let ids = |name: &'static str| -> Result<Vec<String>, DecodeError> {
            let list: Vec<String> = serde_json::from_str(required(&fields, name)?.get())?;
            if list.is_empty() {
                return Err(DecodeError::Field(name));
            }
            list.iter().map(|id| identifier(id)).collect()
        };
        let processors = |name: &'static str| -> Result<String, DecodeError> {
            // `StructureProcessorType.LIST_CODEC` by id (the fossils name registered lists).
            identifier(&string(required(&fields, name)?)?)
        };
        let fossil_structures = ids("fossil_structures")?;
        let overlay_structures = ids("overlay_structures")?;
        if fossil_structures.len() != overlay_structures.len() {
            return Err(DecodeError::Field("fossil structure lists' lengths"));
        }
        Ok(Self {
            fossil_structures,
            overlay_structures,
            fossil_processors: processors("fossil_processors")?,
            overlay_processors: processors("overlay_processors")?,
            max_empty_corners_allowed: int_range(
                required(&fields, "max_empty_corners_allowed")?,
                "max_empty_corners_allowed",
                0,
                7,
            )?,
            loaded: None,
        })
    }

    /// Copies the templates and processor lists the fossil names.
    pub(super) fn attach_templates(
        &mut self,
        templates: &StructureTemplates,
        processors: &ProcessorLists,
    ) {
        let get = |ids: &[String]| -> Vec<StructureTemplate> {
            ids.iter()
                .map(|id| template_of(templates, id).clone())
                .collect()
        };
        let list = |name: &str| {
            processors
                .id(name)
                .map(|id| processors.get(id).clone())
                .unwrap_or_default()
        };
        self.loaded = Some(Loaded {
            fossils: get(&self.fossil_structures),
            overlays: get(&self.overlay_structures),
            fossil_processors: list(&self.fossil_processors),
            overlay_processors: list(&self.overlay_processors),
            known: processors.known().clone(),
        });
    }

    /// Whether the templates are attached (the decoration skips the fossil otherwise).
    pub fn has_templates(&self) -> bool {
        self.loaded.is_some()
    }

    /// `FossilFeature.place`.
    pub fn place<W: FeatureWorld>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: BlockPos,
    ) -> Result<bool, ProviderError> {
        let rotation = [
            Rotation::None,
            Rotation::Clockwise90,
            Rotation::Clockwise180,
            Rotation::Counterclockwise90,
        ][random.next_int_bound(4)? as usize];
        let index = random.next_int_bound(self.fossil_structures.len() as i32)? as usize;
        // The template manager is reached through the level, after these two draws.
        let loaded = self.loaded.as_ref().ok_or(ProviderError::Invalid(
            "the structure templates are not loaded",
        ))?;
        let base = &loaded.fossils[index];
        let overlay = &loaded.overlays[index];
        let (chunk_x, chunk_z) = (origin.x >> 4 << 4, origin.z >> 4 << 4);
        let level = world.level_generation();
        let max_y = level.min_y.wrapping_add(level.height).wrapping_sub(1);
        let bounds = StructureBounds::new(
            [chunk_x - 16, level.min_y, chunk_z - 16],
            [chunk_x + 15 + 16, max_y, chunk_z + 15 + 16],
        )
        .ok_or(ProviderError::Invalid("fossil bounding box"))?;
        let size = base.rotated_size(rotation);
        let low_corner = origin.offset(-size[0] / 2, 0, -size[2] / 2);
        let mut lowest_surface_y = origin.y;
        for x in 0..size[0] {
            for z in 0..size[2] {
                lowest_surface_y = lowest_surface_y.min(world.height(
                    Heightmap::OceanFloorWg,
                    low_corner.x + x,
                    low_corner.z + z,
                )?);
            }
        }
        let target_y = (lowest_surface_y - 15 - random.next_int_bound(10)?).max(level.min_y + 10);
        let target = base.zero_position_with_transform(
            BlockPos::new(low_corner.x, target_y, low_corner.z),
            Mirror::None,
            rotation,
        );
        let corners = base.bounding_box(target, rotation, BlockPos::default(), Mirror::None);
        if self.count_empty_corners(world, corners)? > self.max_empty_corners_allowed {
            return Ok(false);
        }
        let seed = world.world_seed()?;
        let cx = PlaceContext {
            known: &loaded.known,
            seed,
        };
        for (template, processors) in [
            (base, &loaded.fossil_processors),
            (overlay, &loaded.overlay_processors),
        ] {
            let settings = PlaceSettings {
                rotation,
                bounding_box: Some(bounds),
                shared_random: true,
                processors: processors.0.iter().map(Cow::Borrowed).collect(),
                ..PlaceSettings::default()
            };
            // `Block.UPDATE_NONE`.
            place_in_world(world, cx, template, target, target, &settings, random, 260)
                .map_err(|_| ProviderError::Invalid("a fossil template could not be placed"))?;
        }
        Ok(true)
    }

    /// `countEmptyCorners`: corners of the box that are air, lava or water.
    fn count_empty_corners(
        &self,
        world: &impl PlacementWorld,
        bounds: StructureBounds,
    ) -> Result<i32, ProviderError> {
        let (min, max) = (bounds.minimum(), bounds.maximum());
        let mut count = 0;
        for x in [min[0], max[0]] {
            for y in [min[1], max[1]] {
                for z in [min[2], max[2]] {
                    let state = world.block_state(BlockPos::new(x, y, z))?;
                    if world.state_data().has(state, FLAG_AIR)
                        || super::common::is_block(world, state, "minecraft:lava")
                        || super::common::is_block(world, state, "minecraft:water")
                    {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }
}
