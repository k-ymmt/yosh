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

    dead_strip_unused_dylibs();

    generate_embedded_completions();
}

/// On Apple targets, drop load commands for dylibs the binary never
/// references (`ld -dead_strip_dylibs`).
///
/// Dependencies declare frameworks unconditionally via `#[link]`:
/// CoreFoundation (iana-time-zone <- cap-time-ext <- wasmtime-wasi) and
/// Security + CoreFoundation (rustls-platform-verifier <- ureq <-
/// yosh-plugin-manager). In the `yosh` binary none of their code is
/// reachable (only `yosh-plugin` downloads anything, and wasmtime-wasi
/// never asks for a timezone) — yet dyld would load and initialize both
/// frameworks (and their dependency closure) on every launch, which
/// measured ~0.8 ms of a ~3.7 ms `yosh -c :`. Binaries that do use a
/// framework (`yosh-plugin`) keep it: only unreferenced dylibs go.
///
/// ld decides which dylibs are referenced *before* `-dead_strip`, so
/// this only takes effect when the unreachable code is already gone
/// from the objects it links: the release profile's LTO does that
/// (debug builds keep both frameworks; harmless).
fn dead_strip_unused_dylibs() {
    if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple") {
        println!("cargo:rustc-link-arg-bins=-Wl,-dead_strip_dylibs");
    }
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
