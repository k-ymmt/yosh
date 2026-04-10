use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut path = std::env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("kish-test-{}-{}", id, seq));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write_file(&self, name: &str, content: &str) -> PathBuf {
        let file_path = self.path.join(name);
        std::fs::write(&file_path, content).unwrap();
        file_path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
