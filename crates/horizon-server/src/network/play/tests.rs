//! Against `testdata/play_oracle.txt`: each line is a name and the bytes the 26.3 classes encode
//! (`GameProtocols.CLIENTBOUND_TEMPLATE`'s codec, so ids included; serverbound lines are bodies
//! without the id). Dynamic registries were small stand-ins in the oracle: dimension types
//! `[overworld, overworld_caves, the_end, the_nether]`, world clocks `[overworld, the_end]`.

use uuid::uuid;

use super::chunk::{self, BlockEntityInfo, LightSection};
use super::chunk_sending::{self, ChunkRateLimiter, ViewDistances};
use super::clientbound::{self as cb, GameType, PlayerInfoEntry, PositionMoveRotation, SpawnInfo};
use super::join::{self, JoinStep};
use super::serverbound::{self, ClientInformation, Serverbound};
use crate::azalea::chat::FormattedText;
use crate::azalea::chat::translatable_component::{PrimitiveOrComponent, TranslatableComponent};
use crate::block::BlockState;
use crate::core::position::{BlockPos, ChunkPos};
use crate::nbt;
use crate::network::login::{GameProfile, Property};
use crate::world::chunk::{ContainerFactory, LevelChunkSection, SectionFlags, StateFlags};

const ORACLE: &str = include_str!("testdata/play_oracle.txt");

fn oracle(name: &str) -> Vec<u8> {
    let hex = ORACLE
        .lines()
        .find_map(|l| l.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no oracle line {name}"));
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[track_caller]
fn check(name: &str, bytes: Vec<u8>) {
    let expected = oracle(name);
    if bytes != expected {
        let at = bytes
            .iter()
            .zip(&expected)
            .take_while(|(a, b)| a == b)
            .count();
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let (from, to) = (at.saturating_sub(8), at + 24);
        panic!(
            "{name}: lengths {} vs {}, first difference at {at}\n ours: {}\n java: {}",
            bytes.len(),
            expected.len(),
            hex(&bytes[from.min(bytes.len())..to.min(bytes.len())]),
            hex(&expected[from.min(expected.len())..to.min(expected.len())]),
        );
    }
}

const LEVELS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];

fn login() -> cb::Login<'static> {
    cb::Login {
        player_id: 42,
        hardcore: false,
        levels: &LEVELS,
        max_players: 20,
        chunk_radius: 10,
        simulation_distance: 8,
        reduced_debug_info: false,
        show_death_screen: true,
        do_limited_crafting: false,
        spawn: SpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld",
            seed: -123_456_789_012_345,
            game_type: GameType::Survival,
            previous_game_type: None,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
        },
        online_mode: false,
        enforces_secure_chat: false,
    }
}

#[test]
fn login_matches_java() {
    check("login", cb::login(&login()));
    let nether = ["minecraft:the_nether"];
    check(
        "login2",
        cb::login(&cb::Login {
            player_id: 300,
            hardcore: true,
            levels: &nether,
            max_players: 1000,
            chunk_radius: 32,
            simulation_distance: 2,
            reduced_debug_info: true,
            show_death_screen: false,
            do_limited_crafting: true,
            spawn: SpawnInfo {
                dimension_type: 3,
                dimension: "minecraft:the_nether",
                seed: 7,
                game_type: GameType::Creative,
                previous_game_type: Some(GameType::Spectator),
                is_debug: true,
                is_flat: true,
                last_death_location: Some(("minecraft:the_nether", BlockPos::new(1, -2, 3))),
                portal_cooldown: 300,
                sea_level: 32,
            },
            online_mode: true,
            enforces_secure_chat: true,
        }),
    );
}

