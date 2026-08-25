use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Compute the SHA-256 hex digest of a file, streaming in fixed-size
/// chunks so large plugin binaries are never held in memory whole.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let err = |e: std::io::Error| format!("{}: {}", path.display(), e);
    let file = std::fs::File::open(path).map_err(err)?;
    sha256_reader(file).map_err(err)
}

/// Streaming SHA-256 over any reader. Retries `Interrupted` reads, like
/// `read_to_end` does, so a stray EINTR does not fail verification.
fn sha256_reader<R: Read>(mut reader: R) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Check if a file's SHA-256 matches the expected hex digest.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<bool, String> {
    let actual = sha256_file(path)?;
    Ok(actual == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn sha256_known_content() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        let hash = sha256_file(f.path()).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let hash = sha256_file(f.path()).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_multi_chunk_matches_one_shot() {
        // Larger than the 64 KiB streaming buffer, and not a multiple of
        // it, so the loop takes several full reads plus a short tail.
        // Varied bytes (period 251, coprime with the buffer size) so a
        // duplicated, dropped, or reordered chunk changes the digest.
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&data).unwrap();
        let hash = sha256_file(f.path()).unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(&data)));
    }

    #[test]
    fn sha256_reader_retries_interrupted() {
        struct Interrupting {
            data: &'static [u8],
            interrupts_left: usize,
        }
        impl Read for Interrupting {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.interrupts_left > 0 {
                    self.interrupts_left -= 1;
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                let n = self.data.len().min(buf.len()).min(4);
                buf[..n].copy_from_slice(&self.data[..n]);
                self.data = &self.data[n..];
                Ok(n)
            }
        }
        let hash = sha256_reader(Interrupting {
            data: b"hello world",
            interrupts_left: 3,
        })
        .unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_missing_file() {
        assert!(sha256_file(Path::new("/nonexistent/file")).is_err());
    }

    #[test]
    fn verify_checksum_match() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(
            verify_checksum(
                f.path(),
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
            )
            .unwrap()
        );
    }

    #[test]
    fn verify_checksum_mismatch() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(
            !verify_checksum(
                f.path(),
                "0000000000000000000000000000000000000000000000000000000000000000"
            )
            .unwrap()
        );
    }
}
