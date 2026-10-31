//! The server over real TCP: a blocking client logs in through a forged Velocity answer while the
//! server runs on its own thread, over a small synthetic world in a temporary folder.
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use bytes::BytesMut;

use super::*;
use crate::azalea::chat::{FormattedText, read_component};
use crate::block::flags::BlockStateData;
use crate::block::{BlockState, BlockStatesBuilder};
use crate::core::position::BlockPos;
use crate::nbt::Tag;
use crate::network::codec::{Inbound, Outbound, put_var_int};
use crate::network::configuration::select_known_packs_packet;
use crate::network::login::tests::{NOTCH, forged_answer, hex};
use crate::network::login::{self, PROTOCOL_VERSION};
use crate::network::play::ids::{cb as play_cb, sb as play_sb};
use crate::registry::tests::pack;
use crate::registry::{DataPack, KnownPack, parse_builtin};
use crate::server::chunk_system::ChunkSettings;
use crate::server::level::{NETHER, OVERWORLD, WorldSettings, WorldTables};
use crate::server::player_data::PlayerDataStorage;
use crate::server::player_data::SavedPlayer;
use crate::world::chunk::{BiomeIds, ChunkStatus, ContainerFactory, SectionFlags, StateFlags};
use crate::world::chunk_io::{ChunkIo, ChunkIoEvent, CodecTables};
use crate::world::level_chunk::{HeightmapTable, LevelChunk, heightmaps_after};
use crate::world::level_storage::{self as storage, LevelData, LevelStorageAccess};
use crate::world::storage::compression::Compression;

fn config(max_players: i32) -> RuntimeConfig {
    RuntimeConfig {
        server_ip: "127.0.0.1".into(),
        server_port: 0,
        login: LoginConfig {
            velocity_secret: Some("s3cret".into()),
            ..LoginConfig::default()
        },
        tick_loop: TickLoopConfig::default(),
        max_players,
        online_mode: false,
        proxy_online_mode: true,
        white_list: false,
        whitelist_message: "You are not whitelisted on this server!".into(),
        user_lists_dir: PathBuf::new(),
        pause_when_empty_seconds: -1,
        view_distance: 2,
        simulation_distance: 2,
        enforces_secure_chat: false,
        autosave_period: 6000,
        shutdown_message: Reason::Literal("Сервер перезапускается".into()),
        boot: Instant::now(),
    }
}

const STONE: BlockState = BlockState(1);

/// The synthetic tables: air, stone, a slab and water (`flags_tests::synthetic`), a 32-block high
/// overworld and nether (the end 64 blocks high) with one biome.
fn tables() -> WorldTables {
    let mut b = BlockStatesBuilder::new();
    for name in ["air", "stone", "oak_slab", "water"] {
        b.add_block(&format!("minecraft:{name}"), &[]).unwrap();
    }
    let blocks = Arc::new(b.finish());
    let block_data =
        Arc::new(BlockStateData::from_bytes(&crate::block::flags_tests::synthetic(4), 4).unwrap());
    let vanilla = pack(
        "vanilla",
        Some(KnownPack::vanilla("core")),
        &[
            (
                "minecraft/dimension_type/overworld.json",
                r#"{"min_y": 0, "height": 32}"#,
            ),
            (
                "minecraft/dimension_type/the_nether.json",
                r#"{"min_y": 0, "height": 32}"#,
            ),
            (
                "minecraft/dimension_type/the_end.json",
                r#"{"min_y": 0, "height": 64}"#,
            ),
            ("minecraft/worldgen/biome/plains.json", "{}"),
        ],
    );
    let registries = Registries::build(
        parse_builtin(crate::registry::tests::REPORT).unwrap(),
        &[vanilla, DataPack::paper()],
    )
    .unwrap();
    WorldTables {
        blocks,
        block_data,
        registries: Arc::new(registries),
    }
}

struct Flags;
impl SectionFlags for Flags {
    fn section_flags(&self, state: BlockState) -> StateFlags {
        StateFlags(if state.0 == 0 { StateFlags::AIR } else { 0 })
    }
}

