//! Item stacks on the wire (`ItemStack.OPTIONAL_STREAM_CODEC`): the component
//! patch read back as vanilla's `components` compound, so the decoder the saved
//! and command item NBT use fills every field. Both sides use it: a client for
//! what its server sends, a server for a creative client's
//! `SetCreativeModeSlot`; and the other direction, a stack as it goes out
//! (`stack_to_network`).
use crate::items::ItemStack;
use horizon_protocol::item::{Component, Profile, Stack, component};
use horizon_protocol::registry::{Builtin, Dynamic};
use horizon_world::nbt::Tag;
use std::sync::Arc;

/// A stack from the wire: its item by the built-in registry's id, its
/// components by the synced registries' (enchantments, trims).
pub fn from_network(s: &Stack, builtin: &Builtin, synced: &Dynamic) -> ItemStack {
    let mut out = ItemStack {
        item: item_name(builtin, s.item),
        count: s.count.max(0) as u32,
        damage: 0,
        extra: None,
    };
    let components = components_tag(s, builtin, synced);
    let (damage, data) = crate::item_data::decode_components(components.as_ref());
    out.damage = damage;
    if let Some(data) = data {
        *out.data_mut() = *data;
    }
    out.tidy();
    out
}

fn item_name(builtin: &Builtin, id: i32) -> Arc<str> {
    Arc::from(
        builtin
            .get("minecraft:item")
            .and_then(|i| i.name(id))
            .unwrap_or("minecraft:air"),
    )
}

