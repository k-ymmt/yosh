//! A small, fast, non-cryptographic hasher (the FxHash algorithm used by
//! rustc) for the shell's hot name-keyed tables. The keys are variable
//! and function names chosen by the script author, so HashDoS
//! resistance — the reason std's default SipHash is slow — buys nothing
//! here, while every variable lookup pays for the hash.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            self.add(u64::from_le_bytes(bytes[..8].try_into().unwrap()));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            self.add(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64);
            bytes = &bytes[4..];
        }
        for &b in bytes {
            self.add(b as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// `HashMap` keyed with `FxHasher`.
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_round_trips_and_distinguishes_similar_keys() {
        let mut m: FxHashMap<String, usize> = FxHashMap::default();
        for i in 0..2000 {
            m.insert(format!("var_{i}"), i);
        }
        for i in 0..2000 {
            assert_eq!(m.get(format!("var_{i}").as_str()), Some(&i));
        }
        assert_eq!(m.get("var_"), None);
        assert_eq!(m.get("var_2000"), None);
    }
}
