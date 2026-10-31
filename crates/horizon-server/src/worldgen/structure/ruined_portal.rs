//! Ruined portals (`RuinedPortalStructure`, `RuinedPortalPiece`).

use serde_json::value::RawValue;

use super::Structure;
use super::template_pieces::{
    PieceCx, PieceKind, PortalProperties, TemplatePiece, VerticalPlacement, direction_name,
};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld};
use crate::block::transform::{Mirror, Rotation};
use crate::block::{BlockId, BlockState};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::codec::{boolean, float, object, required, string};
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::state_provider::ProviderError;

const PORTALS: [&str; 10] = [
    "ruined_portal/portal_1",
    "ruined_portal/portal_2",
    "ruined_portal/portal_3",
    "ruined_portal/portal_4",
    "ruined_portal/portal_5",
    "ruined_portal/portal_6",
    "ruined_portal/portal_7",
    "ruined_portal/portal_8",
    "ruined_portal/portal_9",
    "ruined_portal/portal_10",
];
const GIANT_PORTALS: [&str; 3] = [
    "ruined_portal/giant_portal_1",
    "ruined_portal/giant_portal_2",
    "ruined_portal/giant_portal_3",
];
const MIN_Y_INDEX: i32 = 15;

/// `RuinedPortalStructure.Setup`.
struct Setup {
    placement: VerticalPlacement,
    air_pocket_probability: f32,
    mossiness: f32,
    overgrown: bool,
    vines: bool,
    can_be_cold: bool,
    replace_with_blackstone: bool,
    weight: f32,
}

fn decode_setup(raw: &RawValue) -> Result<Setup, GenError> {
    let fields = object(raw)?;
    let unit = |name: &'static str| -> Result<f32, GenError> {
        let value = float(required(&fields, name)?)?;
        if (0.0..=1.0).contains(&value) {
            Ok(value)
        } else {
            Err(GenError::Config(name.to_owned()))
        }
    };
    let flag = |name: &'static str| Ok::<_, GenError>(boolean(required(&fields, name)?)?);
    let weight = float(required(&fields, "weight")?)?;
    if weight <= 0.0 {
        return Err(GenError::Config("weight".into()));
    }
    let placement = string(required(&fields, "placement")?)?;
    Ok(Setup {
        placement: VerticalPlacement::from_name(&placement)
            .ok_or_else(|| GenError::Config("placement".into()))?,
        air_pocket_probability: unit("air_pocket_probability")?,
        mossiness: unit("mossiness")?,
        overgrown: flag("overgrown")?,
        vines: flag("vines")?,
        can_be_cold: flag("can_be_cold")?,
        replace_with_blackstone: flag("replace_with_blackstone")?,
        weight,
    })
}

/// `sample(random, limit)`.
fn sample<R: RandomSource>(random: &mut R, limit: f32) -> bool {
    limit != 0.0 && (limit == 1.0 || random.next_float() < limit)
}

/// `Mth.randomBetweenInclusive`.
fn between<R: RandomSource>(
    random: &mut R,
    min: i32,
    max: i32,
) -> Result<i32, crate::util::random::InvalidBound> {
    Ok(random.next_int_bound(max - min + 1)? + min)
}

fn height_map_type(placement: VerticalPlacement) -> Heightmap {
    if placement == VerticalPlacement::OnOceanFloor {
        Heightmap::OceanFloorWg
    } else {
        Heightmap::WorldSurfaceWg
    }
}

