//! `minecraft:fallen_tree`: a stump and an optional fallen log with decorators.
use super::blocks::{self as tb, Axis, PosSet};
use super::{
    NestedPlacer, Run, Sets, TreeBlocksCell, TreeDecoder, TreeResult, decorator::Lists,
    decorator::TreeDecorator, kind, read, sample_state, write,
};
use crate::block::flags::SupportType;
use crate::core::geometry::Direction;
use crate::core::position::BlockPos;
use crate::util::random::RandomSource;
use crate::worldgen::codec::{DecodeError, Object, required};
use crate::worldgen::feature::ConfiguredFeatureId;
use crate::worldgen::feature::simple::FeatureWorld;
use crate::worldgen::placement::provider::IntProvider;
use crate::worldgen::state_provider::{BlockStateProvider, ProviderContext};
use serde_json::value::RawValue;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct FallenTreeFeature {
    trunk_provider: BlockStateProvider,
    log_length: IntProvider,
    stump_decorators: Vec<TreeDecorator>,
    log_decorators: Vec<TreeDecorator>,
    cell: Arc<TreeBlocksCell>,
}
impl FallenTreeFeature {
    pub(crate) fn decode(
        raw: &RawValue,
        ctx: &ProviderContext<'_>,
        cell: Arc<TreeBlocksCell>,
    ) -> Result<Self, DecodeError> {
        let object: Object = crate::worldgen::codec::object(raw)?;
        if kind(&object)? != "minecraft:fallen_tree" {
            return Err(DecodeError::Field("fallen tree feature type"));
        }
        let mut decoder = TreeDecoder { ctx };
        Ok(Self {
            trunk_provider: decoder.provider(required(&object, "trunk_provider")?)?,
            log_length: decoder.int_provider(required(&object, "log_length")?, 0, 16)?,
            stump_decorators: TreeDecorator::decode_list(
                required(&object, "stump_decorators")?,
                &mut decoder,
            )?,
            log_decorators: TreeDecorator::decode_list(
                required(&object, "log_decorators")?,
                &mut decoder,
            )?,
            cell,
        })
    }
    /// Binds the `pale_moss` decorators to the `PALE_MOSS_PATCH` feature.
    pub(crate) fn set_pale_moss_patch(&mut self, patch: ConfiguredFeatureId) {
        for decorator in self
            .stump_decorators
            .iter_mut()
            .chain(&mut self.log_decorators)
        {
            decorator.set_patch(patch);
        }
    }
    /// Whether a decorator places the pale moss patch (a feature of its own).
    pub fn places_pale_moss(&self) -> bool {
        (self.stump_decorators.iter())
            .chain(&self.log_decorators)
            .any(TreeDecorator::is_pale_moss)
    }
    pub(crate) fn place<W: FeatureWorld, R: RandomSource>(
        &self,
        world: &mut W,
        random: &mut R,
        origin: [i32; 3],
        nested: &mut NestedPlacer<'_, W, R>,
    ) -> TreeResult<bool> {
        let blocks = self.cell.get(world.blocks(), world.state_data());
        let mut run = Run {
            world,
            random,
            blocks,
            sets: Sets::default(),
            track: false,
        };
        let stump = self.place_log(&mut run, origin, None)?;
        self.decorate(&mut run, &[stump], &self.stump_decorators, nested)?;
        let direction = tb::HORIZONTAL[run.random.next_int_bound(4)? as usize];
        let length = self.log_length.sample(run.random)? - 2;
        let mut start = tb::relative(origin, direction, 2 + run.random.next_int_bound(2)?);
        start = tb::offset(start, 0, 1, 0);
        for _ in 0..6 {
            if self.may_place_on(&mut run, start)? {
                break;
            }
            start = tb::offset(start, 0, -1, 0);
        }
        if self.can_place_log(&mut run, length, start, direction)? {
            let mut logs = PosSet::default();
            let mut p = start;
            for _ in 0..length {
                logs.insert(self.place_log(&mut run, p, Some(Axis::of(direction)))?);
                p = tb::relative(p, direction, 1);
            }
            self.decorate(&mut run, &logs.to_vec(), &self.log_decorators, nested)?;
        }
        Ok(true)
    }
    fn decorate<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        logs: &[[i32; 3]],
        decorators: &[TreeDecorator],
        nested: &mut NestedPlacer<'_, W, R>,
    ) -> TreeResult<()> {
        if decorators.is_empty() {
            return Ok(());
        }
        let lists = Lists::new(logs.to_vec(), vec![], vec![]);
        for decorator in decorators {
            decorator.place(run, &lists, nested)?;
        }
        Ok(())
    }
    fn place_log<W: FeatureWorld, R: RandomSource>(
        &self,
        run: &mut Run<'_, W, R>,
        p: [i32; 3],
        axis: Option<Axis>,
    ) -> TreeResult<[i32; 3]> {
        let mut state = sample_state(&self.trunk_provider, run.world, run.random, p)?;
        if let Some(axis) = axis {
            state = run.blocks.try_axis(run.world.blocks(), state, axis);
        }
        write(run.world, p, state, 3)?;
        // Feature.markAboveForPostProcessing.
        let mut above = p;
        for _ in 0..2 {
            above = tb::offset(above, 0, 1, 0);
            if run.blocks.is_air(read(run.world, above)?) {
                break;
            }
            run.world
                .mark_postprocessing(BlockPos::new(above[0], above[1], above[2]))?;
        }
        Ok(p)
    }
    fn over_solid_ground<W: FeatureWorld, R>(
        &self,
        run: &mut Run<'_, W, R>,
        p: [i32; 3],
    ) -> TreeResult<bool> {
        let below = read(run.world, tb::offset(p, 0, -1, 0))?;
        Ok(run
            .world
            .state_data()
            .is_face_sturdy(below, Direction::Up, SupportType::Full))
    }
    fn may_place_on<W: FeatureWorld, R>(
        &self,
        run: &mut Run<'_, W, R>,
        p: [i32; 3],
    ) -> TreeResult<bool> {
        Ok(run.blocks.valid_tree_pos(read(run.world, p)?) && self.over_solid_ground(run, p)?)
    }
    fn can_place_log<W: FeatureWorld, R>(
        &self,
        run: &mut Run<'_, W, R>,
        length: i32,
        start: [i32; 3],
        direction: Direction,
    ) -> TreeResult<bool> {
        let mut gap = 0;
        let mut p = start;
        for _ in 0..length {
            if !run.blocks.valid_tree_pos(read(run.world, p)?) {
                return Ok(false);
            }
            if !self.over_solid_ground(run, p)? {
                gap += 1;
                if gap > 2 {
                    return Ok(false);
                }
            } else {
                gap = 0;
            }
            p = tb::relative(p, direction, 1);
        }
        Ok(true)
    }
}