/// The item's patched components as vanilla's `components` compound.
fn components_tag(s: &Stack, builtin: &Builtin, synced: &Dynamic) -> Option<Tag> {
    let name = |id: i32| builtin.get("minecraft:data_component_type")?.name(id);
    let mut entries: Vec<(&str, Tag)> = Vec::new();
    for (kind, c) in &s.added {
        let Some(key) = name(*kind) else {
            continue;
        };
        let value = match c {
            // Read past, not decoded: the item keeps its prototype's value.
            Component::Raw(_) => continue,
            // Trim materials and patterns are synced registries, as enchantments are.
            Component::Trim { material, pattern } => {
                let (Some(material), Some(pattern)) = (
                    synced.name("minecraft:trim_material", *material),
                    synced.name("minecraft:trim_pattern", *pattern),
                ) else {
                    continue;
                };
                Tag::compound([
                    ("material", Tag::String(material.into())),
                    ("pattern", Tag::String(pattern.into())),
                ])
            }
            Component::Int(v) if *kind == component::RARITY => {
                let rarity = ["common", "uncommon", "rare", "epic"];
                Tag::String(rarity.get(*v as usize).copied().unwrap_or("common").into())
            }
            Component::Int(v) => Tag::Int(*v),
            Component::Unit => Tag::Compound(Default::default()),
            Component::Bool(v) => Tag::Byte(i8::from(*v)),
            Component::Text(t) | Component::Nbt(t) => t.clone(),
            Component::Ident(v) => Tag::String(v.as_str().into()),
            Component::Lore(lines) => Tag::List(lines.clone()),
            Component::WritableBook(pages) => Tag::compound([(
                "pages",
                Tag::List(
                    pages
                        .iter()
                        .map(|p| Tag::String(p.as_str().into()))
                        .collect(),
                ),
            )]),
            Component::WrittenBook(b) => Tag::compound([
                ("title", Tag::String(b.title.as_str().into())),
                ("author", Tag::String(b.author.as_str().into())),
                ("generation", Tag::Int(b.generation)),
                ("pages", Tag::List(b.pages.clone())),
                ("resolved", Tag::Byte(i8::from(b.resolved))),
            ]),
            Component::Fireworks { flight, explosions } => Tag::compound([
                ("flight_duration", Tag::Byte(*flight as i8)),
                (
                    "explosions",
                    Tag::List(explosions.iter().map(fireworks_tag).collect()),
                ),
            ]),
            Component::FireworkExplosion(e) => fireworks_tag(e),
            Component::LodestoneTracker { target, tracked } => {
                let mut entries = Vec::new();
                if let Some((dimension, pos)) = target {
                    entries.push((
                        "target",
                        Tag::compound([
                            ("dimension", Tag::String(dimension.as_str().into())),
                            ("pos", Tag::IntArray(pos.to_vec())),
                        ]),
                    ));
                }
                if !tracked {
                    entries.push(("tracked", Tag::Byte(0)));
                }
                Tag::compound(entries)
            }
            Component::Enchantments(list) => {
                // Enchantments are a synced (data-driven) registry: the
                // ids index the server's RegistryData, not the builtin
                // report, so a datapack enchantment names correctly.
                let levels = list.iter().filter_map(|(id, level)| {
                    let name = synced.name("minecraft:enchantment", *id)?;
                    Some((name, Tag::Int(*level)))
                });
                Tag::compound(levels)
            }
            Component::AttackRange(v) => Tag::compound([
                ("min_reach", Tag::Float(v[0].to_bits())),
                ("max_reach", Tag::Float(v[1].to_bits())),
                ("min_attack_momentum", Tag::Float(v[2].to_bits())),
                ("max_attack_momentum", Tag::Float(v[3].to_bits())),
                ("hitbox_margin", Tag::Float(v[4].to_bits())),
                ("mob_factor", Tag::Float(v[5].to_bits())),
            ]),
            Component::Profile(p) => profile_tag(p),
            // PotionContents' NBT: the potion by name and the custom colour.
            Component::Potion { potion, color, .. } => {
                let potion = potion.and_then(|id| builtin.get("minecraft:potion")?.name(id));
                Tag::compound(
                    potion
                        .map(|name| ("potion", Tag::String(name.into())))
                        .into_iter()
                        .chain(color.map(|c| ("custom_color", Tag::Int(c)))),
                )
            }
            // ItemStackTemplate.CODEC: id, count and components.
            Component::Stacks(stacks) => Tag::List(
                stacks
                    .iter()
                    .map(|t| {
                        let mut entries = vec![
                            (
                                "id",
                                Tag::String(item_name(builtin, t.item).as_ref().into()),
                            ),
                            ("count", Tag::Int(t.count)),
                        ];
                        if let Some(c) = components_tag(t, builtin, synced) {
                            entries.push(("components", c));
                        }
                        Tag::compound(entries)
                    })
                    .collect(),
            ),
            Component::CustomModelData {
                floats,
                flags,
                strings,
                colors,
            } => Tag::compound([
                (
                    "floats",
                    Tag::List(floats.iter().map(|f| Tag::Float(f.to_bits())).collect()),
                ),
                (
                    "flags",
                    Tag::List(flags.iter().map(|b| Tag::Byte(i8::from(*b))).collect()),
                ),
                (
                    "strings",
                    Tag::List(
                        strings
                            .iter()
                            .map(|s| Tag::String(s.as_str().into()))
                            .collect(),
                    ),
                ),
                (
                    "colors",
                    Tag::List(colors.iter().map(|c| Tag::Int(*c)).collect()),
                ),
            ]),
            Component::TooltipDisplay { hide, hidden } => {
                let hidden = hidden
                    .iter()
                    .filter_map(|id| Some(Tag::String(name(*id)?.into())));
                Tag::compound([
                    ("hide_tooltip", Tag::Byte(i8::from(*hide))),
                    ("hidden_components", Tag::List(hidden.collect())),
                ])
            }
        };
        entries.push((key, value));
    }
    Some(Tag::compound(entries))
}

/// A wire `FireworkExplosion` as vanilla's NBT codec writes it
/// (`FireworkExplosion.Shape`'s BY_ID order).
fn fireworks_tag(e: &horizon_protocol::item::FireworkExplosion) -> Tag {
    const SHAPES: [&str; 5] = ["small_ball", "large_ball", "star", "creeper", "burst"];
    let ints = |c: &[i32]| Tag::List(c.iter().map(|c| Tag::Int(*c)).collect());
    Tag::compound([
        (
            "shape",
            Tag::String(
                SHAPES
                    .get(e.shape.max(0) as usize)
                    .copied()
                    .unwrap_or("small_ball")
                    .into(),
            ),
        ),
        ("colors", ints(&e.colors)),
        ("fade_colors", ints(&e.fade)),
        ("has_trail", Tag::Byte(i8::from(e.trail))),
        ("has_twinkle", Tag::Byte(i8::from(e.twinkle))),
    ])
}

