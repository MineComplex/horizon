//! `block_states.bin`: a synthetic file written here, plus a check of the real 26.3 file when
//! `HORIZON_SERVER_BLOCK_STATES` names one (made by `tools/leaf_server_reports.py`) and
//! `HORIZON_SERVER_BLOCKS_JSON` its `blocks.json`; that test passes without checking otherwise.

use super::flags::*;
use super::*;
use crate::core::geometry::{Axis, Direction, ShapeOp, VoxelShape};

fn names(out: &mut Vec<u8>, names: &[&str]) {
    out.extend((names.len() as u16).to_le_bytes());
    for name in names {
        out.push(name.len() as u8);
        out.extend(name.as_bytes());
    }
}

fn record(
    out: &mut Vec<u8>,
    flags: u32,
    sturdy: u32,
    bytes: [u8; 6],
    speed: f32,
    shapes: [u32; 2],
) {
    out.extend(flags.to_le_bytes());
    out.extend(sturdy.to_le_bytes());
    out.extend(bytes);
    out.extend([0, 0]);
    out.extend(speed.to_le_bytes());
    out.extend(shapes[0].to_le_bytes());
    out.extend(shapes[1].to_le_bytes());
}

/// Three states: air, a full block, a bottom slab (a 1x2x1 cube grid) and a dynamic-shape plant
/// with a non-cube grid on Y.
fn synthetic(count: u32) -> Vec<u8> {
    let mut out = b"HZBS".to_vec();
    out.extend(VERSION.to_le_bytes());
    out.extend(count.to_le_bytes());
    names(
        &mut out,
        &[
            "minecraft:empty",
            "minecraft:flowing_water",
            "minecraft:water",
        ],
    );
    names(
        &mut out,
        &["PUSH_PULL", "PUSH", "POPPED", "IMMOVEABLE", "IGNORE_ENTITY"],
    );
    names(&mut out, &["harp", "basedrum"]);
    out.extend(4u32.to_le_bytes());
    out.push(0); // empty
    out.push(1); // block
    // A bottom slab: cube grid 1x2x1, lower cell full.
    out.extend([2, 1, 1, 1, 0, 1, 2, 0, 1, 1, 0, 0b01]);
    // A grid with explicit Y coordinates 0, 0.25 (one cell), X and Z cube axes of 1.
    out.extend([2, 0, 1, 1, 0, 0, 2, 0]);
    out.extend(0f64.to_le_bytes());
    out.extend(0.25f64.to_le_bytes());
    out.extend([1, 1, 0, 0b1]);
    record(
        &mut out,
        FLAG_AIR | FLAG_REPLACEABLE | FLAG_PROPAGATES_SKYLIGHT_DOWN,
        0,
        [0; 6],
        0.0,
        [0, 0],
    );
    let stone = FLAG_SOLID_RENDER | FLAG_CAN_OCCLUDE | FLAG_COLLISION_FULL_BLOCK;
    record(
        &mut out,
        stone,
        (1 << 18) - 1,
        [0, 15, 0, 11, 1, 0],
        1.5,
        [1, 1],
    );
    let slab = FLAG_CAN_OCCLUDE | FLAG_USE_SHAPE_FOR_LIGHT_OCCLUSION;
    record(
        &mut out,
        slab,
        1 << 3,
        [3, 0, 3, 11, 0, 2 | 8 << 3],
        2.0,
        [2, 2],
    );
    record(
        &mut out,
        FLAG_DYNAMIC_SHAPE,
        0,
        [14, 0, 2, 0, 0, 1 | 3 << 3 | 0x80],
        0.0,
        [u32::MAX, 3],
    );
    out
}

#[test]
fn synthetic_round_trip() {
    let data = BlockStateData::from_bytes(&synthetic(4), 4).unwrap();
    let (air, stone, slab, plant) = (BlockState(0), BlockState(1), BlockState(2), BlockState(3));
    assert_eq!(data.shape_count(), 4);
    assert!(data.is_air(air) && data.is_replaceable(air) && data.propagates_skylight_down(air));
    assert!(data.collision_shape(air).unwrap().is_empty());
    assert!(data.collision_shape(stone).unwrap().is_block());
    assert!(data.is_solid_render(stone) && !data.is_air(stone));
    assert_eq!(data.light_dampening(stone), 15);
    assert_eq!(data.destroy_speed(stone), 1.5);
    assert_eq!(data.instrument(stone), "basedrum");
    assert_eq!(data.map_color(stone), 11);
    assert!(data.is_face_sturdy(stone, Direction::East, SupportType::Rigid));
    assert!(data.is_face_sturdy(slab, Direction::Up, SupportType::Full));
    assert!(!data.is_face_sturdy(slab, Direction::Down, SupportType::Full));
    let half = data.collision_shape(slab).unwrap();
    assert!(half.is_cube());
    assert_eq!((half.min(Axis::Y), half.max(Axis::Y)), (0.0, 0.5));
    assert!(data.is_conditionally_full_opaque(slab));
    assert_eq!(data.push_reaction(slab), PushReaction::Immoveable);
    assert_eq!(data.light_emission(slab), 3);
    let water = data.fluid(slab);
    assert_eq!(
        (data.fluid_name(water.fluid), water.amount, water.falling),
        ("minecraft:water", 8, false)
    );
    assert!(data.collision_shape(plant).is_none());
    assert_eq!(data.occlusion_shape(plant).max(Axis::Y), 0.25);
    let flowing = data.fluid(plant);
    assert_eq!(
        (flowing.fluid, flowing.amount, flowing.falling),
        (1, 3, true)
    );
    assert_eq!(data.push_reaction(plant), PushReaction::Popped);
    // Equal shapes are one shape: identities are shared through the index.
    assert!(data.occlusion_shape(slab).same_identity(half));
}