/// `level.dat` with the spawn at (8, 0, 8) and the chunks -2..=2 squared: stone at y = 3, the
/// sky light of section 0 saved as all 15s.
fn make_world(dir: &Path, tables: &WorldTables) {
    let access = LevelStorageAccess::open(dir, "world").unwrap();
    let mut data = LevelData::parse(&Default::default());
    data.spawn.pos = BlockPos::new(8, 0, 8);
    access.save_level_data(&data).unwrap();
    let codec = Arc::new(CodecTables {
        factory: ContainerFactory::new(4, 1, 0),
        blocks: tables.blocks.clone(),
        flags: Arc::new(Flags),
        biomes: Arc::new(BiomeIds::new(["minecraft:plains"])),
        min_section_y: 0,
        max_section_y: 1,
    });
    let heightmaps = HeightmapTable::new(4, |s| if s.0 == 0 { 0 } else { 0x3f });
    // The overworld's chunks have stone at y = 3 and full sky light; the nether's stone at y = 5
    // and a sky light of 0x77, to tell them apart in a packet.
    for (dimension, stone_y, light) in [(OVERWORLD, 3, 0xff), (NETHER, 5, 0x77)] {
        let (io, events) = ChunkIo::spawn(
            access.region_path(dimension, "region"),
            Compression::Lz4,
            false,
            codec.clone(),
            1,
        )
        .unwrap();
        for x in -2..=2 {
            for z in -2..=2 {
                let pos = ChunkPos::new(x, z);
                let mut chunk = LevelChunk::empty(pos, 0, 2, &codec.factory);
                chunk.prime_heightmaps(heightmaps_after(ChunkStatus::Full), &heightmaps);
                for (bx, bz) in [(0, 0), (8, 8), (15, 15)] {
                    chunk.set_block_state(
                        BlockPos::new(bx, stone_y, bz),
                        STONE,
                        &Flags,
                        &heightmaps,
                    );
                }
                let mut data = chunk.to_data();
                data.light_correct = true;
                let section = data.sections.iter_mut().find(|s| s.y == 0).unwrap();
                section.sky_light = Some(Box::new([light; 2048]));
                section.sky_light_state = Some(2);
                io.save(pos, Some(Box::new(data)));
            }
        }
        io.shutdown();
        assert!(events.iter().all(|e| match e {
            ChunkIoEvent::Saved { result, .. } => result.is_ok(),
            _ => true,
        }));
    }
}

struct Running {
    addr: SocketAddr,
    control: std_mpsc::Sender<Control>,
    thread: JoinHandle<u8>,
    dir: tempfile::TempDir,
}

/// The server over the synthetic world, not running yet.
fn start(config: RuntimeConfig, chunks: ChunkSettings) -> (Server, tempfile::TempDir) {
    start_with(config, chunks, |_| {})
}

/// [`start`], after `before` has written files into the server folder.
fn start_with(
    mut config: RuntimeConfig,
    chunks: ChunkSettings,
    before: impl FnOnce(&Path),
) -> (Server, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    before(dir.path());
    config.user_lists_dir = dir.path().to_owned();
    let tables = tables();
    make_world(dir.path(), &tables);
    let settings = WorldSettings {
        level_name: "world",
        compression: Compression::Lz4,
        sync_chunk_writes: false,
        new_world: (
            storage::GameType::Survival,
            storage::Difficulty::Normal,
            false,
        ),
        chunks,
    };
    let server = Server::start(config, tables.registries.clone(), || {
        World::open(dir.path(), &settings, &tables)
    })
    .unwrap();
    (server, dir)
}

fn run(config: RuntimeConfig) -> Running {
    run_with(config, |_| {})
}

fn run_with(config: RuntimeConfig, before: impl FnOnce(&Path)) -> Running {
    let (server, dir) = start_with(config, ChunkSettings::default(), before);
    Running {
        addr: server.local_addr(),
        control: server.control(),
        thread: std::thread::spawn(move || server.run()),
        dir,
    }
}

impl Running {
    fn stop(self, control: Control) -> (u8, tempfile::TempDir) {
        self.control.send(control).unwrap();
        (self.thread.join().unwrap(), self.dir)
    }
}

struct Client {
    stream: TcpStream,
    buf: BytesMut,
    inbound: Inbound,
    outbound: Outbound,
}