/// `RuinedPortalStructure.findGenerationPoint` and its pieces.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    structure: &Structure,
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    let fields = object(&structure.json)?;
    let raw: Vec<Box<RawValue>> = serde_json::from_str(required(&fields, "setups")?.get())
        .map_err(|e| GenError::Config(e.to_string()))?;
    let setups = raw
        .iter()
        .map(|s| decode_setup(s))
        .collect::<Result<Vec<_>, _>>()?;
    if setups.is_empty() {
        return Err(GenError::Config("setups".into()));
    }
    let chosen = if setups.len() > 1 {
        let total = setups.iter().fold(0.0f32, |t, s| t + s.weight);
        let mut pick = random.next_float();
        let mut chosen = None;
        for setup in &setups {
            pick -= setup.weight / total;
            if pick < 0.0 {
                chosen = Some(setup);
                break;
            }
        }
        chosen.ok_or_else(|| GenError::Config("no setup chosen".into()))?
    } else {
        &setups[0]
    };
    let air_pocket = sample(random, chosen.air_pocket_probability);
    let location = if random.next_float() < 0.05 {
        GIANT_PORTALS[random.next_int_bound(GIANT_PORTALS.len() as i32)? as usize]
    } else {
        PORTALS[random.next_int_bound(PORTALS.len() as i32)? as usize]
    };
    let location = format!("minecraft:{location}");
    let template = super::pool::template_of(&cx.registry.templates, &location);
    let size = template.size();
    let rotation = Rotation::ALL[random.next_int_bound(4)? as usize];
    let mirror = if random.next_float() < 0.5 {
        Mirror::None
    } else {
        Mirror::FrontBack
    };
    let pivot = BlockPos::new(size[0] / 2, 0, size[2] / 2);
    let (min_x, min_z) = cx.min_block();
    let base = BlockPos::new(min_x, 0, min_z);
    let bounds = template.bounding_box(base, rotation, pivot, mirror);
    let (lo, hi) = (bounds.minimum(), bounds.maximum());
    let center = (
        lo[0] + (hi[0] - lo[0] + 1) / 2,
        lo[2] + (hi[2] - lo[2] + 1) / 2,
    );
    let surface_y = cx.base_height(center.0, center.1, height_map_type(chosen.placement)) - 1;
    let y_span = hi[1] - lo[1] + 1;
    let projected_y = find_suitable_y(
        cx,
        random,
        chosen.placement,
        air_pocket,
        surface_y,
        y_span,
        [lo, hi],
    )?;
    let origin = BlockPos::new(base.x, projected_y, base.z);
    let cold = chosen.can_be_cold && {
        let biome = cx.world.biome_at(origin);
        cx.climates
            .cold_enough_to_snow(biome, origin, cx.generation.sea_level)
    };
    let piece = TemplatePiece::new(
        cx.registry,
        PieceKind::RuinedPortal {
            placement: chosen.placement,
            properties: PortalProperties {
                cold,
                mossiness_bits: chosen.mossiness.to_bits(),
                air_pocket,
                overgrown: chosen.overgrown,
                vines: chosen.vines,
                replace_with_blackstone: chosen.replace_with_blackstone,
            },
        },
        location,
        origin,
        rotation,
        mirror,
        pivot,
    );
    Ok(Some(Stub {
        position: origin,
        pieces: vec![piece],
    }))
}

/// `getRandomWithinInterval`.
fn within_interval<R: RandomSource>(
    random: &mut R,
    min_preferred: i32,
    max: i32,
) -> Result<i32, GenError> {
    Ok(if min_preferred < max {
        between(random, min_preferred, max)?
    } else {
        max
    })
}

/// `findSuitableY`.
fn find_suitable_y<W: TemplateGenWorld, R: RandomSource>(
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
    placement: VerticalPlacement,
    air_pocket: bool,
    surface_y_at_center: i32,
    y_span: i32,
    [lo, hi]: [[i32; 3]; 2],
) -> Result<i32, GenError> {
    let min_y = cx.min_y + MIN_Y_INDEX;
    let new_y = match placement {
        VerticalPlacement::InNether => {
            if air_pocket {
                between(random, 32, 100)?
            } else if random.next_float() < 0.5 {
                between(random, 27, 29)?
            } else {
                between(random, 29, 100)?
            }
        }
        VerticalPlacement::InMountain => within_interval(random, 70, surface_y_at_center - y_span)?,
        VerticalPlacement::Underground => {
            within_interval(random, min_y, surface_y_at_center - y_span)?
        }
        VerticalPlacement::PartlyBuried => surface_y_at_center - y_span + between(random, 2, 8)?,
        _ => surface_y_at_center,
    };
    let corners = [
        (lo[0], lo[2]),
        (hi[0], lo[2]),
        (lo[0], hi[2]),
        (hi[0], hi[2]),
    ];
    let columns: Vec<_> = corners.iter().map(|&(x, z)| cx.base_column(x, z)).collect();
    let heightmap = if placement == VerticalPlacement::OnOceanFloor {
        Heightmap::OceanFloorWg
    } else {
        Heightmap::WorldSurfaceWg
    };
    let mut projected_y = new_y;
    while projected_y > min_y {
        let mut solid = 0;
        for column in &columns {
            if cx.world.is_opaque(column.block(projected_y), heightmap) {
                solid += 1;
                if solid == 3 {
                    return Ok(projected_y);
                }
            }
        }
        projected_y -= 1;
    }
    Ok(projected_y)
}

