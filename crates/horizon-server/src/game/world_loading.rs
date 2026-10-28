//! Prepare local worlds without blocking the window's event loop.
use super::*;

pub(super) struct WorldRequest {
    pub save_root: PathBuf,
    pub folder: String,
    pub name: Option<String>,
    pub seed: Option<i64>,
    pub creative: bool,
    pub create: Option<crate::hc::title::WorldOptions>,
}

pub(super) struct PreparedWorld {
    pub save_root: PathBuf,
    pub level: Level,
    pub world: World,
    pub streaming: Streaming,
    pub save: Arc<SaveDir>,
    pub session_lock: crate::hc::save::SessionLock,
    pub rand_sequences: crate::hc::random_sequences::RandomSequences,
    pub locator: Option<Arc<VanillaTerrain>>,
}

impl WorldRequest {
    pub fn new(
        folder: &str,
        name: Option<&str>,
        seed: Option<i64>,
        creative: bool,
        create: Option<&crate::hc::title::WorldOptions>,
    ) -> Result<Self> {
        Ok(Self {
            save_root: std::env::current_dir()?.join("saves").join(folder),
            folder: folder.to_owned(),
            name: name.map(str::to_owned),
            seed,
            creative,
            create: create.cloned(),
        })
    }

    pub fn prepare(
        self,
        res: &Resources,
        pack: &PackResources,
        options: &Options,
        settings: &Settings,
    ) -> Result<PreparedWorld> {
        // Before any chunk is read: a loaded container with a loot table is rolled as its chunk
        // is decoded (`save::roll_loaded_loot`), and the spawn chunks are read while the world
        // opens. Every world opens here: in-frame, on the server thread, on a dedicated server.
        crate::hc::structure_blocks::loot_data(&options.resources);
        let Self {
            save_root,
            folder,
            name,
            seed,
            creative,
            create,
        } = self;
        let name = name.as_deref();
        let create = create.as_ref();
        crate::hc::save::upgrade_older_world(&save_root)?;
        crate::hc::save::migrate_legacy(&save_root)?;
        let session_lock = crate::hc::save::SessionLock::acquire(&save_root)?;
        // RandomSequences: the world's data/minecraft/random_sequences.dat.
        let rand_sequences =
            crate::hc::save::read_dat(&save_root.join("data/minecraft/random_sequences.dat"))
                .and_then(|t| t.get("data").cloned())
                .map(|d| crate::hc::random_sequences::RandomSequences::from_nbt(&d))
                .unwrap_or_default();
        let existing = Level::read(&save_root);
        let created = existing.is_none();
        let level = existing.unwrap_or_else(|| Level {
            name: name.unwrap_or(&folder).to_owned(),
            // WorldOptions.randomSeed when no seed was given.
            seed: seed.unwrap_or_else(|| {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64);
                (t ^ (t >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15) as i64
            }),
            creative,
            day_time: 0,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            ..Level::default()
        });
        let mut level = level;
        // CreateWorldScreen's choices, before streaming reads the generator.
        if created && let Some(o) = create {
            level.hardcore = o.hardcore;
            level.difficulty = crate::hc::title::DIFFICULTIES[o.difficulty()].into();
            level.allow_commands = Some(o.commands(creative));
            crate::hc::save::write_world_gen_settings_with(
                &save_root,
                level.seed,
                o.world_gen(),
                o.flat_settings(),
            )?;
            // WorldCreationUiState.getGameRules, written with the level.
            level.game_rules = o.rules.by_name();
        }
        // Worlds made without the Create New World screen (--world, the
        // control socket) take its default: commands on for Creative only.
        if created {
            level.allow_commands.get_or_insert(creative);
        }
        if let Some(d) = &options.dimension {
            level.dimension = d.clone();
            level.position = None;
        } else if options.dedicated.is_some() {
            // A server opens on the Overworld (MinecraftServer.overworld), whatever level a
            // single-player save or an older Horizon server wrote into level.dat.
            level.dimension = "minecraft:overworld".to_owned();
        }
        // A server's world has no single player whose file to write.
        level.server = options.dedicated.is_some();
        let dim = Dim::from_id(&level.dimension);
        let (mut streaming, save, locator) =
            make_streaming(res, pack, options, settings, &save_root, dim, level.seed)?;
        streaming.radius = settings.render_distance;
        let mut world = World::new(res.facts().air, level.seed);
        world.fast_lava = dim == Dim::Nether;
        world.game_time = level.time;
        world.day_time = options.time.unwrap_or(level.day_time);
        world.weather = level.weather;
        if let Some(w) = options.weather.as_deref() {
            // /weather: clear, rain or thunder for a long duration.
            let (rain, thunder) = (w != "clear", w == "thunder");
            world.weather = crate::hc::world::Weather {
                clear_time: if w == "clear" { 6000 } else { 0 },
                rain_time: 6000,
                thunder_time: 6000,
                raining: rain,
                thundering: thunder,
                rain_level: if rain { (1.0, 1.0) } else { (0.0, 0.0) },
                thunder_level: if thunder { (1.0, 1.0) } else { (0.0, 0.0) },
            };
        }

        growth::install_live_features(res, pack, options);

        // Persist new metadata on the worker too, before publishing the world.
        if !save_root.join("level.dat").exists() {
            level.write(&save_root)?;
        }
        Ok(PreparedWorld {
            save_root,
            level,
            world,
            streaming,
            save,
            session_lock,
            rand_sequences,
            locator,
        })
    }
}

