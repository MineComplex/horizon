//! Expected bytes come from Java 25 running the 26.3 classes (`LeafConfigurationOracle.java` under
//! `mods/horizon/src/test/java/dev/horizonlab/leafserver/`): the packets' `STREAM_CODEC`s, with the
//! registries loaded as `WorldLoader` does from the vanilla pack plus Paper's `paper` pack.

use super::*;
use crate::network::login::tests::hex;
use crate::registry::tests::pack;
use crate::registry::{DataPack, parse_builtin};

fn with_id(id: u8, payload: &str) -> Vec<u8> {
    let mut out = vec![id];
    out.extend(hex(payload));
    out
}

const MOON: &str = r##"{"height": 16, "min_y": -64, "natural": true, "coordinate_scale": 1.0, "ambient_light": 0.5,
    "infiniburn": "#minecraft:infiniburn_overworld", "effects": [1, 2.5, "x"], "big": 5000000000, "tenth": 0.1}"##;

fn registries() -> Registries {
    let vanilla = pack(
        "vanilla",
        Some(KnownPack::vanilla("core")),
        &[
            ("minecraft/dimension_type/overworld.json", "{}"),
            (
                "minecraft/tags/dimension_type/all.json",
                r#"{"values": ["overworld", "custom:moon"]}"#,
            ),
        ],
    );
    let custom = pack(
        "file/custom",
        None,
        &[
            ("custom/dimension_type/moon.json", MOON),
            ("custom/tags/dimension_type/empty.json", r#"{"values": []}"#),
        ],
    );
    Registries::build(
        parse_builtin("{}").unwrap(),
        &[vanilla, custom, DataPack::paper()],
    )
    .unwrap()
}

fn features() -> Vec<String> {
    vec!["minecraft:vanilla".into()]
}

fn settings<'a>(
    registries: &'a Registries,
    features: &'a [String],
    coc: &'a [(String, String)],
) -> Settings<'a> {
    Settings {
        registries,
        brand: "Leaf",
        enabled_features: features,
        codes_of_conduct: coc,
    }
}

#[test]
fn packets_match_java() {
    let r = registries();
    assert_eq!(
        select_known_packs_packet(&r.known_packs),
        with_id(15, "01096d696e65637261667404636f72650432362e33")
    );
    assert_eq!(
        brand_packet("Leaf"),
        with_id(1, "0f6d696e6563726166743a6272616e64044c656166")
    );
    assert_eq!(
        update_enabled_features_packet(&features()),
        with_id(13, "01116d696e6563726166743a76616e696c6c61")
    );
    // Every flag: Java's `HashSet<Identifier>` order.
    let all: Vec<String> = [
        "vanilla",
        "trade_rebalance",
        "redstone_experiments",
        "minecart_improvements",
    ]
    .iter()
    .map(|f| format!("minecraft:{f}"))
    .collect();
    assert_eq!(
        update_enabled_features_packet(&all),
        with_id(
            13,
            "041e6d696e6563726166743a72656473746f6e655f6578706572696d656e7473116d696e6563726166743a76616e696c6c61\
             196d696e6563726166743a74726164655f726562616c616e63651f6d696e6563726166743a6d696e65636172745f696d70726f76656d656e7473"
        )
    );
    assert_eq!(finish_configuration_packet(), vec![3]);
    assert_eq!(
        keep_alive_packet(1_234_567_890_123),
        with_id(4, "0000011f71fb04cb")
    );
    assert_eq!(
        code_of_conduct_packet("Be nice"),
        with_id(20, "074265206e696365")
    );
    assert_eq!(
        disconnect_packet(&Fault("x".into()).reason()),
        with_id(
            2,
            "0a0900047769746808000000010015496e7465726e616c20457863657074696f6e3a20780800097472616e736c617465\
             0018646973636f6e6e6563742e67656e65726963526561736f6e00"
        )
    );
    assert_eq!(
        disconnect_packet(&Reason::Literal("Invalid custom payload payload!".into())),
        with_id(
            2,
            "08001f496e76616c696420637573746f6d207061796c6f6164207061796c6f616421"
        )
    );
}

