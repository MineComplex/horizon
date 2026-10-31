//! `worldgen/material_rule` and `worldgen/material_condition`: the surface rules of the noise
//! settings, decoded once into two arenas (`MaterialRule`, `MaterialCondition` and their
//! `Holder` references; a reference shares the referenced node). Level-dependent parts (the
//! seeded noises and random factories) are named here and bound by the level's
//! [`SurfaceSystem`](super::SurfaceSystem). Copied from Horizon's `material_condition/{codec,
//! rule}`, which read the same JSON through a defensive registry; here the registries are the
//! datapacks' elements and Paper's `paper:optionally_flat_bedrock_material_condition` is one more
//! condition type (its bedrock rules come in the `vanilla` pack, see `DataPack::vanilla`).

use std::collections::HashMap;

use serde_json::value::RawValue;

use crate::block::{BlockState, BlockStates};
use crate::worldgen::anchor::{OPTIONALLY_FLAT_BEDROCK, OptionallyFlatBedrock, VerticalAnchor};
use crate::worldgen::codec::{
    DecodeError, DensityLoader, Object, boolean, double, float, identifier, int_range, integer,
    object, optional, required, string,
};
use crate::worldgen::density::FunctionId;
use crate::worldgen::settings::block_state;
use crate::worldgen::structure::{BiomeLookup, BiomeSet};

/// A rule of the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleId(u32);

/// A condition of the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConditionId(u32);

impl ConditionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// `MaterialRule`.
#[derive(Clone, Debug)]
pub enum Rule {
    Block(BlockState),
    Bandlands,
    Sequence(Box<[RuleId]>),
    If {
        condition: ConditionId,
        then: RuleId,
    },
    OreVein(OreVein),
}

/// `OreVeinRule`.
#[derive(Clone, Debug)]
pub struct OreVein {
    pub ore: BlockState,
    pub raw_ore: BlockState,
    pub filler: BlockState,
    pub raw_ore_chance: f32,
    pub density: FunctionId,
    pub richness: FunctionId,
    pub filler_gap: FunctionId,
}

/// A vertical gradient, vanilla's or Paper's optionally flat one.
#[derive(Clone, Debug)]
pub struct Gradient {
    /// Index into [`MaterialRules::random_names`].
    pub random: usize,
    pub true_at_and_below: VerticalAnchor,
    pub false_at_and_above: VerticalAnchor,
    /// `Some(is_roof)` for `paper:optionally_flat_bedrock_material_condition`.
    pub paper_roof: Option<bool>,
}

/// `MaterialCondition`.
#[derive(Clone, Debug)]
pub enum Condition {
    Biome(BiomeSet),
    /// `noise` indexes [`MaterialRules::noise_names`].
    Noise {
        noise: usize,
        min: f64,
        max: f64,
        is_3d: bool,
    },
    Gradient(Gradient),
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not(ConditionId),
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        ceiling: bool,
    },
}

/// The decoded rule and condition registries.
#[derive(Clone, Debug, Default)]
pub struct MaterialRules {
    rules: Vec<Rule>,
    conditions: Vec<Condition>,
    /// The `worldgen/material_rule` elements by id.
    elements: Vec<(String, RuleId)>,
    noise_names: Vec<String>,
    random_names: Vec<String>,
    /// `Biomes.ERODED_BADLANDS`, `FROZEN_OCEAN` and `DEEP_FROZEN_OCEAN`, where registered.
    pub eroded_badlands: Option<u32>,
    pub frozen_oceans: [Option<u32>; 2],
}

impl MaterialRules {
    /// A registry element of `worldgen/material_rule` (what `noise_settings.material_rule` names).
    pub fn element(&self, id: &str) -> Option<RuleId> {
        self.elements
            .iter()
            .find(|(name, _)| name == id)
            .map(|(_, rule)| *rule)
    }
    pub fn rule(&self, id: RuleId) -> &Rule {
        &self.rules[id.0 as usize]
    }
    pub fn condition(&self, id: ConditionId) -> &Condition {
        &self.conditions[id.0 as usize]
    }
    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }
    pub fn noise_names(&self) -> &[String] {
        &self.noise_names
    }
    pub fn random_names(&self) -> &[String] {
        &self.random_names
    }

    /// The density functions of the ore veins below `root`, each once, in rule order.
    pub fn ore_functions(&self, root: RuleId) -> Vec<FunctionId> {
        let mut found = Vec::new();
        let mut seen = vec![false; self.rules.len()];
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut seen[id.0 as usize], true) {
                continue;
            }
            match self.rule(id) {
                Rule::Sequence(rules) => stack.extend(rules.iter().rev()),
                Rule::If { then, .. } => stack.push(*then),
                Rule::OreVein(vein) => {
                    for function in [vein.density, vein.richness, vein.filler_gap] {
                        if !found.contains(&function) {
                            found.push(function);
                        }
                    }
                }
                Rule::Block(_) | Rule::Bandlands => {}
            }
        }
        found
    }

    /// Decodes both registries (elements in registration order). A reference to an element
    /// that does not exist, a cycle or an unknown type fails the load, as the registry
    /// loader's does.
    pub(crate) fn decode(
        rules: &[(String, Box<RawValue>)],
        conditions: &[(String, Box<RawValue>)],
        biomes: &BiomeLookup<'_>,
        densities: &mut DensityLoader<'_>,
        blocks: &BlockStates,
    ) -> Result<Self, DecodeError> {
        let find = |name: &str| {
            biomes
                .ids
                .iter()
                .position(|id| id == name)
                .map(|i| i as u32)
        };
        let mut decoder = Decoder {
            raw_rules: rules,
            raw_conditions: conditions,
            biomes,
            densities,
            blocks,
            out: Self {
                eroded_badlands: find("minecraft:eroded_badlands"),
                frozen_oceans: [
                    find("minecraft:frozen_ocean"),
                    find("minecraft:deep_frozen_ocean"),
                ],
                ..Self::default()
            },
            rule_ids: HashMap::new(),
            condition_ids: HashMap::new(),
            active: Vec::new(),
        };
        for (name, _) in rules {
            let id = decoder.rule_element(name, 0)?;
            decoder.out.elements.push((name.clone(), id));
        }
        for (name, _) in conditions {
            decoder.condition_element(name, 0)?;
        }
        Ok(decoder.out)
    }
}

