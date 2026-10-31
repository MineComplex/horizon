use super::*;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn missing_files_are_written_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();

    let spigot = load_spigot(&d.join("spigot.yml")).unwrap();
    assert_eq!((spigot.tab_complete, spigot.send_namespaced), (0, false));
    assert_eq!(
        spigot.world_seeds("minecraft:overworld").unwrap(),
        SpigotWorldSeeds::default()
    );
    let text = read(&d.join("spigot.yml"));
    assert!(
        text.starts_with("# This is the Spigot configuration file for Paper.\n"),
        "{text}"
    );
    assert!(text.contains("\n# with caution, and make sure you know what each option does before configuring.\n#\n# If you need help"));
    assert!(text.contains("# Website: https://papermc.io/\n\nconfig-version: 13\ncommands:\n  tab-complete: 0\n  send-namespaced: false\nworld-settings:\n  default:\n    seed-village: 10387312\n"), "{text}");
    assert!(text.ends_with("    seed-abandonedcamp: 91231127\n    seed-buriedtreasure: 10387320\n    seed-mineshaft: default\n    seed-stronghold: default\n"), "{text}");

    let purpur = load_purpur(&d.join("purpur.yml")).unwrap();
    assert_eq!(
        purpur,
        PurpurConfig {
            verbose: false,
            lagging_threshold: 19.0,
            generate_end_void_rings: false
        }
    );
    let text = read(&d.join("purpur.yml"));
    assert!(text.ends_with("# Docs: https://purpurmc.org/docs \n\nverbose: false\nconfig-version: 49\nsettings:\n  lagging-threshold: 19.0\n  generate-end-void-rings: false\n"), "{text}");

    let paper = load_paper_global(&d.join("config"), None).unwrap();
    assert_eq!(
        paper,
        PaperGlobalConfig {
            velocity: VelocityConfig {
                enabled: false,
                online_mode: true,
                secret: String::new()
            },
            catchup_ticks: None
        }
    );
    let text = read(&d.join("config/paper-global.yml"));
    assert!(
        text.starts_with("# This is the global configuration file for Paper.\n"),
        "{text}"
    );
    assert!(text.contains("before configuring.\n# \n# If you need help"));
    assert!(text.ends_with("# Website: https://papermc.io/\n\n_version: 31\nmisc:\n  catch-up-ticks: default\nproxies:\n  velocity:\n    enabled: false\n    online-mode: true\n    secret: ''\n"), "{text}");

    assert!(load_leaf_global(&d.join("config/leaf-global.yml")).including_5s_in_get_tps);
    let text = read(&d.join("config/leaf-global.yml"));
    assert!(
        text.starts_with("# Leaf Config\n# \n# Website: https://www.leafmc.one/\n"),
        "{text}"
    );
    assert!(text.ends_with("config-version: '3.0'\nasync: {}\nperformance: {}\nfixes: {}\ngameplay-mechanisms: {}\nnetwork: {}\nmisc:\n  including-5s-in-get-tps: true\n"), "{text}");

    // A second start reads its own files back unchanged.
    for name in [
        "spigot.yml",
        "purpur.yml",
        "config/paper-global.yml",
        "config/leaf-global.yml",
    ] {
        let before = read(&d.join(name));
        load_spigot(&d.join("spigot.yml")).unwrap();
        load_purpur(&d.join("purpur.yml")).unwrap();
        load_paper_global(&d.join("config"), None).unwrap();
        load_leaf_global(&d.join("config/leaf-global.yml"));
        assert_eq!(read(&d.join(name)), before, "{name}");
    }
}

#[test]
fn bukkit_files_keep_what_they_do_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spigot.yml");
    // An old file: `tab-complete` was a boolean before version 6. Wrong types read as defaults.
    std::fs::write(&path, "# mine\nconfig-version: 5\nsettings:\n  bungeecord: yes\ncommands:\n  tab-complete: off\n  send-namespaced: 'true'\n").unwrap();
    let spigot = load_spigot(&path).unwrap();
    assert_eq!((spigot.tab_complete, spigot.send_namespaced), (-1, false));
    let text = read(&path);
    assert!(
        text.split_once("\n\n").unwrap().1.starts_with(
            "config-version: 13\nsettings:\n  bungeecord: true\ncommands:\n  tab-complete: -1\n  send-namespaced: 'true'\nworld-settings:\n"
        ),
        "{text}"
    );

    let path = dir.path().join("purpur.yml");
    std::fs::write(
        &path,
        "verbose: on\nsettings:\n  lagging-threshold: 1_8.5\n  generate-end-void-rings: true\n",
    )
    .unwrap();
    assert_eq!(
        load_purpur(&path).unwrap(),
        PurpurConfig {
            verbose: true,
            lagging_threshold: 18.5,
            generate_end_void_rings: true
        }
    );
    std::fs::write(&path, "settings:\n  lagging-threshold: fast\n").unwrap();
    assert_eq!(load_purpur(&path).unwrap().lagging_threshold, 19.0);

    std::fs::write(&path, "settings: [\n").unwrap();
    let err = load_purpur(&path).unwrap_err();
    assert!(
        err.to_string()
            .starts_with("Could not load purpur.yml, please correct your syntax errors"),
        "{err}"
    );
}

