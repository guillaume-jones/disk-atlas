//! Deterministic randomness: every generated shape is seeded from stable strings, so a survey always
//! draws the same map.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// The `FNV-1a` hash.
pub fn hash_str(s: &str) -> u64 {
    s.bytes().fold(FNV_OFFSET, |h, b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

/// `SplitMix64`: tiny, deterministic, good enough for procedural generation.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ GOLDEN_GAMMA)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN_GAMMA);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [0, n), or 0 when n is 0.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}