impl Client {
    fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Client {
            stream,
            buf: BytesMut::new(),
            inbound: Inbound::new(),
            outbound: Outbound::new(),
        }
    }

    fn send(&mut self, body: &[u8]) {
        let mut out = BytesMut::new();
        self.outbound.encode(body, &mut out).unwrap();
        self.stream.write_all(&out).unwrap();
    }

    /// The next packet body, or `None` once the server closed the connection.
    fn recv(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(packet) = self.inbound.decode(&mut self.buf).unwrap() {
                return Some(packet.to_vec());
            }
            let mut chunk = [0; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
            }
        }
    }

    /// Packets until one with this id; `None` when the connection closes first.
    fn until(&mut self, id: u8, seen: &mut Vec<Vec<u8>>) -> Option<Vec<u8>> {
        loop {
            let packet = self.recv()?;
            if packet[0] == id {
                return Some(packet);
            }
            seen.push(packet);
        }
    }

    /// Handshake, hello and the Velocity answer; returns the server's next packet.
    fn log_in(addr: SocketAddr) -> (Self, Vec<u8>) {
        let mut client = Client::connect(addr);
        let mut intention = vec![0];
        put_var_int(&mut intention, PROTOCOL_VERSION);
        intention.push(9);
        intention.extend(b"localhost");
        intention.extend([0x63, 0xdd, 2]);
        client.send(&intention);
        client.send(&hex("00054e6f746368069a79f444e94726a5befca90e38aaf5"));
        let query = client.recv().unwrap();
        let mut answer = vec![2];
        answer.extend_from_slice(&query[1..query.len() - 22]);
        answer.push(1);
        answer.extend(forged_answer());
        client.send(&answer);
        let next = client.recv().unwrap();
        (client, next)
    }

    /// All the way to the configuration state.
    fn join(addr: SocketAddr) -> Self {
        let (mut client, compression) = Client::log_in(addr);
        assert_eq!(compression, hex("038002"));
        client.inbound.set_compression(256);
        client.outbound.set_compression(256);
        let finished = client.recv().unwrap();
        assert_eq!(finished[0], 2, "login_finished");
        client.send(&[3]);
        client
    }

    /// Through the configuration as a vanilla client (view distance 1), up to the play state.
    fn play(addr: SocketAddr) -> Self {
        let mut client = Client::join(addr);
        let mut seen = Vec::new();
        let packs = client.until(15, &mut seen).expect("select_known_packs");
        let mut info = vec![0];
        put_var_int(&mut info, 5);
        info.extend(b"en_us");
        info.extend([1, 0, 1, 0x7f, 1, 0, 1, 0]);
        client.send(&info);
        // The client knows `core`: it answers with the same list.
        let mut answer = packs.clone();
        answer[0] = 7;
        assert_eq!(
            packs,
            select_known_packs_packet(&[KnownPack::vanilla("core")])
        );
        client.send(&answer);
        loop {
            let packet = client.recv().expect("configuration packets");
            match packet[0] {
                // Keep-alives are answered with their challenge.
                4 => client.send(&packet),
                3 => break,
                _ => {}
            }
        }
        client.send(&[3]);
        client
    }

    fn send_play(&mut self, id: i32, fields: &[u8]) {
        let mut body = Vec::new();
        put_var_int(&mut body, id);
        body.extend(fields);
        self.send(&body);
    }

    /// Play packets until `id`, answering keep-alives.
    fn until_play(&mut self, id: u8, seen: &mut Vec<Vec<u8>>) -> Vec<u8> {
        loop {
            let packet = self.recv().expect("a play packet");
            if packet[0] == play_cb::KEEP_ALIVE {
                self.send_play(play_sb::KEEP_ALIVE, &packet[1..]);
            } else if packet[0] == id {
                return packet;
            } else {
                seen.push(packet);
            }
        }
    }

    fn move_to(&mut self, x: f64, y: f64, z: f64) {
        let mut fields = Vec::new();
        for v in [x, y, z] {
            fields.extend(v.to_be_bytes());
        }
        fields.push(1); // on ground
        self.send_play(play_sb::MOVE_PLAYER_POS, &fields);
    }
}

fn component(packet: &[u8]) -> FormattedText {
    read_component(&mut std::io::Cursor::new(&packet[1..])).unwrap()
}

fn chunk_pos(packet: &[u8]) -> (i32, i32) {
    let x = i32::from_be_bytes(packet[1..5].try_into().unwrap());
    let z = i32::from_be_bytes(packet[5..9].try_into().unwrap());
    (x, z)
}

