use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yosh-completions"))
}

#[test]
fn help_prints_usage_and_exits_zero() {
    for arg in [None, Some("-h"), Some("--help")] {
        let mut cmd = bin();
        if let Some(a) = arg {
            cmd.arg(a);
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "args {arg:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "args {arg:?}");
        assert!(stdout.contains("list"), "args {arg:?}");
        assert!(stdout.contains("export"), "args {arg:?}");
    }
}

#[test]
fn list_prints_embedded_spec_names() {
    let output = bin().arg("list").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = stdout.lines().collect();
    assert!(names.contains(&"git"), "list output: {stdout}");
    assert!(names.contains(&"cd"), "list output: {stdout}");
}

#[test]
fn export_writes_spec_and_refuses_overwrite_without_force() {
    let home = tempfile::TempDir::new().unwrap();
    let spec_path = home.path().join(".config/yosh/completions/git.toml");

    let output = bin().env("HOME", home.path()).args(["export", "git"]).output().unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(spec_path.is_file());
    let text = std::fs::read_to_string(&spec_path).unwrap();
    assert!(text.contains("[[subcommands]]"), "exported file should be the git spec");

    // Second export without --force must fail and leave the file alone.
    std::fs::write(&spec_path, "# user edit\n").unwrap();
    let output = bin().env("HOME", home.path()).args(["export", "git"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"), "stderr: {stderr}");
    assert_eq!(std::fs::read_to_string(&spec_path).unwrap(), "# user edit\n");

    // --force overwrites.
    let output = bin()
        .env("HOME", home.path())
        .args(["export", "--force", "git"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(std::fs::read_to_string(&spec_path).unwrap().contains("[[subcommands]]"));
}

#[test]
fn export_unknown_name_exits_one() {
    let home = tempfile::TempDir::new().unwrap();
    let output = bin()
        .env("HOME", home.path())
        .args(["export", "no-such-spec-xyz"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-spec-xyz"), "stderr: {stderr}");
}

#[test]
fn export_without_names_is_usage_error() {
    let output = bin().arg("export").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let output = bin().arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn export_unknown_flag_is_usage_error() {
    let home = tempfile::TempDir::new().unwrap();
    let output = bin()
        .env("HOME", home.path())
        .args(["export", "-f", "git"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown option"), "stderr: {stderr}");
}

#[test]
fn export_without_home_exits_one() {
    let output = bin()
        .env_remove("HOME")
        .args(["export", "git"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("HOME"), "stderr: {stderr}");
}
