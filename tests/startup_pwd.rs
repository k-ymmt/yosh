//! Startup `PWD` initialization (POSIX XCU §2.5.3).
//!
//! Regression for: "while in another directory, `pwd`/the prompt
//! occasionally shows $HOME and relative `cd` fails". A launcher that
//! `chdir()`s but passes its own stale `PWD` (editor `:terminal`,
//! `env -C`, ...) used to leave yosh resolving relative `cd` operands
//! against the wrong directory.

use std::path::Path;
use std::process::Command;

fn run(cwd: &Path, pwd_env: Option<&str>, script: &str) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yosh"));
    cmd.current_dir(cwd).arg("-c").arg(script);
    match pwd_env {
        Some(v) => {
            cmd.env("PWD", v);
        }
        None => {
            cmd.env_remove("PWD");
        }
    }
    let out = cmd.output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

/// A tempdir with a `sub` child; returns (tempdir guard, physical path).
fn dir_with_sub() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("sub")).unwrap();
    // macOS: $TMPDIR lives under /var -> /private/var; use the physical
    // form so `pwd -P` style comparisons are exact.
    let phys = std::fs::canonicalize(tmp.path()).unwrap();
    (tmp, phys)
}

#[test]
fn stale_inherited_pwd_is_replaced_by_physical_cwd() {
    let (_tmp, phys) = dir_with_sub();
    let (out, err, code) = run(&phys, Some("/"), r#"echo "$PWD""#);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, phys.to_string_lossy());
}

#[test]
fn relative_cd_works_despite_stale_inherited_pwd() {
    let (_tmp, phys) = dir_with_sub();
    let (out, err, code) = run(&phys, Some("/"), r#"cd sub && echo "$PWD" && /bin/pwd -P"#);
    assert_eq!(code, 0, "cd sub must succeed; stderr: {err}");
    let expect = phys.join("sub").to_string_lossy().into_owned();
    assert_eq!(out, format!("{expect}\n{expect}"));
}

#[test]
fn cd_dot_stays_put_despite_stale_inherited_pwd() {
    // Before the fix `cd .` resolved to the stale value and moved the
    // shell there.
    let (_tmp, phys) = dir_with_sub();
    let (out, err, code) = run(&phys, Some("/"), r#"cd . && /bin/pwd -P"#);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, phys.to_string_lossy());
}

#[test]
fn unset_pwd_is_initialized_and_exported() {
    let (_tmp, phys) = dir_with_sub();
    let (out, err, code) = run(&phys, None, r#"echo "$PWD"; /usr/bin/env | grep '^PWD='"#);
    assert_eq!(code, 0, "stderr: {err}");
    let p = phys.to_string_lossy();
    assert_eq!(out, format!("{p}\nPWD={p}"));
}

#[test]
fn matching_logical_pwd_is_preserved() {
    // A symlinked spelling of the cwd is a valid logical PWD and must
    // be kept verbatim (not rewritten to the physical path).
    let (tmp, phys) = dir_with_sub();
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&phys, &link).unwrap();
    let logical = link.to_string_lossy().into_owned();
    let (out, err, code) = run(
        &phys,
        Some(&logical),
        r#"echo "$PWD"; cd sub && echo "$PWD""#,
    );
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, format!("{logical}\n{logical}/sub"));
}

#[test]
fn inherited_pwd_with_dot_components_is_rejected() {
    // Names the cwd, but POSIX forbids adopting a value with `.`/`..`.
    let (_tmp, phys) = dir_with_sub();
    let dotted = format!("{}/sub/..", phys.to_string_lossy());
    let (out, err, code) = run(&phys, Some(&dotted), r#"echo "$PWD""#);
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out, phys.to_string_lossy());
}