/// Login, configuration, play: the join, the chunks that can be sent, a teleport accepted, moves
/// across chunk borders, and the player's file written when it leaves.
#[test]
fn a_player_joins_sees_chunks_moves_and_is_saved() {
    let server = run(config(20));
    let mut client = Client::play(server.addr);
    let mut seen = Vec::new();
    let login = client.until_play(play_cb::LOGIN, &mut seen);
    assert_eq!(&login[1..5], &1i32.to_be_bytes(), "entity id 1");
    let position = client.until_play(play_cb::PLAYER_POSITION, &mut seen);
    // Teleport id 1, then x, y (on the stone at y = 3), z.
    assert_eq!(position[1], 1);
    let xyz: Vec<f64> = (0..3)
        .map(|i| f64::from_be_bytes(position[2 + 8 * i..10 + 8 * i].try_into().unwrap()))
        .collect();
    assert_eq!(xyz, [8.5, 4.0, 8.5]);
    assert!(seen.iter().any(|p| p[0] == play_cb::COMMANDS));
    // Send distance min(load - 1, view distance + 1) = 2: the 3x3 chunks whose neighbours are
    // all on disk, nearest first.
    let mut chunks = Vec::new();
    while chunks.len() < 9 {
        let packet = client.until_play(play_cb::LEVEL_CHUNK_WITH_LIGHT, &mut seen);
        assert!(
            packet.windows(2048).any(|w| w.iter().all(|&b| b == 0xff)),
            "the saved sky light"
        );
        chunks.push(chunk_pos(&packet));
    }
    assert_eq!(chunks[0], (0, 0));
    let mut sorted = chunks.clone();
    sorted.sort();
    let expected: Vec<(i32, i32)> = (-1..=1)
        .flat_map(|x| (-1..=1).map(move |z| (x, z)))
        .collect();
    assert_eq!(sorted, expected);

    // Moves before the teleport is accepted are ignored; after it, a new chunk re-centres.
    let mut accept = vec![1];
    for v in [8.5f64, 4.0, 8.5] {
        accept.extend(v.to_be_bytes());
    }
    accept.extend([0; 8]);
    client.send_play(play_sb::ACCEPT_TELEPORTATION, &accept);
    client.move_to(24.5, 4.0, 8.5);
    let center = client.until_play(play_cb::SET_CHUNK_CACHE_CENTER, &mut seen);
    assert_eq!(&center[1..], &[1, 0]);
    // Three chunks further: the chunks at x = -1 and 0 leave the send square, forgotten first.
    seen.clear();
    client.move_to(72.5, 4.0, 8.5);
    let center = client.until_play(play_cb::SET_CHUNK_CACHE_CENTER, &mut seen);
    assert_eq!(&center[1..], &[4, 0]);
    let forgotten: Vec<ChunkPos> = seen
        .iter()
        .filter(|p| p[0] == play_cb::FORGET_LEVEL_CHUNK)
        .map(|p| ChunkPos::unpack(i64::from_be_bytes(p[1..9].try_into().unwrap())))
        .collect();
    assert_eq!(forgotten.len(), 6, "{forgotten:?}");
    assert!(forgotten.iter().all(|p| p.x <= 0));

    drop(client);
    let (code, dir) = server.stop(Control::Console("stop".into()));
    assert_eq!(code, 0);
    let players = PlayerDataStorage::new(&dir.path().join("world")).unwrap();
    let saved = players.load(NOTCH, "Notch").expect("the player's file");
    assert_eq!(saved.position, Some([72.5, 4.0, 8.5]));
    assert_eq!(saved.dimension.as_deref(), Some("minecraft:overworld"));
    assert_eq!(saved.tag.get(&"DataVersion".into()).map(Tag::id), Some(3));
}

#[test]
fn join_then_stop_from_the_console() {
    let server = run(config(20));
    let mut client = Client::join(server.addr);
    let (code, _) = server.stop(Control::Console("/stop".into()));
    assert_eq!(code, 0);
    // Configuration connections are closed without a disconnect packet (`handleAllDisconnections`).
    while let Some(packet) = client.recv() {
        assert_ne!(packet[0], 2, "no disconnect");
    }
}

#[test]
fn a_full_server_refuses() {
    let server = run(config(0));
    let (mut client, refusal) = Client::log_in(server.addr);
    assert_eq!(
        refusal,
        login::disconnect_packet(&Reason::translatable("multiplayer.disconnect.server_full"))
    );
    assert_eq!(client.recv(), None);
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)).0, 130);
}

const LISTED: &str = r#"[{"uuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5","name":"Notch"}]"#;

fn whitelist_on() -> RuntimeConfig {
    RuntimeConfig {
        white_list: true,
        ..config(20)
    }
}

/// A refused login: the login disconnect with this reason, then the connection closes.
fn assert_refused(server: Running, reason: &Reason) {
    let (mut client, refusal) = Client::log_in(server.addr);
    assert_eq!(refusal, login::disconnect_packet(reason));
    assert_eq!(client.recv(), None);
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)).0, 130);
}

/// `white-list=true` with no `whitelist.json` (Leaf's default): nobody gets in.
#[test]
fn an_empty_whitelist_refuses() {
    let server = run(whitelist_on());
    let file = server.dir.path().join("whitelist.json");
    assert_eq!(std::fs::read_to_string(file).unwrap(), "[]");
    let message = Reason::Literal("You are not whitelisted on this server!".into());
    assert_refused(server, &message);
}

#[test]
fn a_whitelisted_player_joins() {
    let server = run_with(whitelist_on(), |dir| {
        std::fs::write(dir.join("whitelist.json"), LISTED).unwrap();
    });
    let mut client = Client::play(server.addr);
    client.until_play(play_cb::LOGIN, &mut Vec::new());
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)).0, 130);
}

