//! POSIX XCU §2.5.3 `PWD` initialization at shell startup.
//!
//! The invoking process may have `chdir()`ed without refreshing the
//! `PWD` it hands down (an editor's `:terminal` after `:cd`, `env -C`,
//! a terminal multiplexer whose server started elsewhere, ...). The
//! `cd` builtin resolves relative operands against `$PWD` (POSIX cd
//! step 7, logical mode), so a stale inherited value makes `cd sub`
//! fail with "No such file or directory" and `cd .` / `cd ..` silently
//! move the shell to the stale directory's neighbourhood.
//!
//! POSIX therefore says the shell adopts the inherited `PWD` only when
//! it is an absolute pathname of the current working directory with no
//! dot or dot-dot components, and otherwise sets `PWD` to what `pwd -P`
//! would print. Called once per shell invocation (not for forked
//! subshells, which inherit the parent's already-correct value).

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

use super::ShellEnv;

/// Validate the inherited `PWD` against the real working directory and
/// replace it with the physical path when it does not match. The
/// result is exported so children see the corrected value.
pub fn ensure_pwd(env: &mut ShellEnv) {
    let inherited = env.vars.get("PWD").map(|s| s.to_string());
    let Some(pwd) = resolve_startup_pwd(inherited.as_deref(), inherited_names_cwd, physical_cwd)
    else {
        // getcwd failed (deleted cwd) and the inherited value is
        // unusable: leave things as they are rather than inventing one.
        return;
    };
    if inherited.as_deref() == Some(pwd.as_str()) {
        return;
    }
    // set() never fails here: PWD cannot be readonly in a fresh env.
    let _ = env.vars.set("PWD", pwd);
    env.vars.export("PWD");
}

/// Pure decision core, parameterized so unit tests need not chdir.
///
/// `names_cwd(p)` reports whether path `p` refers to the current
/// working directory; `physical()` yields the `pwd -P` value.
/// `Some(v)` is the value `PWD` should hold; `None` means no usable
/// value could be determined.
pub(crate) fn resolve_startup_pwd(
    inherited: Option<&str>,
    names_cwd: impl Fn(&str) -> bool,
    physical: impl Fn() -> Option<String>,
) -> Option<String> {
    if let Some(p) = inherited
        && is_acceptable_form(p)
        && names_cwd(p)
    {
        return Some(p.to_string());
    }
    physical()
}

/// The `pwd -L` value (POSIX XCU `pwd`): `pwd_var` when it is an
/// absolute pathname of the current directory without dot or dot-dot
/// components, otherwise the `pwd -P` value. `None` when the physical
/// path cannot be determined either (a deleted working directory).
pub(crate) fn logical_cwd(pwd_var: Option<&str>) -> Option<String> {
    resolve_startup_pwd(pwd_var, inherited_names_cwd, physical_cwd)
}

/// POSIX: absolute, and no component that is dot or dot-dot.
fn is_acceptable_form(p: &str) -> bool {
    p.starts_with('/') && !p.split('/').any(|c| c == "." || c == "..")
}

/// Same `st_dev`/`st_ino` as `.` — the check every other POSIX shell
/// performs at startup.
fn inherited_names_cwd(p: &str) -> bool {
    let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
        &crate::byteenc::decode_bytes(p),
    ));
    match (std::fs::metadata(&path), std::fs::metadata(".")) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

pub(crate) fn physical_cwd() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|p| crate::byteenc::encode_bytes(p.as_os_str().as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phys() -> Option<String> {
        Some("/real/cwd".to_string())
    }

    #[test]
    fn inherited_kept_when_it_names_cwd() {
        let got = resolve_startup_pwd(Some("/via/symlink"), |p| p == "/via/symlink", phys);
        assert_eq!(got.as_deref(), Some("/via/symlink"));
    }

    #[test]
    fn inherited_replaced_when_it_names_another_dir() {
        let got = resolve_startup_pwd(Some("/Users/me"), |_| false, phys);
        assert_eq!(got.as_deref(), Some("/real/cwd"));
    }

    #[test]
    fn inherited_rejected_when_relative() {
        // Even if the kernel check would pass, a relative value is not
        // an acceptable PWD.
        let got = resolve_startup_pwd(Some("sub/dir"), |_| true, phys);
        assert_eq!(got.as_deref(), Some("/real/cwd"));
    }

    #[test]
    fn inherited_rejected_with_dot_components() {
        for bad in ["/a/./b", "/a/../a", "/.", "/a/..", "/./a"] {
            let got = resolve_startup_pwd(Some(bad), |_| true, phys);
            assert_eq!(got.as_deref(), Some("/real/cwd"), "{bad}");
        }
    }

    #[test]
    fn dotfile_components_are_not_dot_components() {
        let got = resolve_startup_pwd(Some("/a/.config/..hidden"), |_| true, phys);
        assert_eq!(got.as_deref(), Some("/a/.config/..hidden"));
    }

    #[test]
    fn unset_inherited_uses_physical() {
        let got = resolve_startup_pwd(None, |_| true, phys);
        assert_eq!(got.as_deref(), Some("/real/cwd"));
    }

    #[test]
    fn unusable_inherited_and_no_physical_yields_none() {
        let got = resolve_startup_pwd(Some("/stale"), |_| false, || None);
        assert_eq!(got, None);
    }

    #[test]
    fn acceptable_form_rules() {
        assert!(is_acceptable_form("/"));
        assert!(is_acceptable_form("/a/b/"));
        assert!(!is_acceptable_form(""));
        assert!(!is_acceptable_form("a"));
        assert!(!is_acceptable_form("/a/./b"));
    }

    #[test]
    fn ensure_pwd_replaces_stale_value_and_exports() {
        let _guard = crate::test_sync::lock_cwd();
        let mut env = ShellEnv::new("yosh", vec![]);
        let real = physical_cwd().expect("cwd readable");
        // "/" is never the cargo test cwd.
        assert_ne!(real, "/");
        let _ = env.vars.set("PWD", "/");
        ensure_pwd(&mut env);
        assert_eq!(env.vars.get("PWD"), Some(real.as_str()));
        assert!(env.vars.get_var("PWD").unwrap().exported);
    }

    #[test]
    fn ensure_pwd_sets_value_when_unset() {
        let _guard = crate::test_sync::lock_cwd();
        let mut env = ShellEnv::new("yosh", vec![]);
        let _ = env.vars.unset("PWD");
        ensure_pwd(&mut env);
        let real = physical_cwd().unwrap();
        assert_eq!(env.vars.get("PWD"), Some(real.as_str()));
        assert!(env.vars.get_var("PWD").unwrap().exported);
    }

    #[test]
    fn ensure_pwd_keeps_matching_logical_value() {
        let _guard = crate::test_sync::lock_cwd();
        let mut env = ShellEnv::new("yosh", vec![]);
        // A logical spelling that names the cwd: trailing slash is an
        // acceptable form and stats to the same inode.
        let logical = format!("{}/", physical_cwd().unwrap());
        let _ = env.vars.set("PWD", logical.clone());
        ensure_pwd(&mut env);
        assert_eq!(env.vars.get("PWD"), Some(logical.as_str()));
    }
}
