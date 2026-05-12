//! Drive `yosh:plugin/*` exports against a `TestCtx`. Used by both
//! `yosh plugin run` (single invocation) and `yosh plugin test`
//! (scenario stepping).

use std::path::Path;
use std::time::Duration;

use wasmtime::Store;
use wasmtime::component::Component;

use crate::generated::{PluginWorld, PluginWorldPre};
use crate::precompile::make_engine;
use crate::test_host::{TestCtx, TestState, build_linker, register_imports};

pub struct LoadedPlugin {
    pub world: PluginWorld,
    pub store: Store<TestCtx>,
    pub engine: wasmtime::Engine,
}

#[derive(Debug)]
pub enum RunnerError {
    Load(String),
    Trap(String),
    Timeout(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Load(s) => write!(f, "load: {}", s),
            RunnerError::Trap(s) => write!(f, "trap: {}", s),
            RunnerError::Timeout(s) => write!(f, "timeout: {}", s),
        }
    }
}

pub fn load_plugin(
    wasm_path: &Path,
    state: TestState,
    timeout: Duration,
) -> Result<LoadedPlugin, RunnerError> {
    let engine = make_engine().map_err(|e| RunnerError::Load(e.to_string()))?;
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| RunnerError::Load(format!("read {}: {}", wasm_path.display(), e)))?;
    let component = Component::new(&engine, &wasm_bytes)
        .map_err(|e| RunnerError::Load(format!("compile: {}", e)))?;

    let mut linker = build_linker(&engine).map_err(|e| RunnerError::Load(e.to_string()))?;
    register_imports(&mut linker).map_err(|e| RunnerError::Load(e.to_string()))?;

    let pre = PluginWorldPre::new(
        linker
            .instantiate_pre(&component)
            .map_err(|e| RunnerError::Load(format!("instantiate_pre: {}", e)))?,
    )
    .map_err(|e| RunnerError::Load(format!("bindings: {}", e)))?;

    let mut store = Store::new(&engine, TestCtx::new(state));
    store.set_epoch_deadline(1);

    let watchdog_engine = engine.clone();
    let _watchdog = std::thread::Builder::new()
        .name("yosh-plugin-test-watchdog".into())
        .spawn(move || {
            std::thread::sleep(timeout);
            watchdog_engine.increment_epoch();
        });

    let world = pre
        .instantiate(&mut store)
        .map_err(|e| RunnerError::Load(format!("instantiate: {}", e)))?;
    Ok(LoadedPlugin {
        world,
        store,
        engine,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_wasm_returns_load_error() {
        let result = load_plugin(
            Path::new("/no/such/file.wasm"),
            TestState::default(),
            Duration::from_secs(1),
        );
        match result {
            Err(RunnerError::Load(_)) => {}
            Err(other) => panic!("expected Load error, got {:?}", other),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }

    #[test]
    fn load_non_wasm_file_returns_load_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not wasm").unwrap();
        let result = load_plugin(tmp.path(), TestState::default(), Duration::from_secs(1));
        match result {
            Err(RunnerError::Load(_)) => {}
            Err(other) => panic!("expected Load error, got {:?}", other),
            Ok(_) => panic!("expected Load error, got Ok"),
        }
    }
}