#[test]
fn the_whitelist_message_is_spigots() {
    let server = run(RuntimeConfig {
        whitelist_message: "Ask the owner".into(),
        ..whitelist_on()
    });
    assert_refused(server, &Reason::Literal("Ask the owner".into()));
}

#[test]
fn the_whitelist_message_with_codes_is_a_styled_component() {
    let server = run(RuntimeConfig {
        whitelist_message: "§cApply on Discord".into(),
        ..whitelist_on()
    });
    let expected = serde_json::json!({"text": "Apply on Discord", "color": "red"});
    assert_refused(server, &Reason::Component(expected));
}

/// Bans are gone with MineComplex's cleanup: the files are neither made nor read.
#[test]
fn ban_files_are_ignored() {
    let ban =
        r#"[{"uuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5","name":"Notch","expires":"forever"}]"#;
    let server = run_with(config(20), |dir| {
        std::fs::write(dir.join("banned-players.json"), ban).unwrap();
        std::fs::write(dir.join("banned-ips.json"), ban).unwrap();
    });
    let mut client = Client::play(server.addr);
    client.until_play(play_cb::LOGIN, &mut Vec::new());
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)).0, 130);
}

/// An operator is let past the whitelist and, with `bypassesPlayerLimit`, a full server; the client
/// is told its level (`entity_event` 24 + level).
#[test]
fn an_operator_gets_in_and_its_level() {
    let ops = r#"[{"uuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5","name":"Notch","level":3,"bypassesPlayerLimit":true}]"#;
    let full_with_whitelist = RuntimeConfig {
        white_list: true,
        ..config(0)
    };
    let server = run_with(full_with_whitelist, |dir| {
        std::fs::write(dir.join("ops.json"), ops).unwrap();
    });
    let mut client = Client::play(server.addr);
    let mut seen = Vec::new();
    client.until_play(play_cb::LOGIN, &mut seen);
    let event = client.until_play(play_cb::ENTITY_EVENT, &mut seen);
    assert_eq!(event[5], 24 + 3);
    assert_eq!(server.stop(Control::Signal(Signal::Interrupt)).0, 130);
}

#[test]
fn a_duplicate_login_kicks_the_old_player() {
    let server = run(config(20));
    let mut first = Client::play(server.addr);
    let mut seen = Vec::new();
    first.until_play(play_cb::LOGIN, &mut seen);
    let mut second = Client::play(server.addr);
    let kick = first.until_play(play_cb::DISCONNECT, &mut seen);
    assert_eq!(
        component(&kick),
        listener::reason_component(&Reason::translatable(
            "multiplayer.disconnect.duplicate_login"
        ))
    );
    assert_eq!(first.recv(), None);
    second.until_play(play_cb::LOGIN, &mut seen);
    // On stop, players get the shutdown message.
    assert_eq!(server.stop(Control::Signal(Signal::Terminate)).0, 143);
    let message = second.until_play(play_cb::DISCONNECT, &mut seen);
    assert_eq!(
        component(&message),
        FormattedText::from("Сервер перезапускается")
    );
    assert_eq!(second.recv(), None);
}

/// `handleKeepAlive`: an answer to no keep-alive is a timeout.
#[test]
fn a_wrong_keep_alive_times_out() {
    let server = run(config(20));
    let mut client = Client::join(server.addr);
    client.send(&[4, 0, 0, 0, 0, 0, 0, 0, 7]);
    let kick = client.until(2, &mut Vec::new()).unwrap();
    assert_eq!(
        component(&kick),
        listener::reason_component(&Reason::translatable("disconnect.timeout"))
    );
    assert_eq!(client.recv(), None);
    assert_eq!(server.stop(Control::Console("stop".into())).0, 0);
}

#[test]
fn a_taken_port_fails_to_start() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config(20);
    config.server_port = i32::from(taken.local_addr().unwrap().port());
    let started = Server::start(
        config,
        Arc::new(Registries::build(Vec::new(), &[]).unwrap()),
        || unreachable!("the world opens after the port is bound"),
    );
    assert!(matches!(started, Err(StartError::Bind(_))));
}

