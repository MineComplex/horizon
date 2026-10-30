//! Sections and chunk NBT against `testdata/chunk_oracle.txt`, printed by a Java oracle (outside Git:
//! the public-tree check refuses Java files) running the real 26.3 classes (vanilla's
//! `LevelChunkSection`, the containers' codecs and
//! `SerializableChunkData`). Block states are named, so a small table with the oracle's blocks
//! (same properties and value orders) stands in for the report; the network bytes, which carry
//! registry ids, are checked when `HORIZON_SERVER_BLOCKS_JSON` names the real `blocks.json`.

use std::io::Cursor;

use super::section::{ContainerFactory, LevelChunkSection, SectionFlags, StateFlags};
use super::serial::{BiomeIds, ChunkCodec, ChunkData, ChunkStatus, RawChunkParts, SectionData};
use crate::block::{BlockState, BlockStates, BlockStatesBuilder};
use crate::core::position::ChunkPos;
use crate::nbt::{Compound, JavaString, Tag, parse_snbt};

const ORACLE: &str = include_str!("testdata/chunk_oracle.txt");

fn values(range: std::ops::RangeInclusive<u32>) -> Vec<String> {
    range.map(|v| v.to_string()).collect()
}

/// The oracle's blocks with vanilla's properties, value orders and defaults.
fn table() -> BlockStates {
    let (b, s) = (&["true", "false"][..], values(0..=15));
    let level: Vec<&str> = s.iter().map(String::as_str).collect();
    let side = &["up", "side", "none"][..];
    let mut t = BlockStatesBuilder::new();
    let mut add = |name, props: &[(&str, &[&str])], default: &[(&str, &str)]| {
        let block = t.add_block(name, props).unwrap();
        let state = t
            .table()
            .state_with(block, default.iter().copied())
            .unwrap();
        t.set_default(block, state).unwrap();
    };
    add("minecraft:air", &[], &[]);
    add("minecraft:stone", &[], &[]);
    add("minecraft:dirt", &[], &[]);
    add(
        "minecraft:grass_block",
        &[("snowy", b)],
        &[("snowy", "false")],
    );
    add("minecraft:water", &[("level", &level)], &[]);
    add("minecraft:lava", &[("level", &level)], &[]);
    add("minecraft:wheat", &[("age", &level[..8])], &[]);
    let facing = &["north", "south", "west", "east"][..];
    let shape = &[
        "straight",
        "inner_left",
        "inner_right",
        "outer_left",
        "outer_right",
    ][..];
    let stairs: &[(&str, &[&str])] = &[
        ("facing", facing),
        ("half", &["top", "bottom"]),
        ("shape", shape),
        ("waterlogged", b),
    ];
    let stairs_default = [("half", "bottom"), ("waterlogged", "false")];
    add("minecraft:oak_stairs", stairs, &stairs_default);
    let leaves: &[(&str, &[&str])] = &[
        ("distance", &level[1..8]),
        ("persistent", b),
        ("waterlogged", b),
    ];
    let leaves_default = [
        ("distance", "7"),
        ("persistent", "false"),
        ("waterlogged", "false"),
    ];
    add("minecraft:oak_leaves", leaves, &leaves_default);
    let wire: &[(&str, &[&str])] = &[
        ("east", side),
        ("north", side),
        ("power", &level),
        ("south", side),
        ("west", side),
    ];
    let none = [
        ("east", "none"),
        ("north", "none"),
        ("south", "none"),
        ("west", "none"),
    ];
    add("minecraft:redstone_wire", wire, &none);
    t.finish()
}

