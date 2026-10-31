//! End cities (`EndCityStructure`, `EndCityPieces`).
//!
//! Leaf's MC-152094 fix (`fixes.mc-152094`, off by default) is not ported: ships are placed as vanilla
//! does.

use super::piece_place::intersects;
use super::template::transform;
use super::template_pieces::{PieceCx, PieceEvent, PieceKind, TemplatePiece};
use super::template_start::{GenError, Stub, TemplateGen, TemplateGenWorld};
use crate::block::transform::{Mirror, Rotation};
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::{InvalidBound, RandomSource};
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::FeatureWorld;
use crate::worldgen::state_provider::ProviderError;

/// `MAX_GEN_DEPTH`.
const MAX_GEN_DEPTH: i32 = 8;

/// The four section generators (`EndCityPieces.SectionGenerator`).
#[derive(Clone, Copy)]
enum Section {
    HouseTower,
    Tower,
    TowerBridge,
    FatTower,
}

const TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(1, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(6, -1, 1)),
    (Rotation::Counterclockwise90, BlockPos::new(0, -1, 5)),
    (Rotation::Clockwise180, BlockPos::new(5, -1, 6)),
];

const FAT_TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(4, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(12, -1, 4)),
    (Rotation::Counterclockwise90, BlockPos::new(0, -1, 8)),
    (Rotation::Clockwise180, BlockPos::new(8, -1, 12)),
];

/// What the generators share: the registry (templates' sizes) and the random.
struct Gen<'a, R> {
    registry: &'a super::StructureRegistry,
    random: &'a mut R,
    /// `TOWER_BRIDGE_GENERATOR.shipCreated`.
    ship_created: bool,
}

/// `EndCityStructure.findGenerationPoint` and `generatePieces`.
pub fn generate<W: TemplateGenWorld, R: RandomSource>(
    cx: &mut TemplateGen<'_, W>,
    random: &mut R,
) -> Result<Option<Stub>, GenError> {
    let (min_x, min_z) = cx.min_block();
    let (x, z) = (min_x + 7, min_z + 7);
    if !cx.could_valid_biome_exist_in_terrain_column(x, z) {
        return Ok(None);
    }
    let rotation = Rotation::random(random);
    let start = cx.lowest_y_in_5_by_5_box(x, z, rotation);
    if start.y < 60 {
        return Ok(None);
    }
    let mut builder = Gen {
        registry: cx.registry,
        random,
        ship_created: false,
    };
    let mut pieces = Vec::new();
    builder.start_house_tower(start, rotation, &mut pieces)?;
    Ok(Some(Stub {
        position: start,
        pieces,
    }))
}

fn piece(
    registry: &super::StructureRegistry,
    name: &str,
    position: BlockPos,
    rotation: Rotation,
    overwrite: bool,
) -> TemplatePiece {
    TemplatePiece::new(
        registry,
        PieceKind::EndCity { overwrite },
        name.to_owned(),
        position,
        rotation,
        Mirror::None,
        BlockPos::default(),
    )
}

/// `addHelper`: the piece joins the list; the caller keeps a copy to build on.
fn add_helper(pieces: &mut Vec<TemplatePiece>, piece: TemplatePiece) -> TemplatePiece {
    pieces.push(piece.clone());
    piece
}

