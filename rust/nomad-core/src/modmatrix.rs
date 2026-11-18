//! Modulation matrix: eight slots, each routing one source to one destination,
//! ported from `nomad/engine/modmatrix.py`.
//!
//! Sources: 0 off, 1 lfo1, 2 lfo2, 3 modwheel, 4 velocity, 5 env2.
//! Destinations: 0 off, 1 pitch, 2 pitch1, 3 pitch2, 4 amp, 5 cutoff, 6 pw1, 7 pw2.

pub const NUM_SLOTS: usize = 8;
const NUM_SOURCES: usize = 6;
const NUM_DESTINATIONS: usize = 8;

#[derive(Clone, Copy, Default, Debug)]
pub struct Sources {
    pub lfo1: f64,
    pub lfo2: f64,
    pub modwheel: f64,
    pub velocity: f64,
    pub env2: f64,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Destinations {
    pub pitch: f64,
    pub pitch1: f64,
    pub pitch2: f64,
    pub amp: f64,
    pub cutoff: f64,
    pub pw1: f64,
    pub pw2: f64,
}

#[derive(Debug, PartialEq)]
pub enum SlotError {
    Slot(usize),
    Source(usize),
    Destination(usize),
}

#[derive(Default)]
pub struct ModMatrix {
    src: [usize; NUM_SLOTS],
    dst: [usize; NUM_SLOTS],
    amt: [f64; NUM_SLOTS],
    out: Destinations,
}

impl ModMatrix {
    pub fn new() -> ModMatrix {
        ModMatrix::default()
    }

    /// Configure slot `i`; bad indices return `Err` untouched, a non-finite amount is 0.
    pub fn set_slot(
        &mut self,
        i: usize,
        src: usize,
        dst: usize,
        amount: f64,
    ) -> Result<(), SlotError> {
        if i >= NUM_SLOTS {
            return Err(SlotError::Slot(i));
        }
        if src >= NUM_SOURCES {
            return Err(SlotError::Source(src));
        }
        if dst >= NUM_DESTINATIONS {
            return Err(SlotError::Destination(dst));
        }
        self.src[i] = src;
        self.dst[i] = dst;
        self.amt[i] = if amount.is_finite() {
            amount.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Ok(())
    }

    /// Sum `amount * source` per destination, in slot order. The result is owned by
    /// the matrix and overwritten on the next call.
    pub fn resolve(&mut self, sources: &Sources) -> &Destinations {
        let mut out = Destinations::default();
        for i in 0..NUM_SLOTS {
            let s = match self.src[i] {
                1 => sources.lfo1,
                2 => sources.lfo2,
                3 => sources.modwheel,
                4 => sources.velocity,
                5 => sources.env2,
                _ => continue,
            };
            let d = match self.dst[i] {
                1 => &mut out.pitch,
                2 => &mut out.pitch1,
                3 => &mut out.pitch2,
                4 => &mut out.amp,
                5 => &mut out.cutoff,
                6 => &mut out.pw1,
                7 => &mut out.pw2,
                _ => continue,
            };
            *d += self.amt[i] * s;
        }
        self.out = out;
        &self.out
    }
}