/// The oracle's flags (it printed them per state): air; wheat below age 7, grass and leaves at
/// distance 7 not persistent tick; water, lava and waterlogged blocks have a fluid; lava's ticks.
struct Flags<'a>(&'a BlockStates);
impl SectionFlags for Flags<'_> {
    fn section_flags(&self, state: BlockState) -> StateFlags {
        let t = self.0;
        let name = t.block_name(t.block(state));
        let value = |p| t.value(state, p);
        let mut f = 0;
        match name {
            "minecraft:air" => f |= StateFlags::AIR,
            "minecraft:wheat" if value("age") != Some("7") => f |= StateFlags::RANDOMLY_TICKING,
            "minecraft:grass_block" => f |= StateFlags::RANDOMLY_TICKING,
            "minecraft:water" => f |= StateFlags::HAS_FLUID,
            "minecraft:lava" => {
                f |= StateFlags::RANDOMLY_TICKING
                    | StateFlags::HAS_FLUID
                    | StateFlags::FLUID_RANDOMLY_TICKING
            }
            "minecraft:oak_stairs" => f |= StateFlags::SPECIAL_COLLIDING,
            _ => {}
        }
        if name == "minecraft:oak_leaves"
            && value("distance") == Some("7")
            && value("persistent") == Some("false")
        {
            f |= StateFlags::RANDOMLY_TICKING;
        }
        if value("waterlogged") == Some("true") {
            f |= StateFlags::HAS_FLUID;
        }
        StateFlags(f)
    }
}

fn biome_ids() -> BiomeIds {
    let mut names: Vec<String> = ["plains", "desert", "forest", "river"]
        .map(|n| format!("minecraft:{n}"))
        .into();
    names.extend((4..70).map(|i| format!("test:biome_{i}")));
    BiomeIds::new(names)
}

/// `String.hashCode`.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}
fn state_name(t: &BlockStates, s: BlockState) -> String {
    let mut name = t.block_name(t.block(s)).to_owned();
    let props: Vec<String> = t.values(s).map(|(k, v)| format!("{k}={v}")).collect();
    if !props.is_empty() {
        name += &format!("[{}]", props.join(","));
    }
    name
}
fn states_hash(t: &BlockStates, s: &LevelChunkSection) -> i64 {
    (0..4096).fold(0i64, |h, i| {
        h.wrapping_mul(31)
            .wrapping_add(i64::from(java_hash(&state_name(t, s.states().get(i)))))
    })
}
fn biomes_hash(b: &BiomeIds, c: &super::PalettedContainer<u32>) -> i64 {
    (0..64).fold(0i64, |h, i| {
        h.wrapping_mul(31)
            .wrapping_add(i64::from(java_hash(b.name(c.get(i)))))
    })
}
/// The oracle's `th`: a hash of a tag tree with compound keys in Java's order.
fn tag_hash(tag: &Tag) -> i64 {
    let mut h = i64::from(tag.id());
    let mut add = |v: i64| h = h.wrapping_mul(31).wrapping_add(v);
    match tag {
        Tag::Compound(map) => {
            for (k, v) in map {
                add(i64::from(java_hash(&k.to_string_lossy())));
                add(tag_hash(v));
            }
        }
        Tag::List(list) => {
            add(list.len() as i64);
            list.iter().for_each(|t| add(tag_hash(t)));
        }
        Tag::ByteArray(a) => a.iter().for_each(|&b| add(i64::from(b as i8))),
        Tag::IntArray(a) => a.iter().for_each(|&v| add(v.into())),
        Tag::LongArray(a) => a.iter().for_each(|&v| add(v)),
        Tag::String(s) => add(java_hash(&s.to_string_lossy()).into()),
        Tag::Byte(v) => add((*v).into()),
        Tag::Short(v) => add((*v).into()),
        Tag::Int(v) => add((*v).into()),
        Tag::Long(v) => add(*v),
        Tag::Float(v) => add(i64::from(*v as i32)),
        Tag::Double(v) => add(*v as i64),
        Tag::End => {}
    }
    h
}

fn oracle(prefix: &str) -> impl Iterator<Item = &'static str> + use<'_> {
    ORACLE.lines().filter_map(move |l| l.strip_prefix(prefix))
}

fn codec<'a>(t: &'a BlockStates, f: &'a Flags<'a>, b: &'a BiomeIds) -> ChunkCodec<'a, Flags<'a>> {
    ChunkCodec {
        blocks: t,
        flags: f,
        biomes: b,
        factory: ContainerFactory::new(t.state_count(), b.len(), 0),
        min_section_y: -4,
        max_section_y: -1,
    }
}

