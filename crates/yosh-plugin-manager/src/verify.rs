use std::path::Path;

use sha2::{Sha256, Digest};

/// Compute the SHA-256 hex digest of a file.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let hash = Sha256::digest(&data);
    Ok(format!("{:x}", hash))
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
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn sha256_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let hash = sha256_file(f.path()).unwrap();
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn sha256_missing_file() {
        assert!(sha256_file(Path::new("/nonexistent/file")).is_err());
    }

    #[test]
    fn verify_checksum_match() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(verify_checksum(f.path(), "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9").unwrap());
    }

    #[test]
    fn verify_checksum_mismatch() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        assert!(!verify_checksum(f.path(), "0000000000000000000000000000000000000000000000000000000000000000").unwrap());
    }
}
