//! Counters written by the audio thread and read anywhere (relaxed atomics), and
//! the oscilloscope ring.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub struct Telemetry {
    pub callbacks: AtomicU64,
    pub xruns: AtomicU64,
    pub errors: AtomicU64,
    pub dropped_commands: AtomicU64,
    pub render_ns_last: AtomicU64,
    pub render_ns_max: AtomicU64,
    pub late_us_max: AtomicU64,
    pub scope: ScopeRing,
}

impl Telemetry {
    pub fn new(sample_rate: f64) -> Telemetry {
        Telemetry {
            callbacks: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            dropped_commands: AtomicU64::new(0),
            render_ns_last: AtomicU64::new(0),
            render_ns_max: AtomicU64::new(0),
            late_us_max: AtomicU64::new(0),
            scope: ScopeRing::new((2.0 * sample_rate) as usize),
        }
    }

    pub(crate) fn render_time(&self, ns: u64) {
        self.render_ns_last.store(ns, Ordering::Relaxed);
        self.render_ns_max.fetch_max(ns, Ordering::Relaxed);
    }
}

/// Single-writer ring of the last samples (f32 bits in atomics, no locks). A
/// reader racing the writer may see a torn window; harmless for a scope.
pub struct ScopeRing {
    data: Box<[AtomicU32]>,
    /// Total samples ever written (the next write goes to `pos % len`).
    pos: AtomicUsize,
}

impl ScopeRing {
    pub fn new(len: usize) -> ScopeRing {
        ScopeRing {
            data: (0..len.max(1)).map(|_| AtomicU32::new(0)).collect(),
            pos: AtomicUsize::new(0),
        }
    }

    /// Audio thread: append samples.
    pub fn write(&self, samples: &[f32]) {
        let len = self.data.len();
        let mut p = self.pos.load(Ordering::Relaxed);
        for &s in samples {
            self.data[p % len].store(s.to_bits(), Ordering::Relaxed);
            p = p.wrapping_add(1);
        }
        self.pos.store(p, Ordering::Release);
    }

    /// Reader: copy the latest `min(n, out.len(), capacity, written)` samples,
    /// oldest first, into `out`; returns how many.
    pub fn window(&self, n: usize, out: &mut [f32]) -> usize {
        let len = self.data.len();
        let end = self.pos.load(Ordering::Acquire);
        let k = n.min(out.len()).min(len).min(end);
        for (i, o) in out[..k].iter_mut().enumerate() {
            let idx = (end - k + i) % len;
            *o = f32::from_bits(self.data[idx].load(Ordering::Relaxed));
        }
        k
    }
}
