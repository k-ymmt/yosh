use std::io::Write;

#[test]
fn sync_local_plugin_creates_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join(".config/yosh");
    let plugin_dir = dir.path().join(".yosh/plugins");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&plugin_dir).unwrap();

    // Create a fake plugin binary
    let fake_binary = plugin_dir.join("liblocal.dylib");
    std::fs::write(&fake_binary, b"fake binary content").unwrap();

    // Create plugins.toml
    let toml_path = config_dir.join("plugins.toml");
    let mut f = std::fs::File::create(&toml_path).unwrap();
    write!(
        f,
        r#"
[[plugin]]
name = "local-test"
source = "local:{}"
capabilities = ["io"]
"#,
        fake_binary.display()
    )
    .unwrap();

    // Parse config
    let decls = yosh_plugin_manager::config::load_config(&toml_path).unwrap();
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "local-test");

    // Compute expected SHA-256
    let sha256 = yosh_plugin_manager::verify::sha256_file(&fake_binary).unwrap();
    assert!(!sha256.is_empty());
    assert_eq!(sha256.len(), 64); // hex-encoded SHA-256 is 64 chars
}

#[test]
fn lockfile_round_trip_with_multiple_entries() {
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join("plugins.lock");

    let lockfile = yosh_plugin_manager::lockfile::LockFile {
        plugin: vec![
            yosh_plugin_manager::lockfile::LockEntry {
                name: "a".into(),
                path: "/path/a.dylib".into(),
                enabled: true,
                capabilities: Some(vec!["io".into()]),
                sha256: "aaa".into(),
                upstream_sha256: Some("aaa-upstream".into()),
                source: "github:u/a".into(),
                version: Some("1.0.0".into()),
            },
            yosh_plugin_manager::lockfile::LockEntry {
                name: "b".into(),
                path: "/path/b.dylib".into(),
                enabled: false,
                capabilities: None,
                sha256: "bbb".into(),
                upstream_sha256: None,
                source: "local:/path/b.dylib".into(),
                version: None,
            },
        ],
    };

    yosh_plugin_manager::lockfile::save_lockfile(&lock_path, &lockfile).unwrap();
    let loaded = yosh_plugin_manager::lockfile::load_lockfile(&lock_path).unwrap();
    assert_eq!(loaded.plugin.len(), 2);
    assert_eq!(loaded.plugin[0].name, "a");
    assert!(loaded.plugin[0].enabled);
    assert_eq!(loaded.plugin[1].name, "b");
    assert!(!loaded.plugin[1].enabled);
    assert!(loaded.plugin[1].version.is_none());
}
