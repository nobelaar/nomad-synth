mod common;

use common::{Blob, assert_close, ints, num};
use nomad_core::oscillator::{MAX_INC, Oscillator, Wave};

const SR: f64 = 48000.0;

/// Slave synced to a master whose phase is built like `Voice.render` in Python.
fn render_synced(
    wave: Wave,
    slave_hz: f64,
    master_hz: f64,
    blocks: &[usize],
    synced: &[bool],
) -> (Vec<f64>, f64) {
    let mut master = Oscillator::new(SR);
    let mut slave = Oscillator::new(SR);
    slave.wave = wave;
    let minc = master_hz.abs().min(MAX_INC * SR) / SR;
    let mut all = Vec::new();
    for (&n, &on) in blocks.iter().zip(synced) {
        let mp: Vec<f64> = (0..n)
            .map(|i| (i as f64 * minc + master.phase).rem_euclid(1.0))
            .collect();
        master.render_into(&mut vec![0.0; n], master_hz, 0.5, None);
        let mut buf = vec![0.0; n];
        slave.render_into(&mut buf, slave_hz, 0.5, on.then_some(mp.as_slice()));
        all.extend(buf);
    }
    (all, slave.phase)
}

fn check(blob: &Blob, label: &str) -> Vec<f64> {
    let m = blob.meta(label);
    let wave = Wave::from_index(m["wave"].as_i64().unwrap()).unwrap();
    let synced: Vec<bool> = m["synced"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_bool().unwrap())
        .collect();
    let (got, phase) = render_synced(
        wave,
        num(&m["slave_hz"]),
        num(&m["master_hz"]),
        &ints(&m["blocks"]),
        &synced,
    );
    assert_close(&got, blob.get(label), label);
    let want = num(&m["phase_end"]);
    assert!(
        (phase - want).abs() <= 1e-12,
        "{label}: phase {phase} vs {want}"
    );
    got
}

#[test]
fn sync_matches_python() {
    let blob = Blob::open("oscillator_sync");
    let labels = blob.labels();
    assert_eq!(labels.len(), 18);
    for label in &labels {
        check(&blob, label);
    }
}

#[test]
fn sync_boundary_wrap_detected() {
    let blob = Blob::open("oscillator_sync");
    let synced = check(&blob, "sync/boundary");
    let (free, _) = render_synced(Wave::Saw, 300.0, 750.0, &[64; 8], &[false; 8]);
    // Block 0 has no previous master sample, so only later block starts reset.
    for k in 1..8 {
        assert!((synced[k * 64] - free[k * 64]).abs() > 1e-3, "block {k}");
    }
}

#[test]
fn sync_len_mismatch_writes_nothing() {
    let mut o = Oscillator::new(SR);
    let master = vec![0.5; 10];
    let mut out = vec![0.0; 64];
    let phase = o.render_into(&mut out, 300.0, 0.5, Some(&master));
    assert!(out.iter().all(|&x| x == 0.0));
    assert_eq!(phase, 0.0);
}

#[test]
fn free_run_clears_last_master() {
    let blob = Blob::open("oscillator_sync");
    let after_free = check(&blob, "sync/after_free");
    // If the stale master sample were kept, block 2 would reset at its first sample
    // exactly like an all-synced run does.
    let (all_synced, _) = render_synced(Wave::Saw, 300.0, 750.0, &[64; 3], &[true; 3]);
    assert!((after_free[128] - all_synced[128]).abs() > 1e-3);
}