pub(super) fn make_streaming(
    res: &Resources,
    pack: &PackResources,
    options: &Options,
    settings: &Settings,
    world_root: &std::path::Path,
    dim: Dim,
    seed: i64,
) -> Result<(Streaming, Arc<SaveDir>, Option<Arc<VanillaTerrain>>)> {
    let save = Arc::new(SaveDir::new(crate::hc::save::dimension_dir(
        world_root,
        dim.id(),
    )));
    let mut locator = None;
    let flat = (dim == Dim::Overworld)
        .then(|| crate::hc::save::world_gen_settings(world_root))
        .flatten()
        .and_then(|s| {
            crate::hc::flatgen::FlatTerrain::from_settings(
                &s,
                pack.assets.registry,
                res.biomes(),
            )
        });
    let mut streaming = if let Some(inner) = flat {
        Streaming::new(
            Arc::new(Persistent {
                inner: Arc::new(inner),
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    } else if options.test_terrain && dim == Dim::Overworld {
        let inner = Arc::new(TestTerrain::new(pack.assets.registry, res.biomes())?);
        Streaming::new(
            Arc::new(Persistent {
                inner,
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    } else {
        // WorldOptions.generateStructures, from the world's settings.
        let structures =
            crate::hc::save::WorldGen::read(world_root).is_none_or(|w| w.structures);
        let inner = Arc::new(VanillaTerrain::for_world(&options.resources.join("worldgen"), pack.assets.registry, res.biomes(), seed, dim.id(), structures).map_err(|e| {
                format!("{e}\nPrepare the worldgen bundle with the launcher, or: horizon-prepare --prepare")
            })?);
        if dim == Dim::Overworld && structures {
            inner.search_rings_behind();
        }
        locator = Some(inner.clone());
        Streaming::new(
            Arc::new(Persistent {
                inner,
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    };
    streaming.forced = save.load_forced_chunks();
    streaming.radius = settings.render_distance;
    streaming.biome_blend = settings.biome_blend;
    streaming.smooth_lighting = settings.smooth_lighting;
    streaming.cutout_leaves = settings.cutout_leaves;
    streaming.biome_zoom = BiomeZoom::new(obfuscate_seed(seed));
    streaming.sky_light = dim != Dim::Nether;
    streaming.nether_shading = dim == Dim::Nether;
    // A server draws nothing: neither the integrated server nor the
    // dedicated one meshes sections.
    streaming.mesh = options.execution_role.is_client();
    Ok((streaming, save, locator))
}