#[test]
fn small_packets_match_java() {
    let still = PositionMoveRotation {
        position: [8.5, 65.0, -3.25],
        delta: [0.0; 3],
        y_rot: 90.0,
        x_rot: -12.5,
    };
    check("position", cb::player_position(1, &still, 0));
    let moving = PositionMoveRotation {
        position: [-1.0E7, 1.5, 0.1],
        delta: [0.25, -0.5, 1.0],
        y_rot: -179.9,
        x_rot: 45.0,
    };
    use cb::relative::*;
    check(
        "position2",
        cb::player_position(200, &moving, X | Y_ROT | DELTA_Z | ROTATE_DELTA),
    );
    use cb::game_event::*;
    check("game_event", cb::game_event(LEVEL_CHUNKS_LOAD_START, 0.0));
    check("game_event_rain", cb::game_event(RAIN_LEVEL_CHANGE, 0.75));
    check("center", cb::set_chunk_cache_center(-3, 700));
    check("radius", cb::set_chunk_cache_radius(10));
    check("simulation", cb::set_simulation_distance(8));
    check("batch_start", cb::chunk_batch_start());
    check("batch_finished", cb::chunk_batch_finished(300));
    check(
        "spawn",
        cb::set_default_spawn_position(
            "minecraft:overworld",
            BlockPos::new(-10, 64, 200),
            45.0,
            -5.0,
        ),
    );
    let clock = |clock, total_ticks, partial_tick, rate| cb::ClockUpdate {
        clock,
        total_ticks,
        partial_tick,
        rate,
    };
    check("time", cb::set_time(123_456, &[clock(0, 6000, 0.5, 1.0)]));
    check("time2", cb::set_time(0, &[clock(1, -1, 0.0, 0.0)]));
    check("keep_alive", cb::keep_alive(0x0102_0304_0506_0708));
    check("difficulty", cb::change_difficulty(3, true));
    let abilities = cb::Abilities {
        invulnerable: true,
        flying: false,
        may_fly: true,
        instabuild: true,
        flying_speed: 0.05,
        walking_speed: 0.1,
    };
    check("abilities", cb::player_abilities(&abilities));
    check("held_slot", cb::set_held_slot(4));
    check("entity_event", cb::entity_event(42, 28));
    check("border", cb::initialize_border(&cb::Border::default()));
    check("ticking_state", cb::ticking_state(20.0, false));
    check("ticking_step", cb::ticking_step(0));
}

#[test]
fn components_match_java() {
    check("disconnect", cb::disconnect(&"Bye".into()).unwrap());
    let joined = FormattedText::from(TranslatableComponent::new(
        "multiplayer.player.joined".into(),
        vec![PrimitiveOrComponent::String("Notch".into())],
    ));
    // Java writes compound keys in `HashMap` order ("with" before "translate"), the NBT writer in
    // key order: the same tag, compared as tags.
    let ours = cb::system_chat(&joined, false).unwrap();
    let java = oracle("system_chat");
    let tag = |b: &[u8]| nbt::read_network(&b[1..b.len() - 1], nbt::Limits::default()).unwrap();
    assert_eq!((ours[0], ours.last()), (java[0], java.last()));
    assert_eq!(tag(&ours), tag(&java));
    check(
        "system_chat_overlay",
        cb::system_chat(&"Hi".into(), true).unwrap(),
    );
}

fn notch() -> GameProfile {
    GameProfile {
        id: uuid!("069a79f4-44e9-4726-a5be-fca90e38aaf5"),
        name: "Notch".into(),
        properties: vec![Property {
            name: "textures".into(),
            value: "e30=".into(),
            signature: Some("c2ln".into()),
        }],
    }
}

fn entry(profile: &GameProfile) -> PlayerInfoEntry<'_> {
    PlayerInfoEntry {
        profile,
        listed: true,
        latency: 35,
        game_mode: GameType::Creative,
        display_name: None,
        show_hat: true,
        list_order: 0,
        chat_session: None,
    }
}

#[test]
fn player_info_matches_java() {
    let notch = notch();
    let alex = GameProfile {
        id: uuid!("00000000-0000-0003-0000-000000000004"),
        name: "Alex".into(),
        properties: Vec::new(),
    };
    let al = FormattedText::from("Al");
    let entries = [
        entry(&notch),
        PlayerInfoEntry {
            profile: &alex,
            listed: false,
            latency: 0,
            game_mode: GameType::Survival,
            display_name: Some(&al),
            show_hat: false,
            list_order: 7,
            chat_session: None,
        },
    ];
    check(
        "player_info",
        cb::player_info_update(cb::info_action::INITIALIZING, &entries).unwrap(),
    );
}