/// Nesting deeper than this is a cycle (a registry reference loop) or a hostile pack.
const DEPTH: usize = 256;

struct Decoder<'a, 'b> {
    raw_rules: &'a [(String, Box<RawValue>)],
    raw_conditions: &'a [(String, Box<RawValue>)],
    biomes: &'a BiomeLookup<'a>,
    densities: &'a mut DensityLoader<'b>,
    blocks: &'a BlockStates,
    out: MaterialRules,
    rule_ids: HashMap<String, RuleId>,
    condition_ids: HashMap<String, ConditionId>,
    /// The references being decoded, to refuse a cycle.
    active: Vec<String>,
}

fn intern(names: &mut Vec<String>, name: String) -> usize {
    names.iter().position(|n| *n == name).unwrap_or_else(|| {
        names.push(name);
        names.len() - 1
    })
}

impl Decoder<'_, '_> {
    fn push_rule(&mut self, rule: Rule) -> RuleId {
        self.out.rules.push(rule);
        RuleId(self.out.rules.len() as u32 - 1)
    }
    fn push_condition(&mut self, condition: Condition) -> ConditionId {
        self.out.conditions.push(condition);
        ConditionId(self.out.conditions.len() as u32 - 1)
    }

    fn rule_element(&mut self, name: &str, depth: usize) -> Result<RuleId, DecodeError> {
        if let Some(id) = self.rule_ids.get(name) {
            return Ok(*id);
        }
        if depth > DEPTH || self.active.iter().any(|a| a == name) {
            return Err(DecodeError::Field("cyclic material rule"));
        }
        let raw = self
            .raw_rules
            .iter()
            .find(|(id, _)| id == name)
            .ok_or_else(|| DecodeError::Registry(format!("Missing material rule: {name}")))?
            .1
            .clone();
        self.active.push(name.to_owned());
        let id = self.rule(&raw, depth + 1);
        self.active.pop();
        let id = id?;
        self.rule_ids.insert(name.to_owned(), id);
        Ok(id)
    }

    fn condition_element(&mut self, name: &str, depth: usize) -> Result<ConditionId, DecodeError> {
        if let Some(id) = self.condition_ids.get(name) {
            return Ok(*id);
        }
        if depth > DEPTH || self.active.iter().any(|a| a == name) {
            return Err(DecodeError::Field("cyclic material condition"));
        }
        let raw = self
            .raw_conditions
            .iter()
            .find(|(id, _)| id == name)
            .ok_or_else(|| DecodeError::Registry(format!("Missing material condition: {name}")))?
            .1
            .clone();
        self.active.push(name.to_owned());
        let id = self.condition(&raw, depth + 1);
        self.active.pop();
        let id = id?;
        self.condition_ids.insert(name.to_owned(), id);
        Ok(id)
    }

