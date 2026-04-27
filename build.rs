use std::process::Command;

fn main() {
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