#[test]
fn registry_data_with_and_without_known_packs() {
    let r = registries();
    let dims = r.synced("minecraft:dimension_type").unwrap();
    // custom:moon has no known pack: always sent with its data; minecraft:overworld only without
    // the negotiated core pack.
    let moon = "0b637573746f6d3a6d6f6f6e010a040003626967000000012a05f200090007656666656374730a000000030100000100\
        05000040200000000800000001780006000574656e74683fb999999999999a0100076e61747572616c0105000d616d6269656e\
        745f6c696768743f000000010010636f6f7264696e6174655f7363616c650108000a696e66696e696275726e001f236d696e65\
        6372616674\
        3a696e66696e696275726e5f6f766572776f726c640100056d696e5f79c00100066865696768741000";
    let header = "186d696e6563726166743a64696d656e73696f6e5f7479706502";
    assert_eq!(
        registry_data_packet(dims, &r.known_packs),
        with_id(
            7,
            &format!("{header}{moon}136d696e6563726166743a6f766572776f726c6400")
        )
    );
    assert_eq!(
        registry_data_packet(dims, &[]),
        with_id(
            7,
            &format!("{header}{moon}136d696e6563726166743a6f766572776f726c64010a00")
        )
    );
    // Paper's `paper:raw` with the vanilla chat types known, as Leaf sends it (ids only here:
    // the vanilla chat types come from the jar, so just paper's entry is built in this test).
    assert_eq!(
        registry_data_packet(r.synced("minecraft:chat_type").unwrap(), &r.known_packs),
        with_id(
            7,
            "136d696e6563726166743a636861745f7479706501\
             0970617065723a726177010a0a00046368617408000f7472616e736c6174696f6e5f6b65790002257309000a706172616d\
             657465727308000000010007636f6e74656e74000a00096e6172726174696f6e08000f7472616e736c6174696f6e5f6b6579\
             0002257309000a706172616d657465727308000000010007636f6e74656e740000"
        )
    );
}

#[test]
fn update_tags_matches_java() {
    // Tags `minecraft:all` = [overworld, custom:moon] = [1, 0] and an empty `custom:empty`, in
    // Java's `HashMap` order.
    assert_eq!(
        update_tags_packet(&registries()),
        with_id(
            14,
            "01186d696e6563726166743a64696d656e73696f6e5f74797065020c637573746f6d3a656d707479000d6d696e656372\
             6166743a616c6c020100"
        )
    );
    // Four tags of 26.3's timeline registry (nested `#universal`), Java's order and bytes.
    let timelines = pack(
        "vanilla",
        Some(KnownPack::vanilla("core")),
        &[
            ("minecraft/timeline/t0.json", "{}"),
            ("minecraft/timeline/t1.json", "{}"),
            ("minecraft/timeline/t2.json", "{}"),
            ("minecraft/timeline/t3.json", "{}"),
            (
                "minecraft/tags/timeline/universal.json",
                r#"{"values": ["t3"]}"#,
            ),
            (
                "minecraft/tags/timeline/in_end.json",
                r##"{"values": ["#universal"]}"##,
            ),
            (
                "minecraft/tags/timeline/in_nether.json",
                r##"{"values": ["#universal"]}"##,
            ),
            (
                "minecraft/tags/timeline/in_overworld.json",
                r##"{"values": ["#universal", "t0", "t2", "t1", "t3"]}"##,
            ),
        ],
    );
    let r = Registries::build(parse_builtin("{}").unwrap(), &[timelines]).unwrap();
    let mut expected = vec![14, 1];
    expected.extend(b"\x12minecraft:timeline");
    expected.extend(hex(
        "04136d696e6563726166743a756e6976657273616c0103136d696e6563726166743a696e5f6e65746865720103106d696e65\
         63726166743a696e5f656e640103166d696e6563726166743a696e5f6f766572776f726c640403000201",
    ));
    assert_eq!(update_tags_packet(&r), expected);
}

fn client_information(language: &str) -> Vec<u8> {
    let mut out = vec![0];
    put_utf(&mut out, language);
    out.extend([10, 0, 1, 0x7f, 1, 0, 1, 0]);
    out
}

fn custom_payload(channel: &str, data: &[u8]) -> Vec<u8> {
    let mut out = vec![2];
    put_utf(&mut out, channel);
    out.extend(data);
    out
}

