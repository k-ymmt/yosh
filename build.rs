use std::path::Path;
use std::process::Command;

fn main() {
    verify_bundled_wit("crates/yosh-plugin-api/wit/yosh-plugin.wit", "wit/yosh-plugin.wit");

    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let date = Command::new("git")
        .args(["log", "-1", "--format=%ci"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.get(..10).map(|d| d.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=YOSH_GIT_HASH={}", hash);
    println!("cargo:rustc-env=YOSH_BUILD_DATE={}", date);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    println!("cargo:rerun-if-changed=.git/packed-refs");

    // Pass cargo's TARGET (set by cargo during build) through to the binary
    // as a compile-time env var, so plugin cache code can reference the
    // target triple at runtime via env!(...) without needing a runtime probe.
    let triple = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=TARGET_TRIPLE_OR_RUST_BUILT_IN={}", triple);
}

/// Verify the bundled WIT copy stays in sync with the canonical source
/// in `yosh-plugin-api`.
///
/// The bundled copy at `bundled` is what `cargo install yosh` ships,
/// because the published yosh crate has no access to sibling crate
/// directories; `bindgen!` reads it via `path: "wit"`. The canonical
/// source at `canonical` only exists when building inside the workspace
/// — when this crate is downloaded standalone from crates.io, the path
/// is absent and the check is skipped.
fn verify_bundled_wit(canonical: &str, bundled: &str) {
    println!("cargo:rerun-if-changed={}", bundled);
    let canonical_path = Path::new(canonical);
    if !canonical_path.exists() {
        return;
    }
    println!("cargo:rerun-if-changed={}", canonical);
    let canonical_bytes = std::fs::read(canonical_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", canonical, e));
    let bundled_bytes = std::fs::read(bundled)
        .unwrap_or_else(|e| panic!("failed to read bundled {}: {}", bundled, e));
    if canonical_bytes != bundled_bytes {
        panic!(
            "bundled WIT at {} is out of sync with canonical {}.\n\
             Run: cp {} {}",
            bundled, canonical, canonical, bundled
        );
    }
}
