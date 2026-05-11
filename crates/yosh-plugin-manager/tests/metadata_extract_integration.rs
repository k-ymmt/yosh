//! Integration tests for `metadata_extract::extract` against real
//! cargo-component-built wasm plugins.
//!
//! Regression coverage for #3: real-world Rust plugins built via
//! `cargo component build --target wasm32-wasip2` import the full
//! WASI Preview 2 surface transitively (wasi:io/*, wasi:cli/*) and
//! every `yosh:plugin/*` interface declared in `plugin-world`. The
//! metadata sandbox must satisfy those imports — with empty WasiCtx
//! and Err(Denied) stubs — or `instantiate_pre` fails and the plugin
//! is silently dropped from `plugins.lock`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static TEST_PLUGIN_WASM: OnceLock<PathBuf> = OnceLock::new();

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR for this crate is `<workspace>/crates/yosh-plugin-manager`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root from manager crate manifest dir")
        .to_path_buf()
}

fn ensure_test_plugin_built() -> PathBuf {
    TEST_PLUGIN_WASM
        .get_or_init(|| {
            let status = Command::new("cargo")
                .args([
                    "component",
                    "build",
                    "-p",
                    "test_plugin",
                    "--target",
                    "wasm32-wasip2",
                    "--release",
                ])
                .current_dir(workspace_root())
                .status()
                .expect("cargo component build failed (is cargo-component installed?)");
            assert!(status.success(), "test_plugin build failed");
            workspace_root().join("target/wasm32-wasip2/release/test_plugin.wasm")
        })
        .clone()
}

/// Real cargo-component-built plugins import yosh:plugin/files,
/// yosh:plugin/commands, and the full wasi:io/* + wasi:cli/* surface
/// transitively. Before #3's fix, `extract` failed at
/// `instantiate_pre` with "component imports instance
/// `yosh:plugin/files@0.2.1`, but a matching implementation was not
/// found in the linker". This regression test asserts the metadata
/// sandbox satisfies every import the SDK pulls in.
#[test]
fn extract_succeeds_for_real_cargo_component_plugin() {
    let wasm_path = ensure_test_plugin_built();
    let wasm_bytes = std::fs::read(&wasm_path).expect("read test_plugin wasm");
    let engine = yosh_plugin_manager::precompile::make_engine().expect("make engine");

    let md = yosh_plugin_manager::metadata_extract::extract(&engine, &wasm_bytes)
        .expect("metadata extract should succeed for a real cargo-component plugin");

    assert_eq!(md.name, "test_plugin");
    // test_plugin advertises files-read/write and commands-exec capabilities
    // among others. The presence of these in the extracted metadata proves
    // (a) the plugin instantiated under the deny-stub linker, and (b) the
    // metadata() call succeeded end-to-end.
    assert!(
        md.required_capabilities.iter().any(|c| c == "files:read"),
        "expected files:read in capabilities, got {:?}",
        md.required_capabilities
    );
    assert!(
        md.required_capabilities.iter().any(|c| c == "commands:exec"),
        "expected commands:exec in capabilities, got {:?}",
        md.required_capabilities
    );
}