    /// `MaterialRule.CODEC`: a registry id or a rule.
    fn rule(&mut self, raw: &RawValue, depth: usize) -> Result<RuleId, DecodeError> {
        if depth > DEPTH {
            return Err(DecodeError::Field("material rule depth"));
        }
        if raw.get().starts_with('"') {
            return self.rule_element(&identifier(&string(raw)?)?, depth);
        }
        let fields = object(raw)?;
        let kind = identifier(&string(required(&fields, "type")?)?)?;
        let rule = match kind.as_str() {
            "minecraft:block" => Rule::Block(block_state(
                required(&fields, "result_state")?,
                self.blocks,
            )?),
            "minecraft:bandlands" => Rule::Bandlands,
            "minecraft:sequence" => {
                let items: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "sequence")?.get())?;
                Rule::Sequence(
                    items
                        .iter()
                        .map(|item| self.rule(item, depth + 1))
                        .collect::<Result<_, _>>()?,
                )
            }
            "minecraft:condition" => Rule::If {
                condition: self.condition(required(&fields, "if_true")?, depth + 1)?,
                then: self.rule(required(&fields, "then_run")?, depth + 1)?,
            },
            "minecraft:ore_vein" => self.ore_vein(&fields)?,
            _ => {
                return Err(DecodeError::Registry(format!(
                    "Unknown registry key in minecraft:material_rule_type: {kind}"
                )));
            }
        };
        Ok(self.push_rule(rule))
    }

    fn ore_vein(&mut self, fields: &Object) -> Result<Rule, DecodeError> {
        let chance = float(required(fields, "raw_ore_chance")?)?;
        if !(0.0..=1.0).contains(&chance) {
            return Err(DecodeError::Field("raw_ore_chance"));
        }
        let state = |name| block_state(required(fields, name)?, self.blocks);
        let (ore, raw_ore, filler) = (
            state("ore_block")?,
            state("raw_ore_block")?,
            state("filler_block")?,
        );
        let mut function = |name| self.densities.decode(required(fields, name)?, 0);
        Ok(Rule::OreVein(OreVein {
            ore,
            raw_ore,
            filler,
            raw_ore_chance: chance,
            density: function("density")?,
            richness: function("richness")?,
            filler_gap: function("filler_gap")?,
        }))
    }

    /// `MaterialCondition.CODEC`: a registry id or a condition.
    fn condition(&mut self, raw: &RawValue, depth: usize) -> Result<ConditionId, DecodeError> {
        if depth > DEPTH {
            return Err(DecodeError::Field("material condition depth"));
        }
        if raw.get().starts_with('"') {
            return self.condition_element(&identifier(&string(raw)?)?, depth);
        }
        let fields = object(raw)?;
        let kind = identifier(&string(required(&fields, "type")?)?)?;
        let int = |name: &'static str| integer(required(&fields, name)?);
        let multiplier = || {
            int_range(
                required(&fields, "surface_depth_multiplier")?,
                "surface_depth_multiplier",
                -20,
                20,
            )
        };
        let flag = |name: &'static str| boolean(required(&fields, name)?);
        let condition = match kind.as_str() {
            "minecraft:biome" => {
                Condition::Biome(self.biomes.decode(required(&fields, "biome_is")?)?)
            }
            "minecraft:noise_threshold" => Condition::Noise {
                noise: intern(
                    &mut self.out.noise_names,
                    identifier(&string(required(&fields, "noise")?)?)?,
                ),
                min: double(required(&fields, "min_threshold")?)?,
                max: double(required(&fields, "max_threshold")?)?,
                is_3d: optional(&fields, "is_3d")
                    .map(boolean)
                    .transpose()?
                    .unwrap_or(false),
            },
            "minecraft:vertical_gradient" => Condition::Gradient(Gradient {
                random: intern(
                    &mut self.out.random_names,
                    identifier(&string(required(&fields, "random_name")?)?)?,
                ),
                true_at_and_below: VerticalAnchor::decode(required(&fields, "true_at_and_below")?)?,
                false_at_and_above: VerticalAnchor::decode(required(
                    &fields,
                    "false_at_and_above",
                )?)?,
                paper_roof: None,
            }),
            OPTIONALLY_FLAT_BEDROCK => {
                let paper = OptionallyFlatBedrock::decode(raw)?;
                Condition::Gradient(Gradient {
                    random: intern(&mut self.out.random_names, paper.random_name),
                    true_at_and_below: paper.true_at_and_below,
                    false_at_and_above: paper.false_at_and_above,
                    paper_roof: Some(paper.is_roof),
                })
            }
            "minecraft:y_above" => Condition::YAbove {
                anchor: VerticalAnchor::decode(required(&fields, "anchor")?)?,
                surface_depth_multiplier: multiplier()?,
                add_stone_depth: flag("add_stone_depth")?,
            },
            "minecraft:water" => Condition::Water {
                offset: int("offset")?,
                surface_depth_multiplier: multiplier()?,
                add_stone_depth: flag("add_stone_depth")?,
            },
            "minecraft:temperature" => Condition::Temperature,
            "minecraft:steep" => Condition::Steep,
            "minecraft:not" => {
                Condition::Not(self.condition(required(&fields, "invert")?, depth + 1)?)
            }
            "minecraft:hole" => Condition::Hole,
            "minecraft:above_preliminary_surface" => Condition::AbovePreliminarySurface,
            "minecraft:stone_depth" => Condition::StoneDepth {
                offset: int("offset")?,
                add_surface_depth: flag("add_surface_depth")?,
                secondary_depth_range: int("secondary_depth_range")?,
                ceiling: match string(required(&fields, "surface_type")?)?.as_str() {
                    "floor" => false,
                    "ceiling" => true,
                    _ => return Err(DecodeError::Field("surface_type")),
                },
            },
            _ => {
                return Err(DecodeError::Registry(format!(
                    "Unknown registry key in minecraft:material_condition_type: {kind}"
                )));
            }
        };
        Ok(self.push_condition(condition))
    }
}