fn known_packs(packs: &[KnownPack]) -> Vec<u8> {
    let mut out = select_known_packs_packet(packs);
    out[0] = 7;
    out
}

fn sent(actions: &mut Vec<Action>) -> Vec<Vec<u8>> {
    actions
        .drain(..)
        .map(|a| match a {
            Action::Send(p) => p,
            other => panic!("expected a packet, got {other:?}"),
        })
        .collect()
}

const S: i64 = 1_000_000_000;

#[test]
fn a_full_configuration() {
    let r = registries();
    let features = features();
    let s = settings(&r, &features, &[]);
    let profile = GameProfile::offline("Notch");
    let mut out = Vec::new();
    let mut c = Configuration::start(profile.clone(), false, &s, 100 * S, &mut out);
    assert_eq!(
        sent(&mut out),
        [
            brand_packet("Leaf"),
            update_enabled_features_packet(&features),
            select_known_packs_packet(&r.known_packs)
        ]
    );
    assert_eq!(c.current_task(), Some(Task::SynchronizeRegistries));

    c.handle(&client_information("ru_RU"), &s, 100 * S, &mut out);
    let mut brand = Vec::new();
    put_utf(&mut brand, "vanilla");
    c.handle(
        &custom_payload("minecraft:brand", &brand),
        &s,
        100 * S,
        &mut out,
    );
    c.handle(
        &custom_payload("minecraft:register", b"a:b\0BungeeCord\0\0c:d"),
        &s,
        100 * S,
        &mut out,
    );
    c.handle(
        &custom_payload("minecraft:unregister", b"c:d"),
        &s,
        100 * S,
        &mut out,
    );
    assert!(out.is_empty());

    // The client knows `core`: registries with only the unknown entries' data, then the tags.
    c.handle(&known_packs(&r.known_packs), &s, 100 * S, &mut out);
    let prepare = out.pop();
    assert!(matches!(prepare, Some(Action::PrepareSpawn)), "{prepare:?}");
    let packets = sent(&mut out);
    assert_eq!(packets.len(), 31);
    assert_eq!(
        packets[18],
        registry_data_packet(
            r.synced("minecraft:dimension_type").unwrap(),
            &r.known_packs
        )
    );
    assert_eq!(packets[30], update_tags_packet(&r));
    // Paper's event task finished at once.
    assert_eq!(c.current_task(), Some(Task::PrepareSpawn));

    // A keep-alive each second once the last one is a second old, answered: latency measured.
    c.tick(&s, 100 * S + S / 2, &mut out);
    assert!(out.is_empty());
    c.tick(&s, 101 * S, &mut out);
    assert_eq!(sent(&mut out), [keep_alive_packet(101_000)]);
    let mut pong = vec![4];
    pong.extend(101_000i64.to_be_bytes());
    c.handle(&pong, &s, 101 * S + 40_000_000, &mut out);
    assert!(out.is_empty());

    c.spawn_prepared();
    c.tick(&s, 101 * S + S / 2, &mut out);
    assert_eq!(sent(&mut out), [finish_configuration_packet()]);
    assert_eq!(c.current_task(), Some(Task::JoinWorld));
    // Closed by the terminal packet: no more keep-alives.
    c.tick(&s, 103 * S, &mut out);
    assert!(out.is_empty());

    c.handle(&[3], &s, 103 * S, &mut out);
    let Some(Action::Play(handoff)) = out.pop() else {
        panic!("{out:?}")
    };
    assert_eq!(handoff.profile, profile);
    assert_eq!(handoff.latency, 40);
    assert_eq!(handoff.client_information.language, "ru_RU");
    assert_eq!(handoff.client_information.view_distance, 10);
    assert_eq!(handoff.client_brand.as_deref(), Some("vanilla"));
    assert_eq!(
        handoff.channels,
        BTreeSet::from(["a:b".to_string(), "bungeecord:main".to_string()])
    );
    assert!(out.is_empty());
}

#[test]
fn unknown_packs_get_every_entry_in_full() {
    let r = registries();
    let features = features();
    let s = settings(&r, &features, &[]);
    let mut out = Vec::new();
    let mut c = Configuration::start(GameProfile::offline("Notch"), false, &s, 0, &mut out);
    out.clear();
    // Any answer other than exactly the offered list: nothing is known.
    let other = [KnownPack::vanilla("core"), KnownPack::vanilla("other")];
    c.handle(&known_packs(&other), &s, 0, &mut out);
    out.pop();
    let packets = sent(&mut out);
    assert_eq!(
        packets[18],
        registry_data_packet(r.synced("minecraft:dimension_type").unwrap(), &[])
    );
}