#[test]
fn containers_decode_like_vanilla_codecs() {
    let (t, b) = (table(), biome_ids());
    let f = Flags(&t);
    let c = codec(&t, &f, &b);
    let mut checked = 0;
    for line in oracle("case\t") {
        let [kind, input, status, hash, message]: [&str; 5] =
            line.split('\t').collect::<Vec<_>>().try_into().unwrap();
        let tag = parse_snbt(input).unwrap().0;
        let (got_status, got_hash, got_message) = if kind == "blocks" {
            match c.decode_block_states(&tag) {
                Ok((container, partial)) => {
                    let section =
                        LevelChunkSection::new(container, c.factory.create_for_biomes(), &f);
                    let status = if partial.is_some() { "partial" } else { "ok" };
                    (
                        status,
                        states_hash(&t, &section).to_string(),
                        partial.unwrap_or_default(),
                    )
                }
                Err(e) => ("error", "0".into(), e),
            }
        } else {
            match c.decode_biomes(&tag) {
                Ok((container, partial)) => {
                    let status = if partial.is_some() { "partial" } else { "ok" };
                    (
                        status,
                        biomes_hash(&b, &container).to_string(),
                        partial.unwrap_or_default(),
                    )
                }
                Err(e) => ("error", "0".into(), e),
            }
        };
        if hash.starts_with("throws:") {
            // `{palette:[]}`: vanilla loads it and throws on first use; Leaf's section
            // constructor uses it at once, so the chunk fails to load either way.
            assert_eq!(got_status, "error", "{input}");
            continue;
        }
        assert_eq!(
            (got_status, got_hash.as_str(), got_message.as_str()),
            (status, hash, message),
            "{input}"
        );
        checked += 1;
    }
    assert_eq!(checked, 32);
}

/// The oracle's five sections, built the same way (values in index order).
fn oracle_sections(t: &BlockStates, b: &BiomeIds, f: &Flags) -> Vec<LevelChunkSection> {
    let states = |name| t.states(t.block_by_name(name).unwrap()).collect::<Vec<_>>();
    let default = |name| t.default_state(t.block_by_name(name).unwrap());
    let (wire, leaves, wheat, stairs, water) = (
        states("minecraft:redstone_wire"),
        states("minecraft:oak_leaves"),
        states("minecraft:wheat"),
        states("minecraft:oak_stairs"),
        states("minecraft:water"),
    );
    let factory = ContainerFactory::new(t.state_count(), b.len(), 0);
    (0..5)
        .map(|s| {
            let mut section = LevelChunkSection::empty(&factory);
            for i in 0..4096u32 {
                let (x, z, y) = (i & 15, (i >> 4) & 15, i >> 8);
                let i = i as usize;
                let state = match s {
                    0 => default("minecraft:stone"),
                    _ if i == 0 => BlockState::AIR,
                    1 if y < 4 => default("minecraft:stone"),
                    1 if y < 8 => water[((x + z) & 15) as usize],
                    1 if y < 10 => wheat[i % wheat.len()],
                    1 if y < 12 => default("minecraft:grass_block"),
                    2 => wire[(i * 7) % 300],
                    3 if i.is_multiple_of(5) => leaves[i % leaves.len()],
                    3 if i % 5 == 1 => stairs[i % stairs.len()],
                    3 => default("minecraft:lava"),
                    _ => BlockState::AIR,
                };
                section.set_block_state(x, y, z, state, f);
            }
            for i in 0..64u32 {
                let biome = match s {
                    1 => i % 3,
                    2 => i % 20,
                    0 => 0,
                    _ => 1,
                };
                section.set_noise_biome(i & 3, i >> 4, (i >> 2) & 3, biome);
            }
            section
        })
        .collect()
}