/// A `minecraft:profile` component as the item data JSON the model reads.
fn profile_tag(p: &Profile) -> Tag {
    let properties = p
        .properties
        .iter()
        .map(|(name, value, signature)| {
            let mut entries: Vec<(&'static str, Tag)> = vec![
                ("name", Tag::String(name.as_str().into())),
                ("value", Tag::String(value.as_str().into())),
            ];
            if let Some(signature) = signature {
                entries.push(("signature", Tag::String(signature.as_str().into())));
            }
            Tag::compound(entries)
        })
        .collect::<Vec<_>>();
    let mut entries: Vec<(&'static str, Tag)> = Vec::new();
    if let Some(name) = &p.name {
        entries.push(("name", Tag::String(name.as_str().into())));
    }
    if let Some(id) = p.id {
        entries.push((
            "id",
            Tag::IntArray(vec![
                (id >> 96) as i32,
                (id >> 64) as i32,
                (id >> 32) as i32,
                id as i32,
            ]),
        ));
    }
    entries.push(("properties", Tag::List(properties)));
    Tag::compound(entries)
}

/// `FireworkExplosion.STREAM_CODEC`'s fields (`Shape` by its BY_ID order).
fn network_explosion(
    e: &crate::particles::FireworkExplosion,
) -> horizon_protocol::item::FireworkExplosion {
    horizon_protocol::item::FireworkExplosion {
        shape: e.shape as i32,
        colors: e.colors.iter().map(|c| *c as i32).collect(),
        fade: e.fade.iter().map(|c| *c as i32).collect(),
        trail: e.trail,
        twinkle: e.twinkle,
    }
}

/// A rocket's `minecraft:fireworks` and a star's `minecraft:firework_explosion`.
pub fn firework_components(
    d: &crate::item_data::StackData,
) -> Vec<(i32, horizon_protocol::item::Component)> {
    use horizon_protocol::item::{Component, component};
    let mut out = Vec::new();
    if let Some((flight, explosions)) = &d.fireworks {
        out.push((
            component::FIREWORKS,
            Component::Fireworks {
                flight: i32::from(*flight),
                explosions: explosions.iter().map(network_explosion).collect(),
            },
        ));
    }
    if let Some(e) = &d.firework_explosion {
        out.push((
            component::FIREWORK_EXPLOSION,
            Component::FireworkExplosion(network_explosion(e)),
        ));
    }
    out
}

/// The off hand slot is conventionally 40 in saves and 45 in the menu.
/// `synced_id` names an entry of a synced registry (enchantments, trim
/// materials and patterns) by its id on this connection.
pub fn stack_to_network(
    item_id: impl Fn(&str) -> i32,
    synced_id: impl Fn(&str, &str) -> Option<i32>,
    s: &Option<ItemStack>,
) -> Option<Stack> {
    network_stack(&item_id, &synced_id, s)
}
fn network_stack(
    item_id: &dyn Fn(&str) -> i32,
    synced_id: &dyn Fn(&str, &str) -> Option<i32>,
    s: &Option<ItemStack>,
) -> Option<Stack> {
    let enchantment_id = |name: &str| synced_id("minecraft:enchantment", name);
    let s = s.as_ref()?;
    let mut stack = Stack::new(item_id(&s.item), s.count as i32);
    if s.damage > 0 {
        stack.added.push((
            horizon_protocol::item::component::DAMAGE,
            horizon_protocol::item::Component::Int(s.damage as i32),
        ));
    }
    let enchantments: Vec<(i32, i32)> = s
        .enchantments()
        .iter()
        .filter_map(|(e, level)| Some((enchantment_id(e)?, *level as i32)))
        .collect();
    if !enchantments.is_empty() {
        stack.added.push((
            horizon_protocol::item::component::ENCHANTMENTS,
            horizon_protocol::item::Component::Enchantments(enchantments),
        ));
    }
    let stored: Vec<(i32, i32)> = s
        .stored_enchantments()
        .iter()
        .filter_map(|(e, level)| Some((enchantment_id(e)?, *level as i32)))
        .collect();
    if !stored.is_empty() {
        stack.added.push((
            horizon_protocol::item::component::STORED_ENCHANTMENTS,
            horizon_protocol::item::Component::Enchantments(stored),
        ));
    }
    if let Some(d) = s.data() {
        if let Some(rgb) = d.dyed_color {
            stack.added.push((
                horizon_protocol::item::component::DYED_COLOR,
                horizon_protocol::item::Component::Int(rgb),
            ));
        }
        // ArmorTrim: both holders as references into the synced registries.
        if let Some((material, pattern)) = &d.trim {
            let id = |registry: &str, name: &str| {
                if name.contains(':') {
                    synced_id(registry, name)
                } else {
                    synced_id(registry, &format!("minecraft:{name}"))
                }
            };
            if let (Some(material), Some(pattern)) = (
                id("minecraft:trim_material", material),
                id("minecraft:trim_pattern", pattern),
            ) {
                stack.added.push((
                    horizon_protocol::item::component::TRIM,
                    horizon_protocol::item::Component::Trim { material, pattern },
                ));
            }
        }
        if let Some(id) = d.map_id {
            stack.added.push((
                horizon_protocol::item::component::MAP_ID,
                horizon_protocol::item::Component::Int(id),
            ));
        }
        if d.repair_cost > 0 {
            stack.added.push((
                horizon_protocol::item::component::REPAIR_COST,
                horizon_protocol::item::Component::Int(d.repair_cost as i32),
            ));
        }
        if let Some(mode) = d.map_post_processing {
            stack.added.push((
                horizon_protocol::item::component::MAP_POST_PROCESSING,
                horizon_protocol::item::Component::Int(i32::from(mode)),
            ));
        }
        stack.added.extend(firework_components(d));
        // The compass's lodestone target.
        if let Some(t) = d.other("minecraft:lodestone_tracker") {
            let target = t.get("target").and_then(|g| {
                let Some(horizon_world::nbt::Tag::IntArray(pos)) = g.get("pos") else {
                    return None;
                };
                let dimension = crate::save::text(g.get("dimension"))?;
                Some((dimension, [*pos.first()?, *pos.get(1)?, *pos.get(2)?]))
            });
            let tracked = crate::save::number(t.get("tracked")).is_none_or(|v| v != 0);
            stack.added.push((
                horizon_protocol::item::component::LODESTONE_TRACKER,
                horizon_protocol::item::Component::LodestoneTracker { target, tracked },
            ));
        }
        // PotionContents: the potion by its `minecraft:potion` id.
        if let Some(id) = (d.potion.as_deref()).and_then(|potion| {
            synced_id(
                "minecraft:potion",
                &horizon_world::ident::namespaced(potion),
            )
        }) {
            stack.added.push((
                horizon_protocol::item::component::POTION_CONTENTS,
                horizon_protocol::item::Component::Potion {
                    potion: Some(id),
                    color: None,
                    rest: Vec::new(),
                },
            ));
        }
        // A loaded crossbow's ammunition.
        if !d.charged.is_empty() {
            let charged = d
                .charged
                .iter()
                .filter_map(|c| network_stack(item_id, synced_id, &Some(c.clone())))
                .collect();
            stack.added.push((
                horizon_protocol::item::component::CHARGED_PROJECTILES,
                horizon_protocol::item::Component::Stacks(charged),
            ));
        }
        // What a bundle holds, for its tooltip (ClientBundleTooltip).
        if !d.bundle_contents.is_empty() {
            let contents = d
                .bundle_contents
                .iter()
                .filter_map(|c| network_stack(item_id, synced_id, &Some(c.clone())))
                .collect();
            stack.added.push((
                horizon_protocol::item::component::BUNDLE_CONTENTS,
                horizon_protocol::item::Component::Stacks(contents),
            ));
        }
        // The books a lectern shows and a player reads.
        if !d.writable_pages.is_empty() {
            stack.added.push((
                horizon_protocol::item::component::WRITABLE_BOOK_CONTENT,
                horizon_protocol::item::Component::WritableBook(d.writable_pages.clone()),
            ));
        }
        if let Some(w) = &d.written {
            stack.added.push((
                horizon_protocol::item::component::WRITTEN_BOOK_CONTENT,
                horizon_protocol::item::Component::WrittenBook(
                    horizon_protocol::item::WrittenBook {
                        title: w.title.clone(),
                        author: w.author.clone(),
                        generation: i32::from(w.generation),
                        pages: if w.page_tags.len() == w.pages.len() {
                            w.page_tags.clone()
                        } else {
                            w.pages
                                .iter()
                                .map(|p| horizon_protocol::text::literal(p))
                                .collect()
                        },
                        resolved: true,
                    },
                ),
            ));
        }
    }
    if let Some(name) = s.custom_name() {
        let text = s
            .custom_name_rich()
            .cloned()
            .unwrap_or_else(|| horizon_protocol::text::literal(name));
        stack.added.push((
            horizon_protocol::item::component::CUSTOM_NAME,
            horizon_protocol::item::Component::Text(text),
        ));
    }
    // Components Horizon keeps as loaded that clients show (a plugin's menu
    // items: lore, glint, models, hidden tooltips), as vanilla syncs them.
    if let Some(d) = s.data() {
        use horizon_protocol::item::{Component as P, component as id};
        use horizon_world::nbt::Tag;
        if let Some(Tag::List(lines)) = d.other("minecraft:lore") {
            stack.added.push((id::LORE, P::Lore(lines.clone())));
        }
        if d.other("minecraft:unbreakable").is_some() {
            stack.added.push((id::UNBREAKABLE, P::Unit));
        }
        if let Some(Tag::Byte(b)) = d.other("minecraft:enchantment_glint_override") {
            stack
                .added
                .push((id::ENCHANTMENT_GLINT_OVERRIDE, P::Bool(*b != 0)));
        }
        if let Some(Tag::Int(v)) = d.other("minecraft:max_stack_size") {
            stack.added.push((id::MAX_STACK_SIZE, P::Int(*v)));
        }
        if let Some(Tag::Int(v)) = d.other("minecraft:max_damage") {
            stack.added.push((id::MAX_DAMAGE, P::Int(*v)));
        }
        if let Some(c @ Tag::Compound(_)) = d.other("minecraft:custom_data") {
            stack.added.push((id::CUSTOM_DATA, P::Nbt(c.clone())));
        }
        if let Some(c) = d.other("minecraft:custom_model_data") {
            let list = |k: &str| match c.get(k) {
                Some(Tag::List(l)) => l.clone(),
                _ => Vec::new(),
            };
            stack.added.push((
                id::CUSTOM_MODEL_DATA,
                P::CustomModelData {
                    floats: list("floats")
                        .iter()
                        .filter_map(|t| match t {
                            Tag::Float(v) => Some(f32::from_bits(*v)),
                            Tag::Double(v) => Some(f64::from_bits(*v) as f32),
                            Tag::Int(v) => Some(*v as f32),
                            _ => None,
                        })
                        .collect(),
                    flags: list("flags")
                        .iter()
                        .filter_map(|t| match t {
                            Tag::Byte(b) => Some(*b != 0),
                            _ => None,
                        })
                        .collect(),
                    strings: list("strings")
                        .iter()
                        .filter_map(|t| match t {
                            Tag::String(s) => Some(s.to_string_lossy()),
                            _ => None,
                        })
                        .collect(),
                    colors: list("colors")
                        .iter()
                        .filter_map(|t| match t {
                            Tag::Int(v) => Some(*v),
                            _ => None,
                        })
                        .collect(),
                },
            ));
        }
        if let Some(c) = d.other("minecraft:tooltip_display") {
            let hide = matches!(c.get("hide_tooltip"), Some(Tag::Byte(1)));
            let hidden = match c.get("hidden_components") {
                Some(Tag::List(l)) => l
                    .iter()
                    .filter_map(|t| match t {
                        Tag::String(s) => {
                            synced_id("minecraft:data_component_type", &s.to_string_lossy())
                        }
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            stack
                .added
                .push((id::TOOLTIP_DISPLAY, P::TooltipDisplay { hide, hidden }));
        }
    }
    // The component as loaded, so a translatable name stays one.
    if let Some(name) = s.data().and_then(|d| d.other("minecraft:item_name")) {
        stack.added.push((
            horizon_protocol::item::component::ITEM_NAME,
            horizon_protocol::item::Component::Text(name.clone()),
        ));
    }
    Some(stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::registry::Ids;

    #[test]
    fn merchant_water_cost_survives_the_offer_packet() {
        use horizon_protocol::{
            buf::{Codec, Reader, Writer},
            packets::play::{NetOffer, Offers},
        };
        let mut components = vec![String::new(); component::POTION_CONTENTS as usize + 1];
        components[component::POTION_CONTENTS as usize] = "minecraft:potion_contents".into();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".into(),
                    Ids::from_names(vec!["minecraft:potion".into(), "minecraft:emerald".into()]),
                ),
                (
                    "minecraft:potion".into(),
                    Ids::from_names(vec!["minecraft:water".into()]),
                ),
                (
                    "minecraft:data_component_type".into(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let dynamic = Dynamic::default();
        let water = ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water");
        let wire = super::stack_to_network(
            |name| builtin.get("minecraft:item").unwrap().id(name).unwrap(),
            |registry, name| builtin.get(registry)?.id(name),
            &Some(water.clone()),
        )
        .unwrap();
        let offers = vec![NetOffer {
            cost_a: wire.into(),
            result: Stack::new(1, 1),
            cost_b: Some(Stack::new(1, 2).into()),
            out_of_stock: false,
            uses: 0,
            max_uses: 2,
            xp: 0,
            special_price: 0,
            price_multiplier: 0.05,
            demand: 0,
        }];
        let mut w = Writer::new();
        Offers::write(&offers, &mut w);
        let mut r = Reader::new(&w.data);
        let decoded = Offers::read(&mut r).unwrap();
        assert_eq!(r.remaining(), 0);
        let cost = crate::merchant::ItemCost {
            stack: from_network(&decoded[0].cost_a.clone().into(), &builtin, &dynamic),
        };
        assert_eq!(cost.stack, water);
        assert_eq!(cost.stack.potion(), Some("minecraft:water"));
        let authoritative = crate::merchant::ItemCost {
            stack: water.clone(),
        };
        for payment in [
            water,
            ItemStack::new("minecraft:potion", 1).with_potion("minecraft:swiftness"),
            ItemStack::new("minecraft:potion", 1),
        ] {
            assert_eq!(cost.test(&payment), authoritative.test(&payment));
        }
        assert!(cost.test(&ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water")));
        assert!(
            !cost.test(&ItemStack::new("minecraft:potion", 1).with_potion("minecraft:swiftness"))
        );
        let emerald = crate::merchant::ItemCost {
            stack: from_network(
                &decoded[0].cost_b.clone().unwrap().into(),
                &builtin,
                &dynamic,
            ),
        };
        assert!(emerald.test(&ItemStack::new("minecraft:emerald", 2)));
    }

    /// A bundle's contents cross the wire, so a server's client can draw
    /// its tooltip (BundleContents' stream codec: a list of templates).
    #[test]
    fn a_bundle_keeps_its_contents_across_the_wire() {
        let mut components = vec![String::new(); 60];
        components[component::BUNDLE_CONTENTS as usize] = "minecraft:bundle_contents".into();
        let items = ["minecraft:air", "minecraft:bundle", "minecraft:stone"];
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(items.iter().map(|s| s.to_string()).collect()),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let dynamic = Dynamic {
            registries: Default::default(),
        };
        let mut bundle = ItemStack::new("minecraft:bundle", 1);
        bundle.data_mut().bundle_contents = vec![ItemStack::new("minecraft:stone", 32)];
        let wire = super::stack_to_network(
            |name| items.iter().position(|i| *i == name).unwrap_or(0) as i32,
            |_, _| None,
            &Some(bundle),
        )
        .unwrap();
        let back = from_network(&wire, &builtin, &dynamic);
        let contents = &back.data().expect("components").bundle_contents;
        assert_eq!(contents.len(), 1);
        assert_eq!(
            (&*contents[0].item, contents[0].count),
            ("minecraft:stone", 32)
        );
    }

    /// What a creative client sends (`stack_to_network`) comes back whole on
    /// the server: damage, both enchantment lists, repair cost, name, trim.
    #[test]
    fn a_creative_stack_keeps_its_components_across_the_wire() {
        let mut components = vec![String::new(); 60];
        for (id, name) in [
            (component::DAMAGE, "minecraft:damage"),
            (component::CUSTOM_NAME, "minecraft:custom_name"),
            (component::ITEM_NAME, "minecraft:item_name"),
            (component::ENCHANTMENTS, "minecraft:enchantments"),
            (component::POTION_CONTENTS, "minecraft:potion_contents"),
            (component::REPAIR_COST, "minecraft:repair_cost"),
            (
                component::STORED_ENCHANTMENTS,
                "minecraft:stored_enchantments",
            ),
            (component::TRIM, "minecraft:trim"),
        ] {
            components[id as usize] = name.to_owned();
        }
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(names(&[
                        "minecraft:air",
                        "minecraft:diamond_chestplate",
                        "minecraft:splash_potion",
                    ])),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
                (
                    "minecraft:potion".to_owned(),
                    Ids::from_names(names(&["minecraft:water", "minecraft:swiftness"])),
                ),
            ]
            .into(),
        };
        let synced = |n: &[&str]| n.iter().map(|s| (s.to_string(), None)).collect::<Vec<_>>();
        let dynamic = Dynamic {
            registries: [
                (
                    "minecraft:enchantment".to_owned(),
                    synced(&["minecraft:protection", "minecraft:sharpness"]),
                ),
                (
                    "minecraft:trim_material".to_owned(),
                    synced(&["minecraft:gold"]),
                ),
                (
                    "minecraft:trim_pattern".to_owned(),
                    synced(&["minecraft:sentry"]),
                ),
            ]
            .into(),
        };
        let mut stack = ItemStack::new("minecraft:diamond_chestplate", 1);
        stack.damage = 5;
        stack.enchant("minecraft:protection", 4);
        let d = stack.data_mut();
        d.stored_enchantments
            .push((Arc::from("minecraft:sharpness"), 2));
        d.repair_cost = 9;
        d.custom_name = Some("Keeper".to_owned());
        // A translatable item name reaches the client as the same component.
        let item_name = Tag::compound([("translate", Tag::String("filled_map.monument".into()))]);
        d.other
            .push(("minecraft:item_name".into(), item_name.clone()));
        d.trim = Some((Arc::from("minecraft:gold"), Arc::from("minecraft:sentry")));
        let id = |registry: &str, name: &str| {
            builtin
                .get(registry)
                .and_then(|ids| ids.id(name))
                .or_else(|| dynamic.id(registry, name))
        };
        let wire = super::stack_to_network(
            |name| {
                builtin
                    .get("minecraft:item")
                    .and_then(|i| i.id(name))
                    .unwrap_or(0)
            },
            id,
            &Some(stack.clone()),
        )
        .unwrap();
        let back = from_network(&wire, &builtin, &dynamic);
        assert_eq!(back.item, stack.item);
        assert_eq!(back.damage, 5);
        let d = back.data().expect("components");
        assert_eq!(d.enchantments, vec![(Arc::from("minecraft:protection"), 4)]);
        assert_eq!(
            d.stored_enchantments,
            vec![(Arc::from("minecraft:sharpness"), 2)]
        );
        assert_eq!(d.repair_cost, 9);
        assert_eq!(d.custom_name.as_deref(), Some("Keeper"));
        assert_eq!(d.other("minecraft:item_name"), Some(&item_name));
        assert_eq!(
            d.trim,
            Some((Arc::from("minecraft:gold"), Arc::from("minecraft:sentry")))
        );
        // PotionContents: the potion is a built-in registry, as a hub's
        // `SyncData::id` treats it, so the id has to come from there.
        let potion = ItemStack::new("minecraft:splash_potion", 1).with_potion("swiftness");
        let wire = super::stack_to_network(
            |name| id("minecraft:item", name).unwrap_or(0),
            id,
            &Some(potion),
        )
        .unwrap();
        assert_eq!(
            from_network(&wire, &builtin, &dynamic).potion(),
            Some("minecraft:swiftness")
        );
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use horizon_protocol::item::{Component, component};

    #[test]
    fn potions_go_as_potion_registry_ids() {
        let synced_id = |registry: &str, name: &str| match (registry, name) {
            ("minecraft:potion", "minecraft:swiftness") => Some(14),
            _ => None,
        };
        let potion = ItemStack::new("minecraft:splash_potion", 1).with_potion("swiftness");
        let stack = stack_to_network(|_| 7, synced_id, &Some(potion)).unwrap();
        assert_eq!(
            stack.component(component::POTION_CONTENTS),
            Some(&Component::Potion {
                potion: Some(14),
                color: None,
                rest: Vec::new(),
            })
        );
    }

    #[test]
    fn trims_go_as_synced_registry_references() {
        let mut chestplate = ItemStack::new("minecraft:diamond_chestplate", 1);
        chestplate.data_mut().trim = Some(("minecraft:gold".into(), "coast".into()));
        let synced_id = |registry: &str, name: &str| match (registry, name) {
            ("minecraft:trim_material", "minecraft:gold") => Some(4),
            ("minecraft:trim_pattern", "minecraft:coast") => Some(1),
            _ => None,
        };
        let stack = stack_to_network(|_| 7, synced_id, &Some(chestplate.clone())).unwrap();
        assert_eq!(
            stack.component(component::TRIM),
            Some(&Component::Trim {
                material: 4,
                pattern: 1
            })
        );
        // A name the connection's registry lacks sends no trim.
        chestplate.data_mut().trim = Some(("minecraft:x".into(), "coast".into()));
        let stack = stack_to_network(|_| 7, synced_id, &Some(chestplate)).unwrap();
        assert_eq!(stack.component(component::TRIM), None);
    }
}
