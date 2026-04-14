use std::process::Command;

fn kish_plugin_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kish-plugin"))
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let output = kish_plugin_bin().arg("--help").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kish shell plugins"), "should contain description");
    assert!(stdout.contains("sync"), "should list sync command");
    assert!(stdout.contains("update"), "should list update command");
    assert!(stdout.contains("list"), "should list list command");
    assert!(stdout.contains("verify"), "should list verify command");
}

#[test]
fn short_help_flag_works() {
    let output = kish_plugin_bin().arg("-h").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kish shell plugins"), "should contain description");
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let output = kish_plugin_bin().arg("--version").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "should contain package version");
    assert!(stdout.contains('('), "should contain build info in parens");
}

#[test]
fn subcommand_help_works() {
    for subcmd in &["sync", "update", "list", "verify"] {
        let output = kish_plugin_bin()
            .args([subcmd, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{} --help should exit 0", subcmd);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "{} --help should contain Usage", subcmd);
    }
}

#[test]
fn no_args_shows_help_and_exits_error() {
    let output = kish_plugin_bin().output().unwrap();
    // clap exits with code 2 when no subcommand given
    assert!(!output.status.success(), "no args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:") || stderr.contains("kish-plugin"),
        "should show usage hint on stderr"
    );
}

#[test]
fn unknown_command_exits_error() {
    let output = kish_plugin_bin().arg("bogus").output().unwrap();
    assert!(!output.status.success(), "unknown command should fail");
}