#[test]
fn code_of_conduct_by_language() {
    let r = registries();
    let features = features();
    let coc = [
        ("de_de".to_string(), "Sei nett".to_string()),
        ("en_us".to_string(), "Be nice".to_string()),
    ];
    let s = settings(&r, &features, &coc);
    let mut out = Vec::new();
    let mut c = Configuration::start(GameProfile::offline("Notch"), false, &s, 0, &mut out);
    c.handle(&client_information("fr_FR"), &s, 0, &mut out);
    out.clear();
    c.handle(&known_packs(&r.known_packs), &s, 0, &mut out);
    assert_eq!(
        sent(&mut out).pop(),
        Some(code_of_conduct_packet("Be nice"))
    );
    assert_eq!(c.current_task(), Some(Task::CodeOfConduct));
    c.handle(&[9], &s, 0, &mut out);
    assert!(matches!(out.pop(), Some(Action::PrepareSpawn)));
}

#[test]
fn failures_disconnect_as_java_does() {
    let r = registries();
    let features = features();
    let s = settings(&r, &features, &[]);
    let start = |out: &mut Vec<Action>| {
        let c = Configuration::start(GameProfile::offline("Notch"), false, &s, 0, out);
        out.clear();
        c
    };
    let reason = |out: &mut Vec<Action>| match out.pop() {
        Some(Action::Disconnect(r)) => r,
        other => panic!("{other:?}"),
    };
    let mut out = Vec::new();

    // Accepting a code of conduct that was never sent: Netty-thread exception.
    let mut c = start(&mut out);
    c.handle(&[9], &s, 0, &mut out);
    assert_eq!(
        reason(&mut out),
        Fault::illegal_state(
            "Unexpected request for task finish, current task: synchronize_registries, requested: server_code_of_conduct"
        )
        .reason()
    );
    c.handle(&[9], &s, 0, &mut out);
    assert!(out.is_empty(), "nothing after the disconnect");

    // A finish before the server's: main thread, logged and ignored.
    let mut c = start(&mut out);
    c.handle(&[3], &s, 0, &mut out);
    assert!(out.is_empty());

    let mut c = start(&mut out);
    c.handle(&[42], &s, 0, &mut out);
    assert_eq!(
        reason(&mut out),
        Fault::decoder("Received unknown packet id 42").reason()
    );
    let mut c = start(&mut out);
    c.handle(&[3, 0], &s, 0, &mut out);
    assert_eq!(
        reason(&mut out),
        Fault::decoder(
            "java.io.IOException: Packet configuration/serverbound/minecraft:finish_configuration \
             (ServerboundFinishConfigurationPacket) was larger than I expected, found 1 bytes extra whilst reading \
             packet serverbound/minecraft:finish_configuration"
        )
        .reason()
    );
    let mut c = start(&mut out);
    c.handle(
        &custom_payload("minecraft:register", b"NoSeparator"),
        &s,
        0,
        &mut out,
    );
    assert_eq!(
        reason(&mut out),
        Reason::Literal("Invalid custom payload payload!".into())
    );
    let mut c = start(&mut out);
    c.handle(&[1, 3, b'a', b':', b'b', 0], &s, 0, &mut out);
    assert_eq!(
        reason(&mut out),
        Reason::translatable("multiplayer.disconnect.unexpected_query_response")
    );

    // Keep-alives: a wrong answer, then silence for over 30 s.
    let mut c = start(&mut out);
    c.tick(&s, 2 * S, &mut out);
    out.clear();
    let mut pong = vec![4];
    pong.extend(7i64.to_be_bytes());
    c.handle(&pong, &s, 2 * S, &mut out);
    assert_eq!(reason(&mut out), Reason::translatable("disconnect.timeout"));
    let mut c = start(&mut out);
    for t in 1..=31 {
        c.tick(&s, t * S, &mut out);
    }
    assert!(out.iter().all(|a| matches!(a, Action::Send(_))));
    c.tick(&s, 31 * S + 1, &mut out);
    assert_eq!(reason(&mut out), Reason::translatable("disconnect.timeout"));
}

