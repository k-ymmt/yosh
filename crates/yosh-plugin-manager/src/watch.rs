//! mtime-polling change detection for `yosh plugin run --watch`.
//! Dependency-free by design (spec §3.6): 300 ms polling is plenty for
//! a rebuild-and-rerun dev loop and avoids platform FS-event backends.

use std::path::Path;
use std::time::SystemTime;

pub(crate) const WATCH_POLL_MS: u64 = 300;

/// Block until `path`'s mtime differs from `last`, then return the new
/// mtime. Polls every `WATCH_POLL_MS`; a vanished file (editors and
/// cargo unlink briefly during rebuild) just keeps polling. After the
/// first observed change, waits one extra poll interval so a compiler
/// mid-write doesn't hand us a torn wasm.
pub(crate) fn wait_for_change(path: &Path, last: Option<SystemTime>) -> SystemTime {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(WATCH_POLL_MS));
        let Ok(md) = std::fs::metadata(path) else {
            continue;
        };
        let Ok(mtime) = md.modified() else { continue };
        if last != Some(mtime) {
            std::thread::sleep(std::time::Duration::from_millis(WATCH_POLL_MS));
            return mtime;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_change_sees_mtime_bump() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"v1").unwrap();
        let orig = std::fs::metadata(tmp.path()).unwrap().modified().unwrap();
        let path = tmp.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            // Set an explicit future mtime so the test doesn't depend
            // on filesystem timestamp granularity.
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10))
                .unwrap();
        });
        let start = std::time::Instant::now();
        let new = wait_for_change(tmp.path(), Some(orig));
        writer.join().unwrap();
        assert_ne!(new, orig);
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }
}