fn light(seed: u8) -> Box<[u8; 2048]> {
    Box::new(std::array::from_fn(|i| {
        (i as u8).wrapping_mul(7).wrapping_add(seed)
    }))
}

#[test]
fn light_update_matches_java() {
    let (a, b, c) = (light(1), light(2), light(3));
    let mut sky = vec![LightSection::Absent; 26];
    sky[..4].fill(LightSection::Empty);
    sky[5] = LightSection::Data(&a);
    sky[25] = LightSection::Data(&b);
    let mut block = vec![LightSection::Absent; 71];
    block[70] = LightSection::Data(&c);
    check(
        "light_update",
        chunk::light_update(ChunkPos::new(2, -1), Some(&sky), &block),
    );
    // Starlight's states: null and hidden are absent, uninitialised empty, initialised data.
    assert_eq!(
        LightSection::from_starlight(0, Some(&a)),
        LightSection::Absent
    );
    assert_eq!(LightSection::from_starlight(1, None), LightSection::Empty);
    assert_eq!(
        LightSection::from_starlight(2, Some(&a)),
        LightSection::Data(&a)
    );
    assert_eq!(
        LightSection::from_starlight(3, Some(&a)),
        LightSection::Absent
    );
}

/// The oracle chunk's states: air, stone 1, grass block 9, dirt 10, water 89.
struct Flags;
impl SectionFlags for Flags {
    fn section_flags(&self, state: BlockState) -> StateFlags {
        StateFlags(match state.0 {
            0 => StateFlags::AIR,
            89 => StateFlags::HAS_FLUID,
            _ => 0,
        })
    }
}

#[test]
fn chunk_matches_java() {
    let factory = ContainerFactory::new(35_723, 65, 0);
    let sections: Vec<LevelChunkSection> = (0..4)
        .map(|s| {
            let mut sec = LevelChunkSection::empty(&factory);
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..16 {
                        let state = match (s, y) {
                            (0, _) => 1,
                            (1, 0 | 1) => 10,
                            (1, 2) => 9,
                            (1, 3) if x < 4 && z < 4 => 89,
                            _ => continue,
                        };
                        sec.set_block_state(x, y, z, BlockState(state), &Flags);
                    }
                }
            }
            if s == 1 {
                sec.set_noise_biome(0, 0, 0, 7);
                sec.set_noise_biome(3, 3, 3, 64);
            }
            sec
        })
        .collect();
    let mut heightmaps: [Option<Vec<i64>>; 6] = Default::default();
    heightmaps[1] = Some(vec![1, -1, 0x1_2345_6789]);
    heightmaps[4] = Some(Vec::new());
    // {x: 5, id: "minecraft:sign"} in Java's HashMap order.
    let mut sign = vec![0x0a, 0x03, 0x00, 0x01, b'x', 0, 0, 0, 5, 0x08, 0x00, 0x02];
    sign.extend_from_slice(b"id\x00\x0eminecraft:sign\x00");
    let block_entities = [
        BlockEntityInfo {
            packed_xz: BlockEntityInfo::pack_xz(3, 15),
            y: -60,
            type_id: 1,
            tag: None,
        },
        BlockEntityInfo {
            packed_xz: BlockEntityInfo::pack_xz(-6, 2),
            y: 70,
            type_id: 7,
            tag: Some(&sign),
        },
    ];
    let l: Vec<_> = (9..=12).map(light).collect();
    use LightSection::{Data, Empty};
    let sky = [Empty, Data(&l[0]), Data(&l[1]), Empty, Empty, Data(&l[2])];
    let block = [Empty, Empty, Data(&l[3]), Empty, Empty, Empty];
    check(
        "chunk",
        chunk::level_chunk_with_light(
            ChunkPos::new(-5, 12),
            &heightmaps,
            &sections,
            &block_entities,
            Some(&sky),
            &block,
        ),
    );
}

