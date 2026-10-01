//! A fast, non-cryptographic hasher for interned keys and small integers.
//!
//! The Fx hash from rustc: rotate, xor, multiply. Keys here are short strings
//! and ids, where SipHash's DoS resistance buys nothing.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Clone, Copy, Default)]
pub struct FxHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.mix(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let mut tail = [0u8; 8];
        let rest = chunks.remainder();
        tail[..rest.len()].copy_from_slice(rest);
        self.mix(u64::from_le_bytes(tail) ^ rest.len() as u64);
    }

    fn write_u8(&mut self, n: u8) {
        self.mix(n as u64);
    }

    fn write_u32(&mut self, n: u32) {
        self.mix(n as u64);
    }

    fn write_u64(&mut self, n: u64) {
        self.mix(n);
    }

    fn write_usize(&mut self, n: usize) {
        self.mix(n as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub type Map<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type Set<K> = HashSet<K, BuildHasherDefault<FxHasher>>;
