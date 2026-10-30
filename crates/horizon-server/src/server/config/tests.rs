use super::*;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn missing_files_are_written_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();

    let spigot = load_spigot(&d.join("spigot.yml")).unwrap();
    assert_eq!(
        spigot,
        SpigotConfig {
            tab_complete: 0,
            send_namespaced: false
        }
    );
    let text = read(&d.join("spigot.yml"));
    assert!(
        text.starts_with("# This is the Spigot configuration file for Paper.\n"),
        "{text}"
    );
    assert!(text.contains("\n# with caution, and make sure you know what each option does before configuring.\n#\n# If you need help"));
    assert!(text.ends_with("# Website: https://papermc.io/\n\nconfig-version: 13\ncommands:\n  tab-complete: 0\n  send-namespaced: false\n"), "{text}");

    let purpur = load_purpur(&d.join("purpur.yml")).unwrap();
    assert_eq!(
        purpur,
        PurpurConfig {
            verbose: false,
            lagging_threshold: 19.0
        }
    );
    let text = read(&d.join("purpur.yml"));
    assert!(text.ends_with("# Docs: https://purpurmc.org/docs \n\nverbose: false\nconfig-version: 49\nsettings:\n  lagging-threshold: 19.0\n"), "{text}");

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
    assert_eq!(
        spigot,
        SpigotConfig {
            tab_complete: -1,
            send_namespaced: false
        }
    );
    assert_eq!(
        read(&path).split_once("\n\n").unwrap().1,
        "config-version: 13\nsettings:\n  bungeecord: true\ncommands:\n  tab-complete: -1\n  send-namespaced: 'true'\n"
    );

    let path = dir.path().join("purpur.yml");
    std::fs::write(
        &path,
        "verbose: on\nsettings:\n  lagging-threshold: 1_8.5\n",
    )
    .unwrap();
    assert_eq!(
        load_purpur(&path).unwrap(),
        PurpurConfig {
            verbose: true,
            lagging_threshold: 18.5
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