#[test]
fn serverbound_decode_like_java() {
    let decode = |id: u8, name: &str| {
        let mut body = vec![id];
        body.extend(oracle(name));
        serverbound::decode(&body).map(|p| format!("{p:?}"))
    };
    let expect = |p: Serverbound| Ok(format!("{p:?}"));
    assert_eq!(
        decode(0, "sb_accept_teleport"),
        expect(Serverbound::AcceptTeleportation {
            id: 7,
            position: [1.5, -2.0, 3.25],
            y_rot: 10.0,
            x_rot: -20.0
        })
    );
    assert_eq!(
        decode(11, "sb_batch_received"),
        expect(Serverbound::ChunkBatchReceived {
            desired_chunks_per_tick: 9.5
        })
    );
    assert_eq!(
        decode(28, "sb_keep_alive"),
        expect(Serverbound::KeepAlive(-2))
    );
    assert_eq!(
        decode(14, "sb_client_information"),
        expect(Serverbound::ClientInformation(ClientInformation {
            language: "en_us".into(),
            view_distance: 12,
            chat_visibility: 1,
            chat_colors: true,
            model_customisation: 0x7f,
            main_hand: 0,
            text_filtering_enabled: false,
            allows_listing: true,
            particle_status: 1,
        }))
    );
    let mv = |position, rotation, on_ground, horizontal_collision| {
        expect(Serverbound::MovePlayer {
            position,
            rotation,
            on_ground,
            horizontal_collision,
        })
    };
    assert_eq!(
        decode(31, "sb_move_pos_rot"),
        mv(Some([1.0, 2.0, 3.0]), Some((4.0, 5.0)), true, true)
    );
    assert_eq!(
        decode(30, "sb_move_pos"),
        mv(Some([-1.0, 70.0, 0.5]), None, false, true)
    );
    assert_eq!(
        decode(32, "sb_move_rot"),
        mv(None, Some((180.0, -90.0)), true, false)
    );
    assert_eq!(decode(33, "sb_move_status"), mv(None, None, true, false));
    assert_eq!(serverbound::decode(&[44]), Ok(Serverbound::PlayerLoaded));
    assert_eq!(serverbound::decode(&[13]), Ok(Serverbound::ClientTickEnd));
    assert_eq!(
        serverbound::decode(&[69]).unwrap_err().0,
        "io.netty.handler.codec.DecoderException: Received unknown packet id 69"
    );
    assert!(serverbound::decode(&[28, 0, 0]).is_err());
    assert!(serverbound::decode(&[44, 0]).is_err());
    assert_eq!(
        serverbound::decode(&[2, 9, 9]),
        Ok(Serverbound::Other {
            id: 2,
            body: &[9, 9]
        })
    );
}

#[test]
fn moonrise_distances() {
    // server.properties defaults: view-distance 10, simulation-distance 10.
    let level = ViewDistances::for_level(10, 10);
    assert_eq!(chunk_sending::api_distances(level), (10, 10));
    // A client asking for 4 gets 5 (auto send distance), capped by the view distance.
    let d = chunk_sending::player_distances(ViewDistances::UNSET, level, 4, true);
    assert_eq!((d.tick, d.load, d.send), (10, 11, 5));
    let d = chunk_sending::player_distances(ViewDistances::UNSET, level, 20, true);
    assert_eq!(d.send, 10);
    // Simulation above view: the tick distance follows the load distance - 1.
    let level = ViewDistances::for_level(4, 12);
    assert_eq!(chunk_sending::api_distances(level), (4, 4));
    assert_eq!(
        ViewDistances::for_level(1, -3),
        ViewDistances {
            tick: 0,
            load: 3,
            send: -1
        }
    );
    assert!(chunk_sending::want_chunk_sent((0, 0), 2, (3, 0)));
    assert!(!chunk_sending::want_chunk_sent((0, 0), 2, (4, 0)));
    assert!(chunk_sending::want_chunk_sent((0, 0), 2, (3, 3)));
    assert!(!chunk_sending::want_chunk_sent((0, 0), 2, (4, 1)));
    assert_eq!(chunk_sending::effective_rate(-1.0), chunk_sending::MAX_RATE);
    assert_eq!(chunk_sending::effective_rate(0.5), 1.0);
}

