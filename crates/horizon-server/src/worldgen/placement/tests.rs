//! Placement without the 26.3 jar: decoding, the chain order and the feature sorter on small
//! inputs. The real data is checked by `oracle_tests.rs`.

use serde_json::value::RawValue;

use super::PlacementModifier;
use super::world::{PlacementWorld, WorldError};
use super::{BlockPredicate, DecodeContext, FeaturePlacer, HeightProvider, IntProvider};
use crate::block::{BlockState, BlockStateData, BlockStates, BlockStatesBuilder};
use crate::core::position::BlockPos;
use crate::registry::tests::pack;
use crate::util::random::{InvalidBound, RandomSource, XoroshiroRandom};
use crate::world::chunk::serial::Heightmap;
use crate::worldgen::anchor::WorldGenerationContext;
use crate::worldgen::feature::sort::{FeatureCycle, build_features_per_step};
use crate::worldgen::feature::{FeatureRegistry, PlacedFeatureId};

fn raw(json: &str) -> Box<RawValue> {
    RawValue::from_string(json.to_owned()).unwrap()
}

fn blocks() -> BlockStates {
    let mut builder = BlockStatesBuilder::new();
    for name in ["air", "stone", "water", "lava", "bedrock"] {
        builder
            .add_block(&format!("minecraft:{name}"), &[])
            .unwrap();
    }
    builder.finish()
}

fn names(blocks: &BlockStates) -> DecodeContext<'_> {
    DecodeContext {
        blocks,
        block_tags: &[],
        biomes: &[],
        biome_tags: &[],
        fluid_tags: &[],
    }
}