/// Ticks the server (sleeping between ticks, the client being on another thread) until `done`.
fn tick_until(server: &mut Server, what: &str, mut done: impl FnMut(&mut Server) -> bool) {
    for _ in 0..5000 {
        server.tick();
        if done(server) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("never: {what}");
}

/// A block of the chunk at `pos` as the region file has it.
fn saved_block(dir: &Path, dimension: &str, pos: BlockPos) -> BlockState {
    let tables = tables();
    let codec = CodecTables {
        factory: ContainerFactory::new(4, 1, 0),
        blocks: tables.blocks.clone(),
        flags: Arc::new(Flags),
        biomes: Arc::new(BiomeIds::new(["minecraft:plains"])),
        min_section_y: 0,
        max_section_y: 1,
    };
    let chunk_pos = ChunkPos::new(pos.x >> 4, pos.z >> 4);
    let folder = LevelStorageAccess::open(dir, "world")
        .unwrap()
        .region_path(dimension, "region");
    let mut regions = crate::world::chunk_io::RegionStorage::new(folder, Compression::Lz4, false);
    let tag = regions.read(chunk_pos).unwrap().expect("the chunk on disk");
    let data = codec.codec().read(&tag).unwrap().expect("a chunk");
    let heightmaps = HeightmapTable::new(4, |s| if s.0 == 0 { 0 } else { 0x3f });
    LevelChunk::from_data(data, 2, &codec.factory, &heightmaps).block_state(pos)
}

/// Moonrise's chunk holding end to end: a changed chunk is saved by autosave while loaded, the
/// player walks away, the chunks it left are unloaded after the delay and the changed ones saved,
/// `level.dat` is saved every `autosavePeriod` ticks, and everything else on stop.
#[test]
fn chunks_are_autosaved_unloaded_and_saved_as_the_player_walks_away() {
    let mut config = config(20);
    config.autosave_period = 50;
    let settings = ChunkSettings {
        auto_save_interval: 20,
        delay_chunk_unloads_by: 20,
        ..ChunkSettings::default()
    };
    let (mut server, dir) = start(config, settings);
    let addr = server.local_addr();
    let (go, wait) = std_mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let mut client = Client::play(addr);
        let mut seen = Vec::new();
        client.until_play(play_cb::PLAYER_POSITION, &mut seen);
        let mut accept = vec![1];
        for v in [8.5f64, 4.0, 8.5] {
            accept.extend(v.to_be_bytes());
        }
        accept.extend([0; 8]);
        client.send_play(play_sb::ACCEPT_TELEPORTATION, &accept);
        for _ in 0..9 {
            client.until_play(play_cb::LEVEL_CHUNK_WITH_LIGHT, &mut seen);
        }
        wait.recv().unwrap();
        // Eight chunks east: every chunk on disk is out of the load area.
        client.move_to(136.5, 4.0, 8.5);
        seen.clear();
        // Until the server stops: keep-alives answered, packets counted.
        while let Some(packet) = client.recv() {
            if packet[0] == play_cb::KEEP_ALIVE {
                client.send_play(play_sb::KEEP_ALIVE, &packet[1..]);
            }
            seen.push(packet);
        }
        seen.iter()
            .filter(|p| p[0] == play_cb::FORGET_LEVEL_CHUNK)
            .count()
    });

    tick_until(&mut server, "nine chunks sent", |s| {
        s.players
            .first()
            .is_some_and(|j| j.player.sent_chunks() == 9)
    });
    let heightmaps = HeightmapTable::new(4, |s| if s.0 == 0 { 0 } else { 0x3f });
    let change = |s: &mut Server, pos: BlockPos| {
        let chunk = s.world.levels[0]
            .chunks
            .chunk_mut(ChunkPos::new(pos.x >> 4, pos.z >> 4))
            .unwrap();
        assert!(
            chunk
                .set_block_state(pos, STONE, &Flags, &heightmaps)
                .is_some()
        );
        assert!(chunk.is_unsaved());
    };
    // Autosave: a changed chunk is saved within `auto-save-interval` ticks, staying loaded.
    let center = BlockPos::new(5, 10, 5);
    change(&mut server, center);
    let mut ticks = 0;
    tick_until(&mut server, "the autosave", |s| {
        ticks += 1;
        !s.world.levels[0]
            .chunk(ChunkPos::new(0, 0))
            .unwrap()
            .is_unsaved()
    });
    assert!(ticks <= 21, "{ticks} ticks");
    // Two more changes, then the player leaves them behind.
    let corners = [BlockPos::new(-27, 10, -27), BlockPos::new(37, 11, 37)];
    for pos in corners {
        change(&mut server, pos);
    }
    go.send(()).unwrap();
    tick_until(&mut server, "the player moved", |s| {
        s.players[0].player.position[0] == 136.5
    });
    let held = |s: &Server| {
        (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| ChunkPos::new(x, z)))
            .filter(|&p| s.world.levels[0].chunk(p).is_some())
            .count()
    };
    assert_eq!(held(&server), 25);
    // The delayed player tickets (20 ticks), `PLAYER_JOIN` (100 ticks from the join), then the
    // unloads: every chunk on disk goes.
    tick_until(&mut server, "the chunks unloaded", |s| held(s) == 0);
    tick_until(&mut server, "a level.dat autosave", |s| {
        s.tick_count % 50 == 1
    });
    assert!(dir.path().join("world/level.dat_old").exists());
    // The saves were sent to the region workers; they are on disk once the server stopped.
    assert_eq!(server.stop(), 0);
    for pos in corners {
        assert_eq!(saved_block(dir.path(), OVERWORLD, pos), STONE, "{pos:?}");
    }
    assert_eq!(saved_block(dir.path(), OVERWORLD, center), STONE);
    assert_eq!(
        saved_block(dir.path(), OVERWORLD, BlockPos::new(-20, 10, 20)),
        BlockState(0)
    );
    // The 9 chunks sent were forgotten when the player moved.
    assert_eq!(client.join().unwrap(), 9);
}