#[test]
fn synthetic_errors() {
    let file = synthetic(4);
    assert_eq!(
        BlockStateData::from_bytes(&file, 5).err(),
        Some(BlockDataError::StateDataCount { file: 4, report: 5 })
    );
    let err = |bytes: &[u8]| {
        matches!(
            BlockStateData::from_bytes(bytes, 4),
            Err(BlockDataError::StateData(_))
        )
    };
    assert!(err(&file[..file.len() - 1]));
    assert!(err(&[file.as_slice(), &[0]].concat()));
    let mut version = file.clone();
    version[4] = 9;
    assert!(err(&version));
    let mut bad_shape = file.clone();
    let last = bad_shape.len() - 4;
    bad_shape[last] = 7; // the plant's occlusion shape index
    assert!(err(&bad_shape));
    assert!(err(b"JUNK"));
}

/// `HORIZON_SERVER_BLOCK_STATES=<server>/cache/vanilla-26.3/block_states.bin
/// HORIZON_SERVER_BLOCKS_JSON=<server>/cache/vanilla-26.3/reports/blocks.json cargo test -p
/// horizon-server block::flags_tests::real_26_3_state_data -- --nocapture`
#[test]
fn real_26_3_state_data() {
    let (Some(bin), Some(json)) = (
        std::env::var_os("HORIZON_SERVER_BLOCK_STATES"),
        std::env::var_os("HORIZON_SERVER_BLOCKS_JSON"),
    ) else {
        eprintln!(
            "HORIZON_SERVER_BLOCK_STATES/_BLOCKS_JSON are not set: block_states.bin is not checked"
        );
        return;
    };
    let t = BlockStates::load(json.as_ref()).unwrap();
    let d = BlockStateData::load(bin.as_ref(), &t).unwrap();
    let state = |name: &str, values: &[(&str, &str)]| {
        let block = t.block_by_name(name).unwrap();
        t.state_with(block, values.iter().copied()).unwrap()
    };
    let plain = |name: &str| state(name, &[]);
    let y = |shape: &VoxelShape| (shape.min(Axis::Y), shape.max(Axis::Y));

    let air = plain("minecraft:air");
    assert!(d.is_air(air) && d.is_replaceable(air) && d.propagates_skylight_down(air));
    assert!(d.collision_shape(air).unwrap().is_empty() && d.occlusion_shape(air).is_empty());
    let stone = plain("minecraft:stone");
    assert!(d.collision_shape(stone).unwrap().is_block() && d.occlusion_shape(stone).is_block());
    assert!(
        d.is_solid_render(stone)
            && d.has(stone, FLAG_COLLISION_FULL_BLOCK | FLAG_OCCLUDES_FULL_BLOCK)
    );
    assert!(d.requires_correct_tool_for_drops(stone) && !d.propagates_skylight_down(stone));
    assert_eq!(
        (d.light_dampening(stone), d.destroy_speed(stone)),
        (15, 1.5)
    );
    assert!(d.is_face_sturdy(stone, Direction::Up, SupportType::Full));
    assert_eq!(d.destroy_speed(plain("minecraft:bedrock")), -1.0);
    assert_eq!(
        d.push_reaction(plain("minecraft:obsidian")),
        PushReaction::Immoveable
    );
    assert_eq!(d.instrument(stone), "basedrum");

    let torch = plain("minecraft:torch");
    assert_eq!(d.light_emission(torch), 14);
    assert!(
        d.collision_shape(torch).unwrap().is_empty()
            && d.push_reaction(torch) == PushReaction::Popped
    );
    assert_eq!(d.light_emission(plain("minecraft:glowstone")), 15);
    let lit = |lit| state("minecraft:redstone_lamp", &[("lit", lit)]);
    assert_eq!(
        (
            d.light_emission(lit("true")),
            d.light_emission(lit("false"))
        ),
        (15, 0)
    );

    let slab = |kind| state("minecraft:oak_slab", &[("type", kind)]);
    assert_eq!(y(d.collision_shape(slab("bottom")).unwrap()), (0.0, 0.5));
    assert_eq!(y(d.collision_shape(slab("top")).unwrap()), (0.5, 1.0));
    assert!(
        d.has(slab("double"), FLAG_COLLISION_FULL_BLOCK)
            && !d.has(slab("bottom"), FLAG_COLLISION_FULL_BLOCK)
    );
    assert!(d.is_face_sturdy(slab("bottom"), Direction::Down, SupportType::Full));
    assert!(!d.is_face_sturdy(slab("bottom"), Direction::Up, SupportType::Full));

    let stairs = state(
        "minecraft:oak_stairs",
        &[
            ("facing", "east"),
            ("half", "bottom"),
            ("shape", "straight"),
        ],
    );
    let shape = d.collision_shape(stairs).unwrap();
    assert_eq!(shape.boxes().len(), 2);
    assert_eq!(y(shape), (0.0, 1.0));
    assert!(
        shape.is_full(1, 1, 0) && !shape.is_full(0, 1, 0),
        "east stairs: the upper half is east"
    );
    assert!(d.is_face_sturdy(stairs, Direction::East, SupportType::Full));
    assert!(!d.is_face_sturdy(stairs, Direction::West, SupportType::Full));

    let water = plain("minecraft:water");
    let fluid = d.fluid(water);
    assert!(
        d.has(water, FLAG_LIQUID | FLAG_REPLACEABLE)
            && d.collision_shape(water).unwrap().is_empty()
    );
    assert_eq!(
        (d.fluid_name(fluid.fluid), fluid.amount, fluid.falling),
        ("minecraft:water", 8, false)
    );
    assert!(
        d.has(water, FLAG_CONSTANT_COLLISION)
            && !d.has(plain("minecraft:lava"), FLAG_CONSTANT_COLLISION)
    );
    // `LiquidBlock.getFluidState`: level 1..=7 flows with amount 8 - level, 8.. falls with 8.
    let flowing = |level| {
        let f = d.fluid(state("minecraft:water", &[("level", level)]));
        (d.fluid_name(f.fluid).to_owned(), f.amount, f.falling)
    };
    assert_eq!(flowing("3"), ("minecraft:flowing_water".into(), 5, false));
    assert_eq!(flowing("9"), ("minecraft:flowing_water".into(), 8, true));
    assert_eq!(d.light_emission(plain("minecraft:lava")), 15);
    let logged = d.fluid(state("minecraft:oak_stairs", &[("waterlogged", "true")]));
    assert_eq!(d.fluid_name(logged.fluid), "minecraft:water");

    let fence = plain("minecraft:oak_fence");
    assert!(d.has(fence, FLAG_LARGE_COLLISION_SHAPE | FLAG_SPECIAL_COLLIDING));
    assert_eq!(d.collision_shape(fence).unwrap().max(Axis::Y), 1.5);
    assert!(d.has(
        plain("minecraft:moving_piston"),
        FLAG_DYNAMIC_SHAPE | FLAG_SPECIAL_COLLIDING
    ));
    assert!(d.has(plain("minecraft:poppy"), FLAG_HAS_OFFSET));
    assert!(d.is_randomly_ticking(plain("minecraft:farmland")) && !d.is_randomly_ticking(stone));
    assert!(d.is_randomly_ticking(plain("minecraft:grass_block")));
    assert!(d.has(plain("minecraft:short_grass"), FLAG_REPLACEABLE));
    assert!(!d.has(plain("minecraft:scaffolding"), FLAG_CONSTANT_COLLISION));
    assert!(d.has(plain("minecraft:redstone_block"), FLAG_SIGNAL_SOURCE));

    // Every state: the shapes rebuilt here agree with the flags vanilla computed from its own.
    let mut cached = 0;
    for id in 0..t.state_count() {
        let s = BlockState(id);
        assert_eq!(d.is_air(s), t.is_air(s), "{id}");
        let full = |shape: &VoxelShape| {
            !VoxelShape::block()
                .join_is_not_empty(shape, ShapeOp::XOR)
                .unwrap()
        };
        assert_eq!(full(d.occlusion_shape(s)), d.is_solid_render(s), "{id}");
        let Some(c) = d.collision_shape(s) else {
            continue;
        };
        cached += 1;
        assert_eq!(full(c), d.has(s, FLAG_COLLISION_FULL_BLOCK), "{id}");
        let covers = !c.is_empty()
            && !VoxelShape::block()
                .join_is_not_empty(c, ShapeOp::ONLY_FIRST)
                .unwrap();
        assert_eq!(covers, d.has(s, FLAG_OCCLUDES_FULL_BLOCK), "{id}");
        let large = Axis::ALL
            .into_iter()
            .any(|a| c.min(a) < 0.0 || c.max(a) > 1.0);
        assert_eq!(large, d.has(s, FLAG_LARGE_COLLISION_SHAPE), "{id}");
    }
    eprintln!(
        "{} states ({cached} with a cached collision shape), {} shapes checked",
        t.state_count(),
        d.shape_count()
    );
}