#[test]
fn rate_limiter_matches_moonrise() {
    // From Moonrise's StaggeredRateLimiter with the loader's windows: (time ms, rate, wanted) →
    // taken, over a joining player's first ticks at Paper's 75 chunks/s.
    let steps: &[(i64, f64, i64, i64)] = &[
        (0, 75.0, 100, 0),
        (50, 75.0, 100, 3),
        (100, 75.0, 100, 4),
        (150, 75.0, 100, 4),
        (1_000, 75.0, 100, 30),
        (1_050, 75.0, 2, 2),
        (1_100, 10.0, 100, 0),
        (1_200, 10.0, 100, 1),
        (1_250, 10.0, 100, 1),
        (5_000, 10.0, 100, 4),
        (5_010, 10_000.0, 100_000, 100),
        (5_020, 10_000.0, 100_000, 100),
    ];
    let mut limiter = ChunkRateLimiter::new(0);
    for &(ms, rate, wanted, taken) in steps {
        limiter.tick(ms * 1_000_000, rate);
        assert_eq!(limiter.take(wanted), taken, "at {ms} ms");
    }
}

#[test]
fn join_sequence_order() {
    let notch = notch();
    let level = join::LevelInfo {
        border: cb::Border::default(),
        game_time: 0,
        clocks: &[],
        spawn_dimension: "minecraft:overworld",
        spawn_pos: BlockPos::new(0, 64, 0),
        spawn_yaw: 0.0,
        spawn_pitch: 0.0,
        raining: true,
        rain_level: 1.0,
        thunder_level: 0.0,
        tick_rate: 20.0,
        frozen: false,
        frozen_ticks_to_run: 0,
    };
    let inputs = join::JoinInputs {
        login: login(),
        difficulty: 2,
        difficulty_locked: false,
        abilities: cb::Abilities {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        selected_slot: 0,
        permission_level: 4,
        teleport_id: 1,
        position: [0.5, 64.0, 0.5],
        y_rot: 0.0,
        x_rot: 0.0,
        level,
        own_info: entry(&notch),
        others: &[],
        join_message: None,
        distances: chunk_sending::player_distances(
            ViewDistances::UNSET,
            ViewDistances::for_level(10, 8),
            -1,
            true,
        ),
        chunk: ChunkPos::new(0, 0),
    };
    let steps = join::join_sequence(&inputs).unwrap();
    let names: Vec<String> = steps
        .iter()
        .map(|s| match s {
            JoinStep::Packet(p) => {
                let name = super::ids::CLIENTBOUND[p[0] as usize];
                if name == "game_event" {
                    format!("game_event/{}", p[1])
                } else {
                    name.to_string()
                }
            }
            JoinStep::ToOthers(p) => format!("others:{}", super::ids::CLIENTBOUND[p[0] as usize]),
            other => format!("{other:?}"),
        })
        .collect();
    let level_info = [
        "initialize_border",
        "set_time",
        "set_default_spawn_position",
        "game_event/2",
        "game_event/7",
        "game_event/13",
        "ticking_state",
        "ticking_step",
    ];
    let mut expected = vec![
        "login",
        "change_difficulty",
        "player_abilities",
        "set_held_slot",
        "UpdateRecipes",
        "entity_event",
        "Commands",
        "RecipeBook",
        "Scoreboard",
        "player_position",
    ];
    expected.extend(level_info);
    expected.extend([
        "InventoryMenu",
        "others:player_info_update",
        "player_info_update",
        "set_chunk_cache_radius",
        "set_simulation_distance",
        "set_chunk_cache_center",
        "TrackEntities",
    ]);
    expected.extend(level_info);
    expected.extend(["Effects", "InventoryMenu"]);
    // ticking_state's id (130) takes two VarInt bytes: compare by the first byte's table entry only
    // for single-byte ids.
    let names: Vec<String> = names
        .into_iter()
        .zip(&steps)
        .map(|(n, s)| match s {
            JoinStep::Packet(p) if p[0] & 0x80 != 0 => {
                let id = (p[0] & 0x7f) as usize | (p[1] as usize) << 7;
                super::ids::CLIENTBOUND[id].to_string()
            }
            _ => n,
        })
        .collect();
    assert_eq!(names, expected);
    // The permission level's event: OWNERS = 28.
    assert_eq!(steps[5], JoinStep::Packet(cb::entity_event(42, 28)));
}
