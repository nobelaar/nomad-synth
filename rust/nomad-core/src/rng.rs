//! Bit-exact port of `numpy.random.default_rng(seed)` for non-negative integer
//! seeds: `SeedSequence(seed)` -> `generate_state(4, uint64)` -> PCG64 (XSL-RR
//! 128/64) -> `Generator.random()` and `Generator.standard_normal()` (256-layer
//! ziggurat with numpy's own tables, see `ziggurat.rs`).

use crate::ziggurat::{FI_BITS, KI, WI_BITS};

const ZIGGURAT_NOR_R: f64 = 3.654_152_885_361_009;
const ZIGGURAT_NOR_INV_R: f64 = 0.273_661_237_329_758_3;

const INIT_A: u32 = 0x43b0_d7e5;
const MULT_A: u32 = 0x931e_8875;
const INIT_B: u32 = 0x8b51_f9dd;
const MULT_B: u32 = 0x58f3_8ded;
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const XSHIFT: u32 = 16;
const POOL: usize = 4;
const PCG_MULT: u128 = 0x2360_ED05_1FC6_5DA4_4385_DF64_9FCC_F645;

fn hashmix(value: u32, hash_const: &mut u32) -> u32 {
    let mut v = value ^ *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    v = v.wrapping_mul(*hash_const);
    v ^ (v >> XSHIFT)
}

fn mix(x: u32, y: u32) -> u32 {
    let r = MIX_MULT_L
        .wrapping_mul(x)
        .wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    r ^ (r >> XSHIFT)
}

/// `SeedSequence(seed).pool`: the seed as little-endian u32 words, mixed.
pub fn seed_sequence_pool(seed: u64) -> [u32; POOL] {
    let entropy: &[u32] = &[seed as u32, (seed >> 32) as u32];
    // numpy drops high zero words, but keeps a single 0 for seed 0.
    let entropy = if seed >> 32 == 0 {
        &entropy[..1]
    } else {
        entropy
    };
    let mut hc = INIT_A;
    let mut mixer = [0u32; POOL];
    for (i, m) in mixer.iter_mut().enumerate() {
        *m = hashmix(entropy.get(i).copied().unwrap_or(0), &mut hc);
    }
    for src in 0..POOL {
        for dst in 0..POOL {
            if src != dst {
                let h = hashmix(mixer[src], &mut hc);
                mixer[dst] = mix(mixer[dst], h);
            }
        }
    }
    // The entropy never has more words than the pool, so there is no extra mixing.
    mixer
}

/// `SeedSequence.generate_state(4, np.uint64)` from a pool.
pub fn generate_state(pool: &[u32; POOL]) -> [u64; 4] {
    let mut hc = INIT_B;
    let mut words = [0u32; 8];
    for (i, w) in words.iter_mut().enumerate() {
        let mut v = pool[i % POOL] ^ hc;
        hc = hc.wrapping_mul(MULT_B);
        v = v.wrapping_mul(hc);
        *w = v ^ (v >> XSHIFT);
    }
    // uint32 pairs viewed as little-endian uint64: low word first.
    std::array::from_fn(|i| u64::from(words[2 * i]) | (u64::from(words[2 * i + 1]) << 32))
}

#[derive(Clone, Debug)]
pub struct NumpyRng {
    state: u128,
    inc: u128,
}

impl NumpyRng {
    pub fn new(seed: u64) -> NumpyRng {
        let w = generate_state(&seed_sequence_pool(seed));
        let initstate = (u128::from(w[0]) << 64) | u128::from(w[1]);
        let initseq = (u128::from(w[2]) << 64) | u128::from(w[3]);
        let mut rng = NumpyRng {
            state: 0,
            inc: (initseq << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(initstate);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    /// Internal `(state, inc)`, as `PCG64(seed).state["state"]` reports it.
    pub fn state(&self) -> (u128, u128) {
        (self.state, self.inc)
    }

    /// `PCG64.random_raw()`: step, then XSL-RR output.
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let s = self.state;
        ((s >> 64) as u64 ^ s as u64).rotate_right((s >> 122) as u32)
    }

    /// `Generator.random()`: a double in [0, 1) from the top 53 bits.
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `Generator.standard_normal()`: numpy's `random_standard_normal` (distributions.c).
    pub fn standard_normal(&mut self) -> f64 {
        loop {
            let mut r = self.next_u64();
            let idx = (r & 0xff) as usize;
            r >>= 8;
            let sign = r & 0x1;
            let rabs = (r >> 1) & 0x000f_ffff_ffff_ffff;
            let mut x = rabs as f64 * f64::from_bits(WI_BITS[idx]);
            if sign & 0x1 != 0 {
                x = -x;
            }
            if rabs < KI[idx] {
                return x; // 99.3% of the time
            }
            if idx == 0 {
                // Tail: 1 - U avoids log(0) (numpy GH 13361).
                loop {
                    let xx = -ZIGGURAT_NOR_INV_R * (-self.random()).ln_1p();
                    let yy = -(-self.random()).ln_1p();
                    if yy + yy > xx * xx {
                        return if (rabs >> 8) & 0x1 != 0 {
                            -(ZIGGURAT_NOR_R + xx)
                        } else {
                            ZIGGURAT_NOR_R + xx
                        };
                    }
                }
            }
            let (f0, f1) = (
                f64::from_bits(FI_BITS[idx - 1]),
                f64::from_bits(FI_BITS[idx]),
            );
            if (f0 - f1) * self.random() + f1 < (-0.5 * x * x).exp() {
                return x;
            }
        }
    }
}