/// Everything against the real jar and the Java oracle's output (`LeafConfigurationOracle.java`, run
/// with the `paper` pack): `HORIZON_VANILLA_JAR`, `HORIZON_REGISTRIES_JSON`, `HORIZON_ORACLE_TSV`.
#[test]
#[ignore = "needs the 26.3 jar and the oracle's output"]
fn real_registries_match_java() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let jar_path = var("HORIZON_VANILLA_JAR");
    let mut jar = crate::server::vanilla_jar::VanillaJar::open(jar_path.as_ref()).unwrap();
    let r = Registries::load(&mut jar, var("HORIZON_REGISTRIES_JSON").as_ref()).unwrap();
    let oracle = std::fs::read_to_string(var("HORIZON_ORACLE_TSV")).unwrap();
    let tags_of = |name: &str| {
        let synced = r.synced(name).map(|x| &x.tags);
        synced.or_else(|| r.builtin(name).map(|x| &x.tags)).unwrap()
    };
    let (mut checked, mut tag_lines, mut reordered) = (0, 0, Vec::new());
    for row in oracle.lines().map(|l| l.split('\t').collect::<Vec<_>>()) {
        match row[0] {
            "select_known_packs" => {
                assert_eq!(
                    select_known_packs_packet(&r.known_packs),
                    with_id(15, row[1])
                )
            }
            "registry_known" => {
                // Leaf removes the game test registries vanilla syncs.
                let Some(reg) = r.synced(row[1]) else {
                    assert!(row[1].starts_with("minecraft:test_"), "{}", row[1]);
                    continue;
                };
                let packet = registry_data_packet(reg, &r.known_packs);
                assert_eq!(packet, with_id(7, row[2]), "{}", row[1]);
                checked += 1;
            }
            "raw" => {
                // `JsonOps.convertTo(NbtOps)` of every synced file, byte for byte.
                let (ns, path) = row[1].split_once(':').unwrap();
                let found = crate::registry::SYNCHRONIZED.iter().find_map(|name| {
                    let dir = name.strip_prefix("minecraft:").unwrap();
                    let id = path
                        .strip_prefix(dir)?
                        .strip_prefix('/')?
                        .strip_suffix(".json")?;
                    Some((name, format!("{ns}:{id}")))
                });
                let Some((name, id)) = found else {
                    assert!(path.starts_with("test_"), "{path}");
                    continue;
                };
                let entries = &r.synced(name).unwrap().entries;
                let e = entries.iter().find(|e| e.id == id).unwrap();
                assert_eq!(*e.nbt, hex(row[2])[..], "{}", row[1]);
                checked += 1;
            }
            "tag" => {
                let ids: Vec<i32> = serde_json::from_str(row[3]).unwrap();
                assert_eq!(
                    tags_of(row[1]).get(row[2]),
                    Some(&ids[..]),
                    "{} {}",
                    row[1],
                    row[2]
                );
                tag_lines += 1;
            }
            "tags" => {
                // Same tags; the order may differ only inside a hash bucket (Java's insertion
                // order there follows identity hashes).
                let mut java = vec![14, 1];
                put_utf(&mut java, row[1]);
                java.extend(hex(row[2]));
                let one = Registries {
                    known_packs: Vec::new(),
                    synced: r
                        .synced
                        .iter()
                        .filter(|b| b.name == row[1])
                        .cloned()
                        .collect(),
                    builtin: r
                        .builtin
                        .iter()
                        .filter(|b| b.name == row[1])
                        .cloned()
                        .collect(),
                };
                let ours = update_tags_packet(&one);
                assert_eq!(ours.len(), java.len(), "{}", row[1]);
                if ours != java {
                    reordered.push(row[1].to_owned());
                }
            }
            _ => {}
        }
    }
    let synced = r.synced.iter().map(|x| x.tags.0.len());
    let total: usize = synced.chain(r.builtin.iter().map(|x| x.tags.0.len())).sum();
    assert_eq!(total, tag_lines);
    eprintln!("checked {checked} packets/entries, {tag_lines} tags; reordered: {reordered:?}");
}
