//! Shared helpers for plugin benches (plugin_bench.rs, startup_bench.rs).
//!
//! Both benches need to stage a HOME directory whose
//! `.config/yosh/plugins.lock` lists 0/1/3 perf_plugin entries. Lifting the
//! staging here keeps each bench focused on its measurement.

#![allow(dead_code)] // imported by benches via `mod plugin_bench_helpers;`

use std::io::Write;
use std::path::{Path, PathBuf};

pub fn perf_plugin_wasm() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/perf_plugin.wasm");
    assert!(
        p.exists(),
        "perf_plugin.wasm not found at {}; build it first with \
         `cargo component build -p perf_plugin --target wasm32-wasip2 --release`",
        p.display()
    );
    p
}

/// Create a tempdir with `.config/yosh/plugins.lock` listing `count`
/// aliased perf_plugin entries. Caller sets `HOME` to `tempdir.path()`
/// before launching `yosh` / `yosh-dhat`.
pub fn stage_home_with_plugin(count: usize) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join(".config/yosh");
    std::fs::create_dir_all(&dir).expect("create cfg dir");
    let lock = dir.join("plugins.lock");
    let mut f = std::fs::File::create(&lock).expect("create lock");

    if count > 0 {
        let wasm = perf_plugin_wasm();
        for i in 0..count {
            writeln!(
                f,
                r#"
[[plugin]]
name = "perf_{i}"
path = "{wasm}"
enabled = true
capabilities = ["variables:read", "hooks:pre_prompt", "hooks:pre_exec", "hooks:post_exec"]
"#,
                i = i,
                wasm = wasm.display()
            )
            .expect("write entry");
        }
    }
    drop(f);
    tmp
}