/// `RuinedPortalPiece.postProcess`.
pub fn post_process<W: FeatureWorld, R: RandomSource>(
    piece: &mut TemplatePiece,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let PieceKind::RuinedPortal {
        placement,
        properties,
    } = piece.kind
    else {
        return Ok(());
    };
    let bounds = piece.template_box(cx.tables.structures);
    let (lo, hi) = (bounds.minimum(), bounds.maximum());
    let center = [
        lo[0] + (hi[0] - lo[0] + 1) / 2,
        lo[1] + (hi[1] - lo[1] + 1) / 2,
        lo[2] + (hi[2] - lo[2] + 1) / 2,
    ];
    if !cx.chunk_box.contains(center) {
        return Ok(());
    }
    *cx.chunk_box = cx.chunk_box.encapsulating(bounds);
    piece.post_process_base(cx)?;
    let portal = Portal {
        placement,
        properties,
        bounds: piece.bounding_box,
    };
    portal.spread_netherrack(cx)?;
    portal.add_netherrack_drip_columns_below_portal(cx)?;
    if properties.vines || properties.overgrown {
        let (lo, hi) = (portal.bounds.minimum(), portal.bounds.maximum());
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    let pos = BlockPos::new(x, y, z);
                    if properties.vines {
                        portal.maybe_add_vines(cx, pos)?;
                    }
                    if properties.overgrown {
                        portal.maybe_add_leaves_above(cx, pos)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// The piece's fields the placement helpers read.
struct Portal {
    placement: VerticalPlacement,
    properties: PortalProperties,
    bounds: crate::worldgen::density::StructureBounds,
}

const NETHERRACK_PROBABILITY_BY_DISTANCE: [f32; 14] = [
    1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
];

impl Portal {
    fn block_of<W: FeatureWorld, R: RandomSource>(
        cx: &PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<(BlockState, BlockId), ProviderError> {
        let state = cx.world.block_state(pos)?;
        Ok((state, cx.world.blocks().block(state)))
    }

    /// `maybeAddVines`.
    fn maybe_add_vines<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        let (state, block) = Self::block_of(cx, pos)?;
        let b = cx.blocks().clone();
        if cx.world.state_data().is_air(state) || block == b.vine {
            return Ok(());
        }
        let direction = [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ][cx.random.next_int_bound(4)? as usize];
        let [dx, dy, dz] = direction.normal();
        let neighbour = pos.offset(dx, dy, dz);
        let neighbour_state = cx.world.block_state(neighbour)?;
        if !cx.world.state_data().is_air(neighbour_state) {
            return Ok(());
        }
        let full = cx
            .world
            .state_data()
            .collision_shape(state)
            .is_some_and(|shape| crate::block::survival::shape_fills_face(shape, direction));
        if full {
            let vine = cx.world.blocks().default_state(b.vine);
            let face = direction_name(direction.opposite());
            let vine = cx
                .world
                .blocks()
                .with_property(vine, face, "true")
                .unwrap_or(vine);
            cx.world.set_block(neighbour, vine, 3)?;
        }
        Ok(())
    }

    /// `maybeAddLeavesAbove`.
    fn maybe_add_leaves_above<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        if cx.random.next_float() < 0.5 {
            let b = cx.blocks().clone();
            let above = pos.offset(0, 1, 0);
            if Self::block_of(cx, pos)?.1 == b.netherrack
                && cx.world.state_data().is_air(cx.world.block_state(above)?)
            {
                let leaves = cx.world.blocks().default_state(b.jungle_leaves);
                let leaves = cx
                    .world
                    .blocks()
                    .with_property(leaves, "persistent", "true")
                    .unwrap_or(leaves);
                cx.world.set_block(above, leaves, 3)?;
            }
        }
        Ok(())
    }

    /// `addNetherrackDripColumnsBelowPortal`.
    fn add_netherrack_drip_columns_below_portal<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
    ) -> Result<(), ProviderError> {
        let (lo, hi) = (self.bounds.minimum(), self.bounds.maximum());
        let netherrack = cx.blocks().netherrack;
        for x in lo[0] + 1..hi[0] {
            for z in lo[2] + 1..hi[2] {
                let pos = BlockPos::new(x, lo[1], z);
                if Self::block_of(cx, pos)?.1 == netherrack {
                    self.add_netherrack_drip_column(cx, pos.offset(0, -1, 0))?;
                }
            }
        }
        Ok(())
    }

    /// `addNetherrackDripColumn`.
    fn add_netherrack_drip_column<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        let mut current = pos;
        self.place_netherrack_or_magma(cx, current)?;
        let mut remaining_cap = 8;
        while remaining_cap > 0 && cx.random.next_float() < 0.5 {
            current = current.offset(0, -1, 0);
            remaining_cap -= 1;
            self.place_netherrack_or_magma(cx, current)?;
        }
        Ok(())
    }

    /// `spreadNetherrack`.
    fn spread_netherrack<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
    ) -> Result<(), ProviderError> {
        let follow_ground_surface = matches!(
            self.placement,
            VerticalPlacement::OnLandSurface | VerticalPlacement::OnOceanFloor
        );
        let (lo, hi) = (self.bounds.minimum(), self.bounds.maximum());
        let center_x = lo[0] + (hi[0] - lo[0] + 1) / 2;
        let center_z = lo[2] + (hi[2] - lo[2] + 1) / 2;
        let max_distance = NETHERRACK_PROBABILITY_BY_DISTANCE.len() as i32;
        let average_width = ((hi[0] - lo[0] + 1) + (hi[2] - lo[2] + 1)) / 2;
        let distance_adjustment = cx.random.next_int_bound((8 - average_width / 2).max(1))?;
        let max_y_diff = 3;
        for x in center_x - max_distance..=center_x + max_distance {
            for z in center_z - max_distance..=center_z + max_distance {
                let distance = (x - center_x).abs() + (z - center_z).abs();
                let adjusted_distance = (distance + distance_adjustment).max(0);
                if adjusted_distance < max_distance {
                    let probability =
                        NETHERRACK_PROBABILITY_BY_DISTANCE[adjusted_distance as usize];
                    if cx.random.next_double() < f64::from(probability) {
                        let surface_y = cx
                            .height(height_map_type(self.placement), x, z)?
                            .wrapping_sub(1);
                        let y = if follow_ground_surface {
                            surface_y
                        } else {
                            lo[1].min(surface_y)
                        };
                        let pos = BlockPos::new(x, y, z);
                        if (y - lo[1]).abs() <= max_y_diff
                            && self.can_block_be_replaced_at(cx, pos)?
                        {
                            self.place_netherrack_or_magma(cx, pos)?;
                            if self.properties.overgrown {
                                self.maybe_add_leaves_above(cx, pos)?;
                            }
                            self.add_netherrack_drip_column(cx, pos.offset(0, -1, 0))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn can_block_be_replaced_at<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<bool, ProviderError> {
        let (_, block) = Self::block_of(cx, pos)?;
        let b = cx.blocks();
        let builtins = cx.tables.processors.pieces();
        Ok(block != b.air
            && block != b.obsidian
            && !builtins.cannot_replace.contains(block.0)
            && (self.placement == VerticalPlacement::InNether || block != b.lava))
    }

    /// `placeNetherrackOrMagma`.
    fn place_netherrack_or_magma<W: FeatureWorld, R: RandomSource>(
        &self,
        cx: &mut PieceCx<'_, '_, W, R>,
        pos: BlockPos,
    ) -> Result<(), ProviderError> {
        if self.can_block_be_replaced_at(cx, pos)? {
            let b = cx.blocks().clone();
            let block = if !self.properties.cold && cx.random.next_float() < 0.07 {
                b.magma_block
            } else {
                b.netherrack
            };
            let state = cx.world.blocks().default_state(block);
            cx.world.set_block(pos, state, 3)?;
        }
        Ok(())
    }
}
