use std::path::Path;
use std::process::Command;

fn main() {
    verify_bundled_wit(
        "crates/yosh-plugin-api/wit/yosh-plugin.wit",
        "wit/yosh-plugin.wit",
    );

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

    generate_embedded_completions();
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

/// Generate `$OUT_DIR/embedded_completions.rs`: a static array embedding
/// every `completions/*.toml` so specs work without any user setup.
/// `spec_completion.rs` pulls it in with `include!`.
fn generate_embedded_completions() {
    println!("cargo:rerun-if-changed=completions");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest_dir).join("completions");
    let mut entries: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("completions/ must exist")
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .map(|p| {
            let name = p
                .file_stem()
                .expect("spec file has a stem")
                .to_str()
                .expect("spec file name is UTF-8")
                .to_string();
            // Use relative path from manifest dir for portability
            let rel_path = format!("completions/{}", p.file_name().unwrap().to_str().unwrap());
            (name, rel_path)
        })
        .collect();
    entries.sort();

    let mut code = String::from(
        "/// Completion specs compiled in from `completions/*.toml`, sorted by name.\n\
         /// Used as the fallback layer when no user spec file exists.\n\
         pub static EMBEDDED_SPECS: &[(&str, &str)] = &[\n",
    );
    for (name, path) in &entries {
        // Use concat! with env!("CARGO_MANIFEST_DIR") to create portable absolute paths
        code.push_str(&format!(
            "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{path}\"))),\n"
        ));
    }
    code.push_str("];\n");

    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("embedded_completions.rs");
    std::fs::write(&out, code).expect("write embedded_completions.rs");
}