#[test]
fn sections_count_like_vanilla() {
    let (t, b) = (table(), biome_ids());
    let f = Flags(&t);
    let sections = oracle_sections(&t, &b, &f);
    assert_eq!(oracle("section ").count(), sections.len());
    for (line, section) in oracle("section ").zip(&sections) {
        let w: Vec<&str> = line.split(' ').collect();
        let head = w[w.len() - 1];
        let counts = (
            u16::from_str_radix(&head[0..4], 16).unwrap(),
            u16::from_str_radix(&head[4..8], 16).unwrap(),
        );
        assert_eq!(
            (section.non_empty_block_count(), section.fluid_count()),
            counts,
            "{line}"
        );
        let flags = (
            section.has_only_air(),
            section.is_randomly_ticking_blocks().to_string(),
            section.is_randomly_ticking_fluids().to_string(),
            section.has_fluid().to_string(),
        );
        assert_eq!(
            flags,
            (w[2] == "1", w[3].into(), w[4].into(), w[5].into()),
            "{line}"
        );
        assert_eq!(states_hash(&t, section).to_string(), w[9], "{line}");
        assert_eq!(
            biomes_hash(&b, section.biomes()).to_string(),
            w[10],
            "{line}"
        );
        // Recounting gives the same counts, and the incremental list holds the same positions.
        let mut recounted = section.clone();
        recounted.recalc_block_counts(&f);
        assert_eq!(
            recounted.non_empty_block_count(),
            section.non_empty_block_count()
        );
        assert_eq!(
            recounted.is_randomly_ticking(),
            section.is_randomly_ticking()
        );
        assert_eq!(
            recounted.has_special_colliding_blocks(),
            section.has_special_colliding_blocks()
        );
        let (mut a, mut b) = (
            recounted.ticking_blocks().to_vec(),
            section.ticking_blocks().to_vec(),
        );
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    assert!(
        sections[3].has_special_colliding_blocks() && !sections[2].has_special_colliding_blocks()
    );
}

#[test]
fn ticking_list_follows_moonrise() {
    let t = table();
    let f = Flags(&t);
    let factory = ContainerFactory::new(t.state_count(), 1, 0);
    let mut section = LevelChunkSection::empty(&factory);
    let grass = t.default_state(t.block_by_name("minecraft:grass_block").unwrap());
    let stone = t.default_state(t.block_by_name("minecraft:stone").unwrap());
    for x in 0..4 {
        section.set_block_state(x, 1, 2, grass, &f);
    }
    assert_eq!(section.ticking_blocks(), &[0x120, 0x121, 0x122, 0x123]);
    // Removal moves the last entry into the hole (Moonrise's `ShortList.remove`).
    assert_eq!(section.set_block_state(1, 1, 2, stone, &f), grass);
    assert_eq!(section.ticking_blocks(), &[0x120, 0x123, 0x122]);
    section.set_block_state(1, 1, 2, grass, &f);
    assert_eq!(section.ticking_blocks(), &[0x120, 0x123, 0x122, 0x121]);
    // Recounting rebuilds the list grouped by palette entry, in index order.
    section.recalc_block_counts(&f);
    assert_eq!(section.ticking_blocks(), &[0x120, 0x121, 0x122, 0x123]);
    assert_eq!(section.non_empty_block_count(), 4);
    assert!(section.is_randomly_ticking() && !section.has_fluid());
}

#[test]
fn network_form_round_trips() {
    let (t, b) = (table(), biome_ids());
    let f = Flags(&t);
    let factory = ContainerFactory::new(t.state_count(), b.len(), 0);
    for section in oracle_sections(&t, &b, &f) {
        let mut bytes = Vec::new();
        section.write(&mut bytes);
        assert_eq!(bytes.len(), section.serialized_size());
        let mut read = LevelChunkSection::empty(&factory);
        read.read(&mut Cursor::new(&bytes[..])).unwrap();
        assert_eq!(read.states(), section.states());
        assert_eq!(read.biomes(), section.biomes());
        assert_eq!(
            read.non_empty_block_count(),
            section.non_empty_block_count()
        );
        assert_eq!(read.fluid_count(), section.fluid_count());
    }
}

/// With the real report, the sections' bytes are vanilla's (`Arrays.hashCode` of them).
#[test]
fn network_bytes_match_vanilla_with_the_real_report() {
    let Some(path) = std::env::var_os("HORIZON_SERVER_BLOCKS_JSON") else {
        return;
    };
    let t = BlockStates::load(std::path::Path::new(&path)).unwrap();
    let b = biome_ids();
    let f = Flags(&t);
    assert_eq!(oracle("section ").count(), 5);
    for (line, section) in oracle("section ").zip(oracle_sections(&t, &b, &f)) {
        let w: Vec<&str> = line.split(' ').collect();
        let mut bytes = Vec::new();
        section.write(&mut bytes);
        let hash = bytes.iter().fold(1i32, |h, &x| {
            h.wrapping_mul(31).wrapping_add(i32::from(x as i8))
        });
        assert_eq!(
            (hash.to_string(), bytes.len().to_string()),
            (w[12].into(), w[13].into()),
            "{line}"
        );
        assert_eq!(section.serialized_size().to_string(), w[7]);
    }
}

fn light(f: impl Fn(usize) -> u8) -> Box<[u8; 2048]> {
    Box::new(std::array::from_fn(f))
}
fn snbt(text: &str) -> Compound {
    match parse_snbt(text).unwrap().0 {
        Tag::Compound(c) => c,
        _ => unreachable!(),
    }
}
fn key(k: &str) -> JavaString {
    k.into()
}

/// The oracle's chunk, built as it built its `SerializableChunkData`.
fn oracle_chunk(t: &BlockStates, b: &BiomeIds, f: &Flags, status: ChunkStatus) -> ChunkData {
    let pattern = light(|i| (i * 37) as u8);
    let mut sections = vec![SectionData {
        y: -5,
        section: None,
        block_light: None,
        sky_light: Some(pattern.clone()),
        block_light_state: None,
        sky_light_state: None,
    }];
    for (i, section) in oracle_sections(t, b, f).into_iter().enumerate() {
        sections.push(SectionData {
            y: i as i32 - 4,
            section: Some(section),
            block_light: (i == 1).then(|| pattern.clone()),
            sky_light: (i < 3).then(|| light(|_| 0xff)),
            block_light_state: None,
            sky_light_state: None,
        });
    }
    let mut heightmaps: [Option<Vec<i64>>; 6] = Default::default();
    heightmaps[4] = Some(vec![1, 2, -3]);
    heightmaps[1] = Some(vec![0; 37]);
    ChunkData {
        pos: ChunkPos::new(-3, 7),
        min_section_y: -4,
        last_update: 1234,
        inhabited_time: 99,
        status,
        light_correct: true,
        heightmaps,
        sections,
        raw: RawChunkParts {
            post_processing: Some(parse_snbt("[[],[1s,2s,300s]]").unwrap().0),
            entities: vec![snbt("{id:\"minecraft:pig\"}")],
            block_entities: vec![snbt(
                "{id:\"minecraft:chest\",x:3,y:-60,z:5,keepPacked:0b,Items:[]}",
            )],
            structures: Some(snbt(
                "{starts:{},References:{\"minecraft:village_plains\":[L;12L]}}",
            )),
            ..Default::default()
        },
    }
}

#[test]
fn chunk_nbt_matches_serializable_chunk_data() {
    let (t, b) = (table(), biome_ids());
    let f = Flags(&t);
    let c = codec(&t, &f, &b);
    let chunks: Vec<&str> = oracle("chunk ").collect();
    let parsed: Vec<&str> = oracle("parsed ").collect();
    for (i, status) in [ChunkStatus::Full, ChunkStatus::Features]
        .into_iter()
        .enumerate()
    {
        let chunk = oracle_chunk(&t, &b, &f, status);
        let written = c.write(&chunk);
        // Moonrise's light tags, then vanilla's form for the comparison.
        let mut vanilla = written.clone();
        if status == ChunkStatus::Full {
            assert_eq!(
                vanilla.remove(&key("starlight.light_version")),
                Some(Tag::Int(10))
            );
            assert_eq!(
                vanilla.insert(key("isLightOn"), Tag::Byte(1)),
                Some(Tag::Byte(0))
            );
        } else {
            assert!(!vanilla.contains_key(&key("starlight.light_version")));
        }
        let w: Vec<&str> = chunks[i].split(' ').collect();
        assert_eq!(w[0], format!("minecraft:{}", status.name()));
        assert_eq!(
            tag_hash(&Tag::Compound(vanilla)).to_string(),
            w[1],
            "{status:?}"
        );

        // Read back on a level of sections -4..=-1: section 0 is light-only.
        let back = c.read(&written).unwrap().unwrap();
        let summary: Vec<String> = back
            .sections
            .iter()
            .map(|s| {
                let hash = s
                    .section
                    .as_ref()
                    .map_or("none".into(), |s| states_hash(&t, s).to_string());
                format!(
                    "{}:{hash}:{}:{}",
                    s.y,
                    s.block_light.is_some(),
                    s.sky_light.is_some()
                )
            })
            .collect();
        let expected = parsed[i].split(" sections ").nth(1).unwrap().trim_end();
        assert_eq!(summary.join(" "), expected);
        assert_eq!(
            (back.pos, back.status, back.raw.entities.len()),
            (chunk.pos, status, i)
        );
        // Moonrise: lit only at `light` or later with its version tag (vanilla reads `true` for
        // the `features` chunk).
        assert_eq!(back.light_correct, status == ChunkStatus::Full);
        assert_eq!(c.write(&back), written_for_reread(&c, &chunk));
    }
}

/// What `read` keeps of a written chunk: sections out of the level lose their blocks.
fn written_for_reread(c: &ChunkCodec<Flags>, chunk: &ChunkData) -> Compound {
    let mut chunk = chunk.clone();
    for s in &mut chunk.sections {
        if s.y > c.max_section_y {
            s.section = None;
        }
    }
    chunk.light_correct = chunk.status == ChunkStatus::Full;
    c.write(&chunk)
}

#[test]
fn chunk_read_edges() {
    let (t, b) = (table(), biome_ids());
    let f = Flags(&t);
    let c = codec(&t, &f, &b);
    assert_eq!(c.read(&snbt("{xPos:1}")).unwrap(), None);
    let newer = c
        .read(&snbt("{Status:\"full\",DataVersion:5024}"))
        .unwrap_err();
    assert_eq!(
        newer.to_string(),
        "Server attempted to load chunk saved with newer version of minecraft! 5024 > 5023"
    );
    let chunk = c
        .read(&snbt(
            "{Status:\"nope\",xPos:2.7d,zPos:-1L,isLightOn:1b,\"starlight.light_version\":10,\
             sections:[{Y:-2b,\"starlight.blocklight_state\":2,\"starlight.skylight_state\":\"x\"},5]}",
        ))
        .unwrap()
        .unwrap();
    assert_eq!(
        (chunk.pos, chunk.status, chunk.light_correct),
        (ChunkPos::new(2, -1), ChunkStatus::Empty, false)
    );
    let s = &chunk.sections[0];
    assert_eq!(
        (s.y, s.block_light_state, s.sky_light_state),
        (-2, Some(2), Some(0))
    );
    assert!(s.section.as_ref().unwrap().has_only_air());
    let short = c.read(&snbt(
        "{Status:\"full\",sections:[{Y:-1b,SkyLight:[B;1B]}]}",
    ));
    assert_eq!(
        short.unwrap_err().to_string(),
        "DataLayer should be 2048 bytes not: 1"
    );
    let bad = c.read(&snbt(
        "{Status:\"full\",sections:[{Y:-1b,block_states:{}}]}",
    ));
    assert_eq!(
        bad.unwrap_err().to_string(),
        "No key palette in MapLike[{}]"
    );
    // Raw parts come back unchanged; an empty chunk gets vanilla's empty lists and structures.
    let raw = "{Status:\"minecraft:spawn\",block_ticks:[{i:\"minecraft:stone\",t:3}],\
               ChunkBukkitValues:{a:1},\"Paper.FailedSpawnAttempts\":{monster:4L},UpgradeData:{x:1}}";
    let chunk = c.read(&snbt(raw)).unwrap().unwrap();
    let written = c.write(&chunk);
    for k in [
        "block_ticks",
        "ChunkBukkitValues",
        "Paper.FailedSpawnAttempts",
        "UpgradeData",
    ] {
        assert_eq!(written.get(&key(k)), snbt(raw).get(&key(k)), "{k}");
    }
    assert_eq!(written[&key("fluid_ticks")], Tag::List(vec![]));
    assert_eq!(written[&key("entities")], Tag::List(vec![]));
    assert_eq!(
        written[&key("structures")].to_string(),
        "{References:{},starts:{}}"
    );
}

#[test]
fn tags_print_as_string_tag_visitor() {
    let tag = parse_snbt("{b:1b,\"a b\":[1.5f,2.0d],c:'say \"hi\"',d:[L;1L],\"true\":\"\\n\"}")
        .unwrap()
        .0;
    assert_eq!(
        tag.to_string(),
        "{\"a b\":[1.5f,2.0d],b:1b,c:'say \"hi\"',d:[L;1L],\"true\":\"\\n\"}"
    );
}