#[test]
fn providers_sample_as_vanilla() {
    let mut random = XoroshiroRandom::new(7);
    let mut twin = XoroshiroRandom::new(7);
    let uniform = IntProvider::decode(&raw(
        r#"{"type":"minecraft:uniform","min_inclusive":-4,"max_inclusive":9}"#,
    ))
    .unwrap();
    for _ in 0..50 {
        assert_eq!(
            uniform.sample(&mut random).unwrap(),
            twin.next_int_between_inclusive(-4, 9).unwrap()
        );
    }
    assert_eq!(IntProvider::decode(&raw("5")).unwrap().bounds(), [5, 5]);
    assert!(
        IntProvider::decode(&raw(
            r#"{"type":"minecraft:uniform","min_inclusive":5,"max_inclusive":1}"#
        ))
        .is_err()
    );
    assert!(IntProvider::decode_within(&raw("5000"), 0, 4096).is_err());
    let height = HeightProvider::decode(&raw(r#"{"above_bottom":8}"#)).unwrap();
    let context = WorldGenerationContext::new(-64, 384, 63);
    assert_eq!(height.sample(&mut random, context).unwrap(), -56);
}

#[test]
fn modifiers_refuse_what_vanilla_refuses() {
    let blocks = blocks();
    let names = names(&blocks);
    for bad in [
        r#"{"type":"minecraft:count","count":5000}"#,
        r#"{"type":"minecraft:rarity_filter","chance":0}"#,
        r#"{"type":"minecraft:offset","x":17,"y":0,"z":0}"#,
        r#"{"type":"minecraft:random_chance","chance":1.5}"#,
        r#"{"type":"minecraft:randomly_selected","placements":[]}"#,
        r#"{"type":"minecraft:heightmap","heightmap":"NO_SUCH"}"#,
        r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:solid","offset":[0,17,0]}}"#,
        r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:volume_match","min":[1,0,0],"max":[0,0,0],"match":{"type":"minecraft:true"}}}"#,
        r#"{"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:matching_blocks","blocks":"minecraft:no_such_block"}}"#,
    ] {
        assert!(
            PlacementModifier::decode(&raw(bad), &names).is_err(),
            "{bad}"
        );
    }
    let state = names.block_state(&raw(r#""minecraft:stone""#)).unwrap();
    assert_eq!(
        state,
        blocks.default_state(blocks.block_by_name("minecraft:stone").unwrap())
    );
    assert!(
        BlockPredicate::decode(
            &raw(r#"{"type":"minecraft:would_survive","state":"minecraft:stone"}"#),
            &names
        )
        .is_ok()
    );
}

/// A world of flat ground: only what `in_square` and `count` ask of it.
struct Flat {
    blocks: BlockStates,
    data: BlockStateData,
}

impl PlacementWorld for Flat {
    fn blocks(&self) -> &BlockStates {
        &self.blocks
    }
    fn state_data(&self) -> &BlockStateData {
        &self.data
    }
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn level_generation(&self) -> WorldGenerationContext {
        self.generation()
    }
    fn height(&self, _: Heightmap, _: i32, _: i32) -> Result<i32, WorldError> {
        Ok(64)
    }
    fn block_state(&self, _: BlockPos) -> Result<BlockState, WorldError> {
        Ok(BlockState(0))
    }
    fn biome(&self, _: BlockPos) -> Result<u32, WorldError> {
        Ok(0)
    }
    fn block_survives(&self, _: BlockState, _: BlockPos) -> Result<bool, WorldError> {
        Ok(true)
    }
}

#[test]
fn the_chain_runs_depth_first_in_emission_order() {
    let blocks = blocks();
    let registry = FeatureRegistry::load(&[pack("vanilla", None, &[])], &blocks).unwrap();
    let chain: Vec<_> = [
        r#"{"type":"minecraft:count","count":3}"#,
        r#"{"type":"minecraft:in_square"}"#,
        r#"{"type":"minecraft:heightmap","heightmap":"WORLD_SURFACE_WG"}"#,
    ]
    .iter()
    .map(|m| PlacementModifier::decode(&raw(m), &names(&blocks)).unwrap())
    .collect();
    let mut world = Flat {
        blocks: self::blocks(),
        data: BlockStateData::default(),
    };
    let mut random = XoroshiroRandom::new(42);
    let mut twin = XoroshiroRandom::new(42);
    let mut placed = Vec::new();
    let any = FeaturePlacer::default()
        .place_chain(
            &registry,
            &chain,
            None,
            &mut world,
            &mut random,
            BlockPos::new(32, -64, 48),
            |_, r, p| {
                placed.push(p);
                Ok::<_, InvalidBound>(r.next_boolean())
            },
        )
        .unwrap();
    // Each of the three origins is spread and put on the surface before the next one is.
    let mut expected = Vec::new();
    let mut expected_any = false;
    for _ in 0..3 {
        let (x, z) = (
            32 + twin.next_int_bound(16).unwrap(),
            48 + twin.next_int_bound(16).unwrap(),
        );
        expected.push(BlockPos::new(x, 64, z));
        expected_any |= twin.next_boolean();
    }
    assert_eq!(placed, expected);
    assert_eq!(any, expected_any);
}

#[test]
fn sorter_merges_orders_and_finds_cycles() {
    let f = |i| PlacedFeatureId(i);
    // Two biomes agree on 1 before 2 and add 0 and 3 on either side.
    let steps = build_features_per_step(&[
        vec![vec![f(0), f(1)], vec![f(2)]],
        vec![vec![f(1)], vec![f(2), f(3)]],
    ])
    .unwrap();
    assert_eq!(steps[0].features(), [f(0), f(1)]);
    assert_eq!(steps[1].features(), [f(2), f(3)]);
    assert_eq!(steps[1].index_of(f(3)), Some(1));
    // Opposite orders of one step are a cycle, and so is a feature listed twice in a biome.
    assert_eq!(
        build_features_per_step(&[vec![vec![f(0), f(1)]], vec![vec![f(1), f(0)]]]).unwrap_err(),
        FeatureCycle {
            involved: vec![0, 1]
        }
    );
    assert!(build_features_per_step(&[vec![vec![f(0), f(1), f(0)]]]).is_err());
    // `tryReducingError` drops the sources that are not part of the cycle.
    let cycle = build_features_per_step(&[
        vec![vec![f(5)]],
        vec![vec![f(0), f(1)]],
        vec![vec![f(7), f(8)]],
        vec![vec![f(1), f(0)]],
    ])
    .unwrap_err();
    assert_eq!(cycle.involved, [1, 3]);
    assert_eq!(
        cycle.message(|i| format!("s{i}")),
        "Feature order cycle found, involved sources: [s1, s3]"
    );
}
