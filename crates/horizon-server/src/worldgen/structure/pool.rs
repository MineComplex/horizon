//! The `worldgen/template_pool` registry (`StructureTemplatePool` and its `StructurePoolElement`s),
//! decoded once at startup into an arena: pools and elements are indices, a list element's children
//! are elements of the same arena, and each pool's weighted template list (`templates`, an element per
//! unit of weight) and `getMaxSize` are computed eagerly (vanilla fills `maxSize` on first use).
//!
//! Element types: `single_pool_element`, `legacy_single_pool_element` (the same with
//! `STRUCTURE_AND_AIR` ignored instead of structure blocks), `feature_pool_element` (a placed
//! feature, by id), `list_pool_element` (its children take its projection, `setProjectionOnEachElement`)
//! and `empty_pool_element` (`EmptyPoolElement.INSTANCE`). An element saves itself as
//! `StructurePoolElement.CODEC` writes it into a jigsaw piece's `pool_element`; an inline processor
//! list is written from its JSON with the codec's number types (see [`TemplatePools::element_tag`]).

use std::collections::HashMap;

use serde_json::value::RawValue;

use super::place::LiquidSettings;
use super::processor::{ProcessorListHolder, ProcessorLists, json_to_nbt};
use super::template::{StructureTemplate, StructureTemplates};
use crate::block::transform::Rotation;
use crate::core::position::BlockPos;
use crate::nbt::{Compound, JavaString, Tag};
use crate::worldgen::codec::{
    DecodeError, identifier, int_range, object, optional, required, string,
};
use crate::worldgen::density::StructureBounds;
use crate::worldgen::feature::{FeatureRegistry, PlacedFeatureId};
use crate::worldgen::placement::DecodeContext;

/// A pool: its index in [`TemplatePools`] (registration order, then inline pools).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PoolId(pub u32);

/// An element of the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PoolElementId(pub u32);

/// `StructureTemplatePool.Projection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    TerrainMatching,
    Rigid,
}

impl Projection {
    pub fn name(self) -> &'static str {
        match self {
            Self::TerrainMatching => "terrain_matching",
            Self::Rigid => "rigid",
        }
    }
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "terrain_matching" => Some(Self::TerrainMatching),
            "rigid" => Some(Self::Rigid),
            _ => None,
        }
    }
}

/// What an element places.
#[derive(Clone, Debug)]
pub enum ElementKind {
    Empty,
    /// `SinglePoolElement` (`legacy`: `LegacySinglePoolElement`).
    Single {
        legacy: bool,
        /// The template id (`Identifier`).
        location: String,
        processors: ProcessorListHolder,
        /// The processors as the element saves them (an id, or the inline list).
        processors_tag: Tag,
        liquid: Option<LiquidSettings>,
    },
    Feature {
        feature: PlacedFeatureId,
        name: String,
    },
    List(Vec<PoolElementId>),
}

/// A `StructurePoolElement`.
#[derive(Clone, Debug)]
pub struct PoolElement {
    pub kind: ElementKind,
    pub projection: Projection,
}

/// A `StructureTemplatePool`.
#[derive(Clone, Debug)]
pub struct TemplatePool {
    pub id: String,
    pub fallback: PoolId,
    /// `rawTemplates`: each element with its weight, in the file's order.
    pub raw: Vec<(PoolElementId, i32)>,
    /// `templates`: each element repeated by its weight.
    pub templates: Vec<PoolElementId>,
    /// `getMaxSize`: the tallest non-empty element's box (0 without one).
    pub max_size: i32,
}

/// The `worldgen/template_pool` registry.
#[derive(Debug, Default)]
pub struct TemplatePools {
    pools: Vec<TemplatePool>,
    elements: Vec<PoolElement>,
    ids: HashMap<String, PoolId>,
    /// `minecraft:empty` (`Pools.EMPTY`), when registered.
    empty: Option<PoolId>,
}

/// `Pools.EMPTY`.
pub const EMPTY_POOL: &str = "minecraft:empty";

/// What decoding elements reads besides the JSON.
pub(crate) struct PoolContext<'a> {
    pub names: &'a DecodeContext<'a>,
    pub processors: &'a ProcessorLists,
    pub features: &'a FeatureRegistry,
}

fn liquid_settings(name: &str) -> Result<LiquidSettings, DecodeError> {
    match name {
        "apply_waterlogging" => Ok(LiquidSettings::ApplyWaterlogging),
        "ignore_waterlogging" => Ok(LiquidSettings::IgnoreWaterlogging),
        _ => Err(DecodeError::Field("liquid_settings")),
    }
}

