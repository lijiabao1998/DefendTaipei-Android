//! Compatibility layer over [`crate::map`]: the old placeholder grid is gone; the shared RNG
//! and the collision world are re-exported from here for the gameplay modules.

pub use crate::map::Obstacles;

/// Small deterministic RNG so the layout is the same on every device.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}