#[test]
fn paper_values_are_coerced_and_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("paper-global.yml");
    std::fs::write(&path, "proxies:\n  velocity:\n    enabled: yes\n    online-mode: 'n'\n    secret: 12345\n  proxy-protocol: true\nmisc:\n  catch-up-ticks: 3\n").unwrap();
    let paper = load_paper_global(dir.path(), None).unwrap();
    assert_eq!(
        paper.velocity,
        VelocityConfig {
            enabled: true,
            online_mode: false,
            secret: "12345".into()
        }
    );
    assert_eq!(paper.catchup_ticks, Some(3));
    let text = read(&path);
    // No `_version`: assumed the latest, and added.
    assert!(text.ends_with("\n\n_version: 31\nmisc:\n  catch-up-ticks: 3\nproxies:\n  proxy-protocol: true\n  velocity:\n    enabled: true\n    online-mode: false\n    secret: '12345'\n"), "{text}");

    // Enabled without a secret: disabled, and written back disabled.
    std::fs::write(&path, "proxies:\n  velocity:\n    enabled: true\n").unwrap();
    let paper = load_paper_global(dir.path(), None).unwrap();
    assert!(!paper.velocity.enabled);
    assert!(read(&path).contains("    enabled: false\n"));
    // The environment's secret replaces the file's while Velocity is enabled.
    std::fs::write(
        &path,
        "proxies:\n  velocity:\n    enabled: true\n    secret: file\n",
    )
    .unwrap();
    assert_eq!(
        load_paper_global(dir.path(), Some("env"))
            .unwrap()
            .velocity
            .secret,
        "env"
    );
    assert!(read(&path).contains("    secret: env\n"));

    for (ticks, expected) in [
        ("DEFAULT", Ok(None)),
        ("'7'", Ok(Some(7))),
        ("-2", Ok(Some(-2))),
        ("2.9", Ok(Some(2))),
    ] {
        std::fs::write(&path, format!("misc:\n  catch-up-ticks: {ticks}\n")).unwrap();
        assert_eq!(
            load_paper_global(dir.path(), None)
                .map(|p| p.catchup_ticks)
                .map_err(|_| ()),
            expected,
            "{ticks}"
        );
    }
    for bad in [
        "misc:\n  catch-up-ticks: often\n",
        "misc:\n  catch-up-ticks: '1.5'\n",
        "proxies:\n  velocity:\n    enabled: maybe\n",
    ] {
        std::fs::write(&path, bad).unwrap();
        assert!(
            matches!(
                load_paper_global(dir.path(), None),
                Err(ConfigError::Invalid { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn leaf_reads_any_case_and_survives_a_broken_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("leaf-global.yml");
    std::fs::write(
        &path,
        "misc:\n  including-5s-in-get-tps: FALSE\n  other: 1\n",
    )
    .unwrap();
    assert!(!load_leaf_global(&path).including_5s_in_get_tps);
    assert!(read(&path).ends_with("misc:\n  including-5s-in-get-tps: false\n  other: 1\nconfig-version: '3.0'\nasync: {}\nperformance: {}\nfixes: {}\ngameplay-mechanisms: {}\nnetwork: {}\n"));

    std::fs::write(&path, "misc: [\n").unwrap();
    assert!(load_leaf_global(&path).including_5s_in_get_tps);
    assert_eq!(read(&path), "misc: [\n");
}

/// `SpigotWorldConfig.initWorldGenSeeds` (derived from Spigot's/Paper's sources, Bukkit's
/// `MemorySection.getInt`/`getString` and Commons Lang's `isParsable`): a world's own number wins,
/// a non-number falls back to `world-settings.default`, then to the built-in default; the
/// mineshaft and stronghold seeds are numbers only when parsable.
#[test]
fn world_seeds_resolve_like_spigot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spigot.yml");
    std::fs::write(
        &path,
        "world-settings:\n  default:\n    seed-village: 7\n    seed-desert: oops\n    seed-mineshaft: '42'\n  minecraft:the_nether:\n    seed-village: 4294967297\n    seed-igloo: 2.9\n    seed-desert: 5\n    seed-mineshaft: default\n    seed-stronghold: -9000000000\n    seed-buriedtreasure: x\n  minecraft:the_end:\n    seed-stronghold: 1.5\n",
    )
    .unwrap();
    let spigot = load_spigot(&path).unwrap();
    let overworld = spigot.world_seeds("minecraft:overworld").unwrap();
    assert_eq!(overworld.salt("villages"), Some(7));
    assert_eq!(overworld.salt("desert_pyramids"), Some(14357617));
    assert_eq!(overworld.salt("mineshafts"), None);
    assert_eq!(overworld.mineshaft, Some(42));
    assert_eq!(overworld.stronghold, None);
    let nether = spigot.world_seeds("minecraft:the_nether").unwrap();
    assert_eq!(nether.salt("villages"), Some(1)); // intValue of a long
    assert_eq!(nether.salt("igloos"), Some(2));
    assert_eq!(nether.salt("desert_pyramids"), Some(5));
    assert_eq!(nether.mineshaft, None);
    assert_eq!(nether.stronghold, Some(-9_000_000_000));
    assert_eq!(nether.buried_treasure, 10387320);
    let end = spigot.world_seeds("minecraft:the_end").unwrap_err();
    assert!(
        end.to_string().contains("For input string: \"1.5\""),
        "{end}"
    );
    // The defaults Spigot adds are written; values already there stay.
    let text = read(&path);
    assert!(
        text.contains("    seed-village: 7\n    seed-desert: oops\n    seed-mineshaft: '42'\n    seed-igloo: 14357618\n"),
        "{text}"
    );
}

#[test]
fn paper_world_defaults_chunk_settings() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    assert_eq!(
        load_paper_world_defaults(d).unwrap(),
        ChunkSettings::default()
    );
    let text = read(&d.join("paper-world-defaults.yml"));
    assert!(
        text.starts_with("# This is the world defaults configuration file for Paper.\n"),
        "{text}"
    );
    assert!(text.ends_with("_version: 31\nchunks:\n  auto-save-interval: default\n  delay-chunk-unloads-by: 10s\n  max-auto-save-chunks-per-tick: 24\n  min-chunk-unload-count: 50\n  min-chunk-unload-fraction: 0.05\n"), "{text}");

    // Numbers as text, a negative interval (the default), Paper's duration units; other keys kept.
    std::fs::write(
        d.join("paper-world-defaults.yml"),
        "chunks:\n  auto-save-interval: '-5'\n  max-auto-save-chunks-per-tick: '8'\n  \
         min-chunk-unload-fraction: 1\n  delay-chunk-unloads-by: 2 m\nspawn:\n  keep: 1\n",
    )
    .unwrap();
    let settings = load_paper_world_defaults(d).unwrap();
    assert_eq!(
        settings,
        ChunkSettings {
            auto_save_interval: 6000,
            max_auto_save_chunks_per_tick: 8,
            min_chunk_unload_fraction: 1.0,
            delay_chunk_unloads_by: 2 * 60 * 20,
            ..ChunkSettings::default()
        }
    );
    std::fs::write(
        d.join("paper-world-defaults.yml"),
        "chunks:\n  auto-save-interval: 300\n  delay-chunk-unloads-by: 0s\n",
    )
    .unwrap();
    let settings = load_paper_world_defaults(d).unwrap();
    assert_eq!(
        (settings.auto_save_interval, settings.delay_chunk_unloads_by),
        (300, 0)
    );
    std::fs::write(
        d.join("paper-world-defaults.yml"),
        "chunks:\n  auto-save-interval: often\n",
    )
    .unwrap();
    assert!(load_paper_world_defaults(d).is_err());
}

/// Bukkit splits `world-settings.<world>.<key>` on `.`: a dotted world name nests, and falls back
/// to `world-settings.default` for what its section lacks. A top-level key holding the dots does
/// not match.
#[test]
fn dotted_world_names_nest_like_bukkit_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spigot.yml");
    std::fs::write(
        &path,
        "world-settings:\n  default:\n    seed-village: 7\n    seed-desert: 8\n  my:\n    world:\n      seed-village: 3\n  other.world:\n    seed-village: 99\n",
    )
    .unwrap();
    let spigot = load_spigot(&path).unwrap();
    let nested = spigot.world_seeds("my.world").unwrap();
    assert_eq!(nested.salt("villages"), Some(3));
    assert_eq!(nested.salt("desert_pyramids"), Some(8));
    let literal = spigot.world_seeds("other.world").unwrap();
    assert_eq!(literal.salt("villages"), Some(7));
}