pub(crate) fn liquid_name(liquid: LiquidSettings) -> &'static str {
    match liquid {
        LiquidSettings::ApplyWaterlogging => "apply_waterlogging",
        LiquidSettings::IgnoreWaterlogging => "ignore_waterlogging",
    }
}

pub(crate) fn decode_liquid(raw: &RawValue) -> Result<LiquidSettings, DecodeError> {
    liquid_settings(&string(raw)?)
}

/// Fields whose codec is a float or an int, for writing an inline processor list's JSON as NBT the
/// way the codecs re-encode it (`json_to_nbt` alone picks the narrowest type).
const FLOAT_FIELDS: [&str; 5] = [
    "integrity",
    "mossiness",
    "probability",
    "min_chance",
    "max_chance",
];
const INT_FIELDS: [&str; 7] = [
    "offset",
    "min_dist",
    "max_dist",
    "min_inclusive",
    "max_inclusive",
    "value",
    "limit",
];

fn typed(key: &str, tag: Tag) -> Tag {
    let number = match &tag {
        Tag::Byte(v) => Some(f64::from(*v)),
        Tag::Short(v) => Some(f64::from(*v)),
        Tag::Int(v) => Some(f64::from(*v)),
        Tag::Long(v) => Some(*v as f64),
        Tag::Float(v) => Some(f64::from(f32::from_bits(*v))),
        Tag::Double(v) => Some(f64::from_bits(*v)),
        _ => None,
    };
    match number {
        Some(v) if FLOAT_FIELDS.contains(&key) => Tag::Float((v as f32).to_bits()),
        Some(v) if INT_FIELDS.contains(&key) => Tag::Int(v as i32),
        _ => tag,
    }
}

fn processors_tag(value: &serde_json::Value, key: &str) -> Tag {
    use serde_json::Value;
    match value {
        Value::Object(map) => Tag::Compound(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (JavaString::from(k.as_str()), processors_tag(v, k)))
                .collect(),
        ),
        Value::Array(items) if items.iter().any(|v| v.is_object()) => {
            Tag::List(items.iter().map(|v| processors_tag(v, key)).collect())
        }
        other => typed(key, json_to_nbt(other)),
    }
}

impl TemplatePools {
    pub fn pool(&self, id: PoolId) -> &TemplatePool {
        &self.pools[id.0 as usize]
    }
    pub fn element(&self, id: PoolElementId) -> &PoolElement {
        &self.elements[id.0 as usize]
    }
    pub fn id(&self, name: &str) -> Option<PoolId> {
        self.ids.get(name).copied()
    }
    pub fn len(&self) -> usize {
        self.pools.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }
    /// `targetPool.is(Pools.EMPTY)`.
    pub fn is_empty_pool(&self, id: PoolId) -> bool {
        self.empty == Some(id)
    }

    /// Decodes every pool of `elements` (registration order; fallbacks may name any of them).
    pub(crate) fn decode(
        elements: &[(String, Box<RawValue>)],
        cx: &PoolContext<'_>,
    ) -> Result<Self, (usize, DecodeError)> {
        let mut pools = Self::default();
        for (i, (id, _)) in elements.iter().enumerate() {
            pools.ids.insert(id.clone(), PoolId(i as u32));
        }
        pools.empty = pools.id(EMPTY_POOL);
        // Placeholders so inline fallbacks (appended) get the ids after the registered ones.
        pools.pools = elements
            .iter()
            .map(|(id, _)| TemplatePool {
                id: id.clone(),
                fallback: PoolId(0),
                raw: Vec::new(),
                templates: Vec::new(),
                max_size: 0,
            })
            .collect();
        for (i, (id, raw)) in elements.iter().enumerate() {
            let pool = pools.decode_pool(id, raw, cx).map_err(|e| (i, e))?;
            pools.pools[i] = pool;
        }
        Ok(pools)
    }

    fn decode_pool(
        &mut self,
        id: &str,
        raw: &RawValue,
        cx: &PoolContext<'_>,
    ) -> Result<TemplatePool, DecodeError> {
        let fields = object(raw)?;
        let fallback = self.holder(required(&fields, "fallback")?, cx)?;
        let entries: Vec<Box<RawValue>> =
            serde_json::from_str(required(&fields, "elements")?.get())?;
        let mut pool = TemplatePool {
            id: id.to_owned(),
            fallback,
            raw: Vec::new(),
            templates: Vec::new(),
            max_size: 0,
        };
        for entry in &entries {
            let entry = object(entry)?;
            let element = self.decode_element(required(&entry, "element")?, None, cx)?;
            let weight = int_range(required(&entry, "weight")?, "weight", 1, 150)?;
            pool.raw.push((element, weight));
            pool.templates
                .extend(std::iter::repeat_n(element, weight as usize));
        }
        Ok(pool)
    }

