//! Offline render benchmark (`nomad bench`) and its scene, shared with the live
//! `nomad soak`: `synths` synths of `voices` sustained voices each, a drum machine
//! on its default pattern, and the first synth's sequencer running.

use std::time::Instant;

use nomad_core::engine::{Engine, Kind, MAX_INSTANCES};
use nomad_core::params::pid;

use crate::Args;
use crate::scene::{DRUMS, SYNTH};

const WARMUP: usize = 50;
const SR: f64 = 48000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchOpts {
    pub voices: usize,
    pub synths: usize,
    pub seconds: f64,
    pub block: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchReport {
    /// Blocks rendered, warm-up included; the percentages skip the first 50.
    pub blocks: usize,
    pub mean_pct: f64,
    pub p50_pct: f64,
    pub p99_pct: f64,
    pub max_pct: f64,
    /// Measured blocks that took longer than their real-time budget.
    pub over_budget: usize,
}

/// `[--voices 8] [--synths 2] [--seconds 10] [--block 512]`.
pub fn parse_bench(rest: &[String]) -> Result<BenchOpts, String> {
    let a = Args::parse(rest)?;
    a.only(&["voices", "synths", "seconds", "block"])?;
    let o = BenchOpts {
        voices: a.get("voices", 8)?,
        synths: a.get("synths", 2)?,
        seconds: a.get("seconds", 10.0)?,
        block: a.get("block", 512)?,
    };
    if !(1..=8).contains(&o.voices) {
        return Err("--voices must be 1..8".into());
    }
    // One slot is the drum machine.
    if !(1..MAX_INSTANCES).contains(&o.synths) {
        return Err(format!("--synths must be 1..{}", MAX_INSTANCES - 1));
    }
    if o.block == 0 || !(o.seconds.is_finite() && o.seconds > 0.0) {
        return Err("--block and --seconds must be positive".into());
    }
    Ok(o)
}

/// The measurement scene, with both transports running.
pub fn bench_engine(sample_rate: f64, voices: usize, synths: usize) -> Engine {
    let mut e = Engine::new(sample_rate, 8);
    let mut ids = vec![SYNTH];
    for _ in 1..synths {
        ids.push(e.add_instrument(Kind::Synth));
    }
    for (k, &id) in ids.iter().enumerate() {
        if let Some(s) = e.synth_mut(id) {
            s.set_param(pid::POLYPHONY, voices as f64);
        }
        e.set_target_synth(id);
        for v in 0..voices {
            e.note_on(40 + 5 * k as i64 + 3 * v as i64, 0.7);
        }
    }
    if let Some(s) = e.synth_mut(SYNTH) {
        let sq = &mut s.sequencer;
        for step in 0..16 {
            sq.set_step(step, true, Some(60 + (step as i64 % 5) * 2), None);
        }
        sq.enabled = true;
    }
    e.set_target_synth(SYNTH);
    e.start(SYNTH);
    e.start(DRUMS);
    e
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i]
}

/// Render `seconds` offline in `block`-frame calls and time each against its budget.
pub fn run_bench(voices: usize, synths: usize, seconds: f64, block: usize) -> BenchReport {
    let mut e = bench_engine(SR, voices, synths);
    let budget = block as f64 / SR;
    let blocks = ((seconds * SR) as usize / block).max(WARMUP + 1);
    let mut out = vec![0.0; block];
    let mut p = Vec::with_capacity(blocks);
    for k in 0..blocks {
        let t = Instant::now();
        e.render(&mut out);
        let dt = t.elapsed().as_secs_f64();
        if k >= WARMUP {
            p.push(dt / budget * 100.0);
        }
    }
    let over_budget = p.iter().filter(|&&x| x > 100.0).count();
    let mean_pct = p.iter().sum::<f64>() / p.len() as f64;
    p.sort_by(f64::total_cmp);
    BenchReport {
        blocks,
        mean_pct,
        p50_pct: pct(&p, 0.5),
        p99_pct: pct(&p, 0.99),
        max_pct: p[p.len() - 1],
        over_budget,
    }
}
