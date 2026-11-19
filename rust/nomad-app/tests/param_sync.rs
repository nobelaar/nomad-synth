use std::time::{Duration, Instant};

use nomad_app::param_sync::{ParamSync, SynthParams};

fn ms(t: Instant, n: u64) -> Instant {
    t + Duration::from_millis(n)
}

#[test]
fn drag_across_frames_accumulates() {
    let t = Instant::now();
    let mut p = ParamSync::new(0.5);
    for k in 0..5u64 {
        let v = 0.5 + 0.01 * (k + 1) as f64;
        p.user_set(v, ms(t, 16 * k));
        // The mirror still holds the old value: it must not undo the drag.
        p.observe(0.5, ms(t, 16 * k + 8));
    }
    assert!((p.value() - 0.55).abs() < 1e-12);
    assert!(p.needs_send());
    p.mark_sent();
    assert!(!p.needs_send());
}

#[test]
fn mirror_wins_after_window() {
    let t = Instant::now();
    let mut p = ParamSync::new(0.5);
    p.user_set(0.6, t);
    p.observe(0.9, ms(t, 200));
    assert_eq!(p.value(), 0.9);
}

#[test]
fn mirror_ignored_inside_window() {
    let t = Instant::now();
    let mut p = ParamSync::new(0.5);
    p.user_set(0.6, t);
    p.observe(0.9, ms(t, 50));
    assert_eq!(p.value(), 0.6);
}

#[test]
fn equal_mirror_is_noop() {
    let t = Instant::now();
    let mut p = ParamSync::new(1000.0);
    p.observe(1000.0 * (1.0 + 1e-12), ms(t, 500));
    assert_eq!(p.value(), 1000.0);
    // An external change with nobody touching the knob shows up at once.
    p.observe(1200.0, ms(t, 501));
    assert_eq!(p.value(), 1200.0);
}

#[test]
fn non_finite_mirror_is_ignored() {
    let t = Instant::now();
    let mut p = ParamSync::new(0.3);
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        p.observe(x, ms(t, 500));
    }
    assert_eq!(p.value(), 0.3);
}

#[test]
fn synth_params_ui_value() {
    let t = Instant::now();
    let mut s = SynthParams::new(&[0.0; 63]);
    assert_eq!(s.ui_value(5, 0.7, t), 0.7);
    s.user_set(5, 0.2, t);
    assert_eq!(s.ui_value(5, 0.7, ms(t, 10)), 0.2);
}