    /// `StructureTemplatePool.CODEC`: a registered id or an inline pool (appended).
    pub(crate) fn holder(
        &mut self,
        raw: &RawValue,
        cx: &PoolContext<'_>,
    ) -> Result<PoolId, DecodeError> {
        if raw.get().trim_start().starts_with('{') {
            let index = PoolId(self.pools.len() as u32);
            let id = format!("<inline pool {}>", index.0);
            self.pools.push(TemplatePool {
                id: id.clone(),
                fallback: PoolId(0),
                raw: Vec::new(),
                templates: Vec::new(),
                max_size: 0,
            });
            let pool = self.decode_pool(&id, raw, cx)?;
            self.pools[index.0 as usize] = pool;
            return Ok(index);
        }
        let name = identifier(&string(raw)?)?;
        self.id(&name)
            .ok_or_else(|| DecodeError::Registry(format!("Missing template pool: {name}")))
    }

    /// `StructurePoolElement.CODEC`; `projection` overrides the element's own (a list's children).
    fn decode_element(
        &mut self,
        raw: &RawValue,
        projection: Option<Projection>,
        cx: &PoolContext<'_>,
    ) -> Result<PoolElementId, DecodeError> {
        let fields = object(raw)?;
        let kind = identifier(&string(required(&fields, "element_type")?)?)?;
        let own_projection = || -> Result<Projection, DecodeError> {
            Projection::by_name(&string(required(&fields, "projection")?)?)
                .ok_or(DecodeError::Field("projection"))
        };
        let element = match kind.as_str() {
            "minecraft:empty_pool_element" => PoolElement {
                kind: ElementKind::Empty,
                projection: Projection::TerrainMatching,
            },
            "minecraft:single_pool_element" | "minecraft:legacy_single_pool_element" => {
                let location = identifier(&string(required(&fields, "location")?)?)?;
                let raw_processors = required(&fields, "processors")?;
                let processors = cx.processors.holder(raw_processors, cx.names)?;
                let processors_tag = match &processors {
                    ProcessorListHolder::Registry(id) => {
                        Tag::String(cx.processors.name(*id).into())
                    }
                    ProcessorListHolder::Inline(_) => {
                        let value: serde_json::Value = serde_json::from_str(raw_processors.get())?;
                        // `DIRECT_CODEC` encodes the object form.
                        let value = match value {
                            serde_json::Value::Array(list) => {
                                serde_json::json!({ "processors": list })
                            }
                            other => other,
                        };
                        processors_tag(&value, "")
                    }
                };
                PoolElement {
                    kind: ElementKind::Single {
                        legacy: kind == "minecraft:legacy_single_pool_element",
                        location,
                        processors,
                        processors_tag,
                        liquid: optional(&fields, "override_liquid_settings")
                            .map(decode_liquid)
                            .transpose()?,
                    },
                    projection: projection.map_or_else(own_projection, Ok)?,
                }
            }
            "minecraft:feature_pool_element" => {
                let reference = required(&fields, "feature")?;
                if reference.get().trim_start().starts_with('{') {
                    return Err(DecodeError::Registry(
                        "inline placed features in pool elements are not supported".into(),
                    ));
                }
                let name = identifier(&string(reference)?)?;
                let feature = cx.features.placed_id(&name).ok_or_else(|| {
                    DecodeError::Registry(format!("Missing placed feature: {name}"))
                })?;
                PoolElement {
                    kind: ElementKind::Feature { feature, name },
                    projection: projection.map_or_else(own_projection, Ok)?,
                }
            }
            "minecraft:list_pool_element" => {
                let projection = projection.map_or_else(own_projection, Ok)?;
                let children: Vec<Box<RawValue>> =
                    serde_json::from_str(required(&fields, "elements")?.get())?;
                if children.is_empty() {
                    return Err(DecodeError::Registry("Elements are empty".into()));
                }
                let children = children
                    .iter()
                    .map(|c| self.decode_element(c, Some(projection), cx))
                    .collect::<Result<_, _>>()?;
                PoolElement {
                    kind: ElementKind::List(children),
                    projection,
                }
            }
            other => {
                return Err(DecodeError::Registry(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/structure_pool_element]: {other}"
                )));
            }
        };
        self.elements.push(element);
        Ok(PoolElementId(self.elements.len() as u32 - 1))
    }

    /// Fills each pool's `getMaxSize` from the templates.
    pub fn compute_max_sizes(&mut self, templates: &StructureTemplates) {
        let sizes: Vec<i32> = (self.pools.iter())
            .map(|pool| {
                (pool.templates.iter())
                    .filter(|&&e| !matches!(self.element(e).kind, ElementKind::Empty))
                    .map(|&e| {
                        let b =
                            self.bounding_box(templates, e, BlockPos::default(), Rotation::None);
                        b.maximum()[1] - b.minimum()[1] + 1
                    })
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for (pool, size) in self.pools.iter_mut().zip(sizes) {
            pool.max_size = size;
        }
    }

    /// `getBoundingBox(manager, position, rotation)`; an empty element (vanilla throws) is the
    /// position's block.
    pub fn bounding_box(
        &self,
        templates: &StructureTemplates,
        element: PoolElementId,
        position: BlockPos,
        rotation: Rotation,
    ) -> StructureBounds {
        let p = [position.x, position.y, position.z];
        match &self.element(element).kind {
            ElementKind::Single { location, .. } => template_of(templates, location).bounding_box(
                position,
                rotation,
                BlockPos::default(),
                crate::block::transform::Mirror::None,
            ),
            ElementKind::Feature { .. } | ElementKind::Empty => StructureBounds::from_corners(p, p),
            ElementKind::List(children) => (children.iter())
                .filter(|&&c| !matches!(self.element(c).kind, ElementKind::Empty))
                .map(|&c| self.bounding_box(templates, c, position, rotation))
                .reduce(StructureBounds::encapsulating)
                .unwrap_or_else(|| StructureBounds::from_corners(p, p)),
        }
    }

    /// The element as `StructurePoolElement.CODEC` writes it.
    pub fn element_tag(&self, element: PoolElementId) -> Compound {
        let element = self.element(element);
        let mut tag = Compound::new();
        let mut put = |key: &str, value: Tag| {
            tag.insert(key.into(), value);
        };
        let projection = Tag::String(element.projection.name().into());
        match &element.kind {
            ElementKind::Empty => put(
                "element_type",
                Tag::String("minecraft:empty_pool_element".into()),
            ),
            ElementKind::Single {
                legacy,
                location,
                processors_tag,
                liquid,
                ..
            } => {
                let kind = if *legacy {
                    "minecraft:legacy_single_pool_element"
                } else {
                    "minecraft:single_pool_element"
                };
                put("element_type", Tag::String(kind.into()));
                put("location", Tag::String(location.as_str().into()));
                put("processors", processors_tag.clone());
                put("projection", projection);
                if let Some(liquid) = liquid {
                    put(
                        "override_liquid_settings",
                        Tag::String(liquid_name(*liquid).into()),
                    );
                }
            }
            ElementKind::Feature { name, .. } => {
                put(
                    "element_type",
                    Tag::String("minecraft:feature_pool_element".into()),
                );
                put("feature", Tag::String(name.as_str().into()));
                put("projection", projection);
            }
            ElementKind::List(children) => {
                put(
                    "element_type",
                    Tag::String("minecraft:list_pool_element".into()),
                );
                let list = children
                    .iter()
                    .map(|&c| Tag::Compound(self.element_tag(c)))
                    .collect();
                put("elements", Tag::List(list));
                put("projection", projection);
            }
        }
        tag
    }

    /// The registry element a saved `pool_element` tag stands for: the one writing the same tag,
    /// else (a tag another encoder wrote) the one with the same type, template or feature,
    /// projection, liquid settings and processor list id. `None` when nothing matches (vanilla
    /// would decode the element itself; a datapack changed since the save).
    pub fn find_element(&self, tag: &Compound) -> Option<PoolElementId> {
        let ids = (0..self.elements.len() as u32).map(PoolElementId);
        if let Some(found) = ids.clone().find(|&e| self.element_tag(e) == *tag) {
            return Some(found);
        }
        let key = |tag: &Compound| -> Vec<String> {
            let text = |k: &str| match tag.get(&JavaString::from(k)) {
                Some(Tag::String(s)) => s.to_string_lossy(),
                Some(Tag::Compound(_)) => "<inline>".to_owned(),
                _ => String::new(),
            };
            let mut key: Vec<String> = [
                "element_type",
                "location",
                "feature",
                "projection",
                "override_liquid_settings",
                "processors",
            ]
            .iter()
            .map(|k| text(k))
            .collect();
            if let Some(Tag::List(children)) = tag.get(&JavaString::from("elements")) {
                key.extend(children.iter().map(|c| format!("{c:?}")));
            }
            key
        };
        let wanted = key(tag);
        ids.into_iter()
            .find(|&e| key(&self.element_tag(e)) == wanted)
    }
}

/// `getOrCreate`: an unknown template is an empty one.
pub(crate) fn template_of<'t>(
    templates: &'t StructureTemplates,
    id: &str,
) -> &'t StructureTemplate {
    static EMPTY: std::sync::LazyLock<StructureTemplate> =
        std::sync::LazyLock::new(StructureTemplate::default);
    templates.get(id).unwrap_or(&EMPTY)
}