/// How often the packet holds this text.
fn occurrences(packet: &[u8], text: &str) -> usize {
    packet
        .windows(text.len())
        .filter(|w| *w == text.as_bytes())
        .count()
}

/// Writes the player file of Notch before the server starts.
fn save_notch(dir: &Path, dimension: &str, position: [f64; 3]) {
    let players = PlayerDataStorage::new(&dir.join("world")).unwrap();
    let saved = SavedPlayer {
        dimension: Some(dimension.to_owned()),
        position: Some(position),
        rotation: Some((0.0, 0.0)),
        ..SavedPlayer::default()
    };
    players.save(NOTCH, "Notch", &saved);
}

#[test]
fn the_server_has_a_level_per_stem_with_its_dimension_type() {
    let (server, _dir) = start(config(20), ChunkSettings::default());
    let levels = &server.world.levels;
    let names: Vec<_> = levels.iter().map(|l| l.dimension.as_str()).collect();
    assert_eq!(
        names,
        [
            "minecraft:overworld",
            "minecraft:the_nether",
            "minecraft:the_end"
        ]
    );
    let worlds: Vec<_> = levels.iter().map(|l| l.world_name.as_str()).collect();
    assert_eq!(worlds, ["world", "world_nether", "world_the_end"]);
    assert_eq!(
        levels.iter().map(|l| l.section_count).collect::<Vec<_>>(),
        [2, 2, 4],
        "the heights of the synced dimension types"
    );
    assert_eq!(
        levels.iter().map(|l| l.sea_level).collect::<Vec<_>>(),
        [63, 63, 63],
        "no generator settings in a world without world_gen_settings"
    );
    let types: Vec<_> = levels.iter().map(|l| l.dimension_type).collect();
    assert!(types[0] != types[1] && types[1] != types[2]);
}

/// A player saved in the nether joins there: the login names the nether, the chunks are the
/// nether's, a stop saves the changed chunks of the nether (and only those).
#[test]
fn a_player_saved_in_the_nether_joins_the_nether_and_stop_saves_every_level() {
    let (mut server, dir) = start_with(config(20), ChunkSettings::default(), |dir| {
        save_notch(dir, NETHER, [8.5, 6.0, 8.5])
    });
    let addr = server.local_addr();
    let client = std::thread::spawn(move || {
        let mut client = Client::play(addr);
        let mut seen = Vec::new();
        let login = client.until_play(play_cb::LOGIN, &mut seen);
        let mut lights = Vec::new();
        while lights.len() < 9 {
            let packet = client.until_play(play_cb::LEVEL_CHUNK_WITH_LIGHT, &mut seen);
            lights.push(packet.windows(2048).any(|w| w.iter().all(|&b| b == 0x77)));
        }
        // Until the server stops.
        while client.recv().is_some() {}
        (login, lights)
    });
    tick_until(&mut server, "nine chunks sent", |s| {
        s.players
            .first()
            .is_some_and(|j| j.player.sent_chunks() == 9)
    });
    assert_eq!(server.players[0].player.level, 1, "the nether");
    let heightmaps = HeightmapTable::new(4, |s| if s.0 == 0 { 0 } else { 0x3f });
    let pos = BlockPos::new(5, 10, 5);
    let chunk = server.world.levels[1]
        .chunks
        .chunk_mut(ChunkPos::new(0, 0))
        .expect("the nether's chunk is held");
    assert!(
        chunk
            .set_block_state(pos, STONE, &Flags, &heightmaps)
            .is_some()
    );
    assert!(
        server.world.levels[0].chunk(ChunkPos::new(0, 0)).is_none(),
        "the overworld holds nothing for a player in the nether"
    );
    assert_eq!(server.stop(), 0);
    let (login, lights) = client.join().unwrap();
    assert_eq!(
        occurrences(&login, "minecraft:the_nether"),
        2,
        "the list, the spawn"
    );
    assert_eq!(occurrences(&login, "minecraft:overworld"), 1, "the list");
    assert!(lights.iter().all(|&nether| nether), "the nether's chunks");
    assert_eq!(saved_block(dir.path(), NETHER, pos), STONE);
    let players = PlayerDataStorage::new(&dir.path().join("world")).unwrap();
    let saved = players.load(NOTCH, "Notch").unwrap();
    assert_eq!(saved.dimension.as_deref(), Some(NETHER));
}

