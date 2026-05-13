use std::path::Path;

fn main() {
    verify_bundled_wit(
        "../yosh-plugin-api/wit/yosh-plugin.wit",
        "wit/yosh-plugin.wit",
    );
}

/// Verify the bundled WIT copy stays in sync with the canonical source
/// in `yosh-plugin-api`.
///
/// The bundled copy at `bundled` is what `cargo install yosh-plugin-sdk`
/// ships — the published SDK crate is extracted standalone, so a sibling
/// path like `../yosh-plugin-api/wit` cannot resolve. `wit_bindgen::generate!`
/// reads it via `path: "wit"`. The canonical source at `canonical` only
/// exists when building inside the workspace; when the crate is downloaded
/// standalone from crates.io, the path is absent and the check is skipped.
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
            "bundled WIT at crates/yosh-plugin-sdk/{} is out of sync with canonical {}.\n\
             Run: cp {} crates/yosh-plugin-sdk/{}",
            bundled, canonical, canonical, bundled
        );
    }
}
