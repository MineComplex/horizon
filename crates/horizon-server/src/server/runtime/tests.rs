//! The server over real TCP: a blocking client logs in through a forged Velocity answer while the
//! server runs on its own thread, over a small synthetic world in a temporary folder.
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::Path;
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
use crate::server::level::{WorldSettings, WorldTables};
use crate::server::player_data::PlayerDataStorage;
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
/// overworld with one biome.
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
    let (io, events) = ChunkIo::spawn(
        access.region_path("minecraft:overworld", "region"),
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
                chunk.set_block_state(BlockPos::new(bx, 3, bz), STONE, &Flags, &heightmaps);
            }
            let mut data = chunk.to_data();
            data.light_correct = true;
            let section = data.sections.iter_mut().find(|s| s.y == 0).unwrap();
            section.sky_light = Some(Box::new([0xff; 2048]));
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

struct Running {
    addr: SocketAddr,
    control: std_mpsc::Sender<Control>,
    thread: JoinHandle<u8>,
    dir: tempfile::TempDir,
}

/// The server over the synthetic world, not running yet.
fn start(config: RuntimeConfig, chunks: ChunkSettings) -> (Server, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
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
    let (server, dir) = start(config, ChunkSettings::default());
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
fn obfuscated_seed_is_guava_sha256() {
    // `Hashing.sha256().hashLong(seed).asLong()` from Guava 33.6 (`BiomeManager.obfuscateSeed`).
    assert_eq!(obfuscate_seed(0), 8_794_265_229_978_523_055);
    assert_eq!(obfuscate_seed(12345), 293_737_985_876_514_017);
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
fn saved_block(dir: &Path, pos: BlockPos) -> BlockState {
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
        .region_path("minecraft:overworld", "region");
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
        let chunk = s
            .world
            .overworld
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
        !s.world
            .overworld
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
            .filter(|&p| s.world.overworld.chunk(p).is_some())
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
        assert_eq!(saved_block(dir.path(), pos), STONE, "{pos:?}");
    }
    assert_eq!(saved_block(dir.path(), center), STONE);
    assert_eq!(
        saved_block(dir.path(), BlockPos::new(-20, 10, 20)),
        BlockState(0)
    );
    // The 9 chunks sent were forgotten when the player moved.
    assert_eq!(client.join().unwrap(), 9);
}
