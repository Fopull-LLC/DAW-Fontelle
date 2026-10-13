//! The noise a diffusion or flow model starts from, made here from the
//! render's seed so that the same phrase and seed sing the same take.

/// Standard normal numbers from a seed (xoshiro256** and Box–Muller).
pub struct Gaussian {
    state: [u64; 4],
    spare: Option<f32>,
}

impl Gaussian {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = sm;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        Self {
            state: [next(), next(), next(), next()],
            spare: None,
        }
    }

    fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in (0, 1].
    fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) + 1) as f64 / (1u64 << 53) as f64
    }

    pub fn next_f32(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let r = (-2.0 * self.uniform().ln()).sqrt();
        let theta = std::f64::consts::TAU * self.uniform();
        self.spare = Some((r * theta.sin()) as f32);
        (r * theta.cos()) as f32
    }

    pub fn fill(&mut self, out: &mut [f32]) {
        for v in out {
            *v = self.next_f32();
        }
    }
}