impl<R: RandomSource> Gen<'_, R> {
    /// `EndCityPieces.addPiece`: the child hangs off the parent's connection point.
    fn add_piece(
        &self,
        parent: &TemplatePiece,
        offset: BlockPos,
        name: &str,
        rotation: Rotation,
        overwrite: bool,
    ) -> TemplatePiece {
        let mut child = piece(self.registry, name, parent.position, rotation, overwrite);
        // `calculateConnectedPosition(parent settings, offset, child settings, ZERO)`.
        let a = transform(offset, parent.mirror, parent.rotation, parent.pivot);
        let b = transform(
            BlockPos::default(),
            child.mirror,
            child.rotation,
            child.pivot,
        );
        child.move_by(a.x - b.x, a.y - b.y, a.z - b.z);
        child
    }

    /// `addHelper(pieces, addPiece(...))`.
    fn chain(
        &self,
        pieces: &mut Vec<TemplatePiece>,
        last: &TemplatePiece,
        offset: BlockPos,
        name: &str,
        rotation: Rotation,
        overwrite: bool,
    ) -> TemplatePiece {
        add_helper(
            pieces,
            self.add_piece(last, offset, name, rotation, overwrite),
        )
    }

    /// `EndCityPieces.startHouseTower`.
    fn start_house_tower(
        &mut self,
        origin: BlockPos,
        rotation: Rotation,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<(), InvalidBound> {
        let mut last = add_helper(
            pieces,
            piece(self.registry, "base_floor", origin, rotation, true),
        );
        last = self.chain(
            pieces,
            &last,
            BlockPos::new(-1, 0, -1),
            "second_floor_1",
            rotation,
            false,
        );
        last = self.chain(
            pieces,
            &last,
            BlockPos::new(-1, 4, -1),
            "third_floor_1",
            rotation,
            false,
        );
        last = self.chain(
            pieces,
            &last,
            BlockPos::new(-1, 8, -1),
            "third_roof",
            rotation,
            true,
        );
        self.recursive_children(Section::Tower, 1, &last, None, pieces)?;
        Ok(())
    }

    /// `EndCityPieces.recursiveChildren`.
    fn recursive_children(
        &mut self,
        section: Section,
        gen_depth: i32,
        parent: &TemplatePiece,
        offset: Option<BlockPos>,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        if gen_depth > MAX_GEN_DEPTH {
            return Ok(false);
        }
        let mut children = Vec::new();
        if self.generate_section(section, gen_depth, parent, offset, &mut children)? {
            let mut collision = false;
            let tag = self.random.next_int();
            for child in &mut children {
                child.gen_depth = tag;
                let found = pieces
                    .iter()
                    .find(|p| intersects(p.bounding_box, child.bounding_box));
                if found.is_some_and(|p| p.gen_depth != parent.gen_depth) {
                    collision = true;
                    break;
                }
            }
            if !collision {
                pieces.extend(children);
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn generate_section(
        &mut self,
        section: Section,
        gen_depth: i32,
        parent: &TemplatePiece,
        offset: Option<BlockPos>,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        match section {
            Section::HouseTower => self.house_tower(gen_depth, parent, offset, pieces),
            Section::Tower => self.tower(gen_depth, parent, pieces),
            Section::TowerBridge => self.tower_bridge(gen_depth, parent, pieces),
            Section::FatTower => self.fat_tower(gen_depth, parent, pieces),
        }
    }

    /// `HOUSE_TOWER_GENERATOR`.
    fn house_tower(
        &mut self,
        gen_depth: i32,
        parent: &TemplatePiece,
        offset: Option<BlockPos>,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        if gen_depth > 8 {
            return Ok(false);
        }
        let rotation = parent.rotation;
        let offset = offset.unwrap_or_default();
        let added = self.add_piece(parent, offset, "base_floor", rotation, true);
        let mut last = add_helper(pieces, added);
        let floors = self.random.next_int_bound(3)?;
        if floors == 0 {
            self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 4, -1),
                "base_roof",
                rotation,
                true,
            );
        } else if floors == 1 {
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 0, -1),
                "second_floor_2",
                rotation,
                false,
            );
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 8, -1),
                "second_roof",
                rotation,
                false,
            );
            self.recursive_children(Section::Tower, gen_depth + 1, &last, None, pieces)?;
        } else if floors == 2 {
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 0, -1),
                "second_floor_2",
                rotation,
                false,
            );
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 4, -1),
                "third_floor_2",
                rotation,
                false,
            );
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 8, -1),
                "third_roof",
                rotation,
                true,
            );
            self.recursive_children(Section::Tower, gen_depth + 1, &last, None, pieces)?;
        }
        Ok(true)
    }

    /// `TOWER_GENERATOR`.
    fn tower(
        &mut self,
        gen_depth: i32,
        parent: &TemplatePiece,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        let rotation = parent.rotation;
        let mut last = parent.clone();
        let dx = 3 + self.random.next_int_bound(2)?;
        let dz = 3 + self.random.next_int_bound(2)?;
        last = self.chain(
            pieces,
            &last,
            BlockPos::new(dx, -3, dz),
            "tower_base",
            rotation,
            true,
        );
        last = self.chain(
            pieces,
            &last,
            BlockPos::new(0, 7, 0),
            "tower_piece",
            rotation,
            true,
        );
        let mut bridge_piece = if self.random.next_int_bound(3)? == 0 {
            Some(last.clone())
        } else {
            None
        };
        let tower_height = 1 + self.random.next_int_bound(3)?;
        for i in 0..tower_height {
            last = self.chain(
                pieces,
                &last,
                BlockPos::new(0, 4, 0),
                "tower_piece",
                rotation,
                true,
            );
            if i < tower_height - 1 && self.random.next_boolean() {
                bridge_piece = Some(last.clone());
            }
        }
        if let Some(bridge_piece) = bridge_piece {
            for (bridge_rotation, bridge_offset) in TOWER_BRIDGES {
                if self.random.next_boolean() {
                    let added = self.add_piece(
                        &bridge_piece,
                        bridge_offset,
                        "bridge_end",
                        rotation.then(bridge_rotation),
                        true,
                    );
                    let bridge_start = add_helper(pieces, added);
                    self.recursive_children(
                        Section::TowerBridge,
                        gen_depth + 1,
                        &bridge_start,
                        None,
                        pieces,
                    )?;
                }
            }
            self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 4, -1),
                "tower_top",
                rotation,
                true,
            );
        } else {
            if gen_depth != 7 {
                return self.recursive_children(
                    Section::FatTower,
                    gen_depth + 1,
                    &last,
                    None,
                    pieces,
                );
            }
            self.chain(
                pieces,
                &last,
                BlockPos::new(-1, 4, -1),
                "tower_top",
                rotation,
                true,
            );
        }
        Ok(true)
    }

    /// `TOWER_BRIDGE_GENERATOR`.
    fn tower_bridge(
        &mut self,
        gen_depth: i32,
        parent: &TemplatePiece,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        let rotation = parent.rotation;
        let bridge_length = self.random.next_int_bound(4)? + 1;
        let added = self.add_piece(
            parent,
            BlockPos::new(0, 0, -4),
            "bridge_piece",
            rotation,
            true,
        );
        let mut last = add_helper(pieces, added);
        last.gen_depth = -1;
        if let Some(stored) = pieces.last_mut() {
            stored.gen_depth = -1;
        }
        let mut next_y = 0;
        for _ in 0..bridge_length {
            if self.random.next_boolean() {
                let added = self.add_piece(
                    &last,
                    BlockPos::new(0, next_y, -4),
                    "bridge_piece",
                    rotation,
                    true,
                );
                last = add_helper(pieces, added);
                next_y = 0;
            } else {
                let added = if self.random.next_boolean() {
                    self.add_piece(
                        &last,
                        BlockPos::new(0, next_y, -4),
                        "bridge_steep_stairs",
                        rotation,
                        true,
                    )
                } else {
                    self.add_piece(
                        &last,
                        BlockPos::new(0, next_y, -8),
                        "bridge_gentle_stairs",
                        rotation,
                        true,
                    )
                };
                last = add_helper(pieces, added);
                next_y = 4;
            }
        }
        if !self.ship_created && self.random.next_int_bound(10 - gen_depth)? == 0 {
            let x = -8 + self.random.next_int_bound(8)?;
            let z = -70 + self.random.next_int_bound(10)?;
            let added = self.add_piece(&last, BlockPos::new(x, next_y, z), "ship", rotation, true);
            add_helper(pieces, added);
            self.ship_created = true;
        } else if !self.recursive_children(
            Section::HouseTower,
            gen_depth + 1,
            &last,
            Some(BlockPos::new(-3, next_y + 1, -11)),
            pieces,
        )? {
            return Ok(false);
        }
        let added = self.add_piece(
            &last,
            BlockPos::new(4, next_y, 0),
            "bridge_end",
            rotation.then(Rotation::Clockwise180),
            true,
        );
        add_helper(pieces, added);
        if let Some(stored) = pieces.last_mut() {
            stored.gen_depth = -1;
        }
        Ok(true)
    }

    /// `FAT_TOWER_GENERATOR`.
    fn fat_tower(
        &mut self,
        gen_depth: i32,
        parent: &TemplatePiece,
        pieces: &mut Vec<TemplatePiece>,
    ) -> Result<bool, InvalidBound> {
        let rotation = parent.rotation;
        let added = self.add_piece(
            parent,
            BlockPos::new(-3, 4, -3),
            "fat_tower_base",
            rotation,
            true,
        );
        let mut last = add_helper(pieces, added);
        let added = self.add_piece(
            &last,
            BlockPos::new(0, 4, 0),
            "fat_tower_middle",
            rotation,
            true,
        );
        last = add_helper(pieces, added);
        let mut i = 0;
        while i < 2 && self.random.next_int_bound(3)? != 0 {
            let added = self.add_piece(
                &last,
                BlockPos::new(0, 8, 0),
                "fat_tower_middle",
                rotation,
                true,
            );
            last = add_helper(pieces, added);
            for (bridge_rotation, bridge_offset) in FAT_TOWER_BRIDGES {
                if self.random.next_boolean() {
                    let added = self.add_piece(
                        &last,
                        bridge_offset,
                        "bridge_end",
                        rotation.then(bridge_rotation),
                        true,
                    );
                    let bridge_start = add_helper(pieces, added);
                    self.recursive_children(
                        Section::TowerBridge,
                        gen_depth + 1,
                        &bridge_start,
                        None,
                        pieces,
                    )?;
                }
            }
            i += 1;
        }
        let added = self.add_piece(
            &last,
            BlockPos::new(-2, 8, -2),
            "fat_tower_top",
            rotation,
            true,
        );
        add_helper(pieces, added);
        Ok(true)
    }
}