/// A saved dimension the server has no level for: Paper's "unknown respawn dimension", the level of
/// the respawn data (the overworld here), keeping the saved position.
#[test]
fn a_player_saved_in_a_missing_level_joins_the_respawn_level() {
    let server = run_with(config(20), |dir| {
        save_notch(dir, "minecraft:missing", [8.5, 4.0, 8.5]);
    });
    let mut client = Client::play(server.addr);
    let mut seen = Vec::new();
    let login = client.until_play(play_cb::LOGIN, &mut seen);
    assert_eq!(occurrences(&login, "minecraft:overworld"), 2);
    assert_eq!(occurrences(&login, "minecraft:missing"), 0);
    let chunk = client.until_play(play_cb::LEVEL_CHUNK_WITH_LIGHT, &mut seen);
    assert!(chunk.windows(2048).any(|w| w.iter().all(|&b| b == 0xff)));
    drop(client);
    let (code, dir) = server.stop(Control::Console("stop".into()));
    assert_eq!(code, 0);
    let players = PlayerDataStorage::new(&dir.path().join("world")).unwrap();
    let saved = players.load(NOTCH, "Notch").unwrap();
    assert_eq!(saved.dimension.as_deref(), Some(OVERWORLD));
}

/// The real binary, end to end: run `horizon-server` on a copy of a world made by vanilla 26.3 with
/// Velocity forwarding on (`config/paper-global.yml` `proxies.velocity.secret: s3cret`,
/// `online-mode=false`), then
/// `HORIZON_SERVER_ADDR=127.0.0.1:<port> cargo test -p horizon-server real_binary_session -- --ignored --nocapture`.
/// A player joins, accepts the spawn teleport, receives chunks, walks 12 chunks east (the chunks
/// behind it are forgotten, and unloaded and saved by the server after Paper's delay) and leaves;
/// the server's log and files are checked by hand (`tools/leaf_server/README.md`).
#[test]
#[ignore = "needs a running horizon-server (HORIZON_SERVER_ADDR)"]
fn real_binary_session() {
    let addr: SocketAddr = std::env::var("HORIZON_SERVER_ADDR")
        .expect("HORIZON_SERVER_ADDR")
        .parse()
        .unwrap();
    let mut client = Client::play(addr);
    let mut seen = Vec::new();
    client.until_play(play_cb::LOGIN, &mut seen);
    let position = client.until_play(play_cb::PLAYER_POSITION, &mut seen);
    let (teleport, n) = crate::network::codec::read_var_int(&position[1..])
        .unwrap()
        .unwrap();
    let at =
        |i: usize| f64::from_be_bytes(position[1 + n + 8 * i..9 + n + 8 * i].try_into().unwrap());
    let (x, y, z) = (at(0), at(1), at(2));
    let mut accept = Vec::new();
    put_var_int(&mut accept, teleport);
    for v in [x, y, z] {
        accept.extend(v.to_be_bytes());
    }
    accept.extend([0; 8]);
    client.send_play(play_sb::ACCEPT_TELEPORTATION, &accept);
    let mut chunks = seen
        .iter()
        .filter(|p| p[0] == play_cb::LEVEL_CHUNK_WITH_LIGHT)
        .count();
    while chunks < 9 {
        client.until_play(play_cb::LEVEL_CHUNK_WITH_LIGHT, &mut seen);
        chunks += 1;
    }
    eprintln!("spawned at {x} {y} {z} (teleport {teleport}), {chunks} chunks so far");
    let mut forgotten = 0;
    for step in 1..=12 {
        seen.clear();
        client.move_to(x + 16.0 * f64::from(step), y, z);
        client.until_play(play_cb::SET_CHUNK_CACHE_CENTER, &mut seen);
        forgotten += seen
            .iter()
            .filter(|p| p[0] == play_cb::FORGET_LEVEL_CHUNK)
            .count();
        chunks += seen
            .iter()
            .filter(|p| p[0] == play_cb::LEVEL_CHUNK_WITH_LIGHT)
            .count();
    }
    // Some time in the new place, answering keep-alives, before leaving.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let packet = client.recv().expect("still connected");
        if packet[0] == play_cb::KEEP_ALIVE {
            client.send_play(play_sb::KEEP_ALIVE, &packet[1..]);
        }
    }
    eprintln!("walked 12 chunks east: {chunks} chunks received, {forgotten} forgotten");
    assert!(forgotten > 0, "chunks behind the player are forgotten");
}
