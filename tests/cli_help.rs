use std::process::Command;

fn yosh_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yosh"))
}

#[test]
fn help_flag_prints_usage_and_exits_zero() {
    let output = yosh_bin().arg("--help").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("yosh - A POSIX-compliant shell"),
        "should contain description"
    );
    assert!(stdout.contains("Usage:"), "should contain Usage section");
    assert!(
        stdout.contains("Options:"),
        "should contain Options section"
    );
    assert!(stdout.contains("--help"), "should list --help option");
    assert!(stdout.contains("--version"), "should list --version option");
    assert!(stdout.contains("-c <command>"), "should list -c option");
    assert!(stdout.contains("plugin"), "should list plugin subcommand");
}

#[test]
fn short_help_flag_works() {
    let output = yosh_bin().arg("-h").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("yosh - A POSIX-compliant shell"),
        "should contain description"
    );
    assert!(stdout.contains("Usage:"), "should contain Usage section");
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let output = yosh_bin().arg("--version").output().unwrap();
    assert!(output.status.success(), "exit code should be 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("yosh "), "should start with 'yosh '");
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "should contain package version"
    );
    // Version format: yosh 0.1.0 (hash date)
    assert!(stdout.contains('('), "should contain build info in parens");
}

#[test]
fn help_output_goes_to_stdout() {
    let output = yosh_bin().arg("--help").output().unwrap();
    assert!(!output.stdout.is_empty(), "stdout should have content");
    assert!(output.stderr.is_empty(), "stderr should be empty");
}

#[test]
fn help_no_color_when_env_set() {
    let output = yosh_bin()
        .arg("--help")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // ANSI escape sequences start with \x1b[
    assert!(
        !stdout.contains('\x1b'),
        "should not contain ANSI escapes when NO_COLOR is set"
    );
}

#[test]
fn help_color_forced_with_clicolor_force() {
    let output = yosh_bin()
        .arg("--help")
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('\x1b'),
        "should contain ANSI escapes when CLICOLOR_FORCE=1"
    );
}

#[test]
fn help_clicolor_force_zero_does_not_force() {
    // CLICOLOR_FORCE=0 should not force colors; since test stdout is not a TTY, no color
    let output = yosh_bin()
        .arg("--help")
        .env("CLICOLOR_FORCE", "0")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\x1b'),
        "CLICOLOR_FORCE=0 should not force ANSI escapes"
    );
}