/// `EndCityPiece.handleDataMarker`.
pub fn data_marker<W: FeatureWorld, R: RandomSource>(
    piece: &TemplatePiece,
    marker: &str,
    pos: BlockPos,
    cx: &mut PieceCx<'_, '_, W, R>,
) -> Result<(), ProviderError> {
    let inside = |b: StructureBounds, p: BlockPos| b.contains([p.x, p.y, p.z]);
    if marker.starts_with("Chest") {
        let chest = pos.offset(0, -1, 0);
        if inside(*cx.chunk_box, chest) {
            cx.set_loot_table(chest, "minecraft:chests/end_city_treasure")?;
        }
    } else if inside(*cx.chunk_box, pos) && in_spawnable_bounds(pos) {
        if marker.starts_with("Sentry") {
            cx.out.events.push(PieceEvent::Spawn {
                entity: "minecraft:shulker",
                pos,
                group: false,
            });
        } else if marker.starts_with("Elytra") {
            let _facing: Direction = piece.rotation.rotate(Direction::South);
            cx.out.events.push(PieceEvent::Spawn {
                entity: "minecraft:item_frame",
                pos,
                group: false,
            });
        }
    }
    Ok(())
}

/// `Level.isInSpawnableBounds`.
fn in_spawnable_bounds(pos: BlockPos) -> bool {
    const LIMIT: i32 = 30_000_000;
    const HEIGHT: i32 = 20_000_000;
    !(pos.y < -HEIGHT || pos.y >= HEIGHT)
        && (-LIMIT..LIMIT).contains(&pos.x)
        && (-LIMIT..LIMIT).contains(&pos.z)
}
