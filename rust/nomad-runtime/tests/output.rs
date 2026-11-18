use nomad_runtime::output::{
    AudioOutput, DeviceInfo, LateTracker, list_output_devices, negotiate, pick_device,
};

fn dev(index: usize, host: &str, channels: u16) -> DeviceInfo {
    DeviceInfo {
        index,
        name: format!("dev{index}"),
        host: host.into(),
        channels,
        default_sample_rate: 48000,
    }
}

#[test]
fn pick_device_prefers_jack_stereo() {
    let d = [dev(0, "ALSA", 2), dev(1, "JACK", 1), dev(2, "JACK", 2)];
    assert_eq!(pick_device(&d, Some(0)), Ok(2));
}

#[test]
fn pick_device_takes_pipewire_as_the_graph() {
    let mut d = [dev(0, "ALSA", 2), dev(1, "ALSA", 2)];
    d[1].name = "PipeWire Sound Server (alsa:pipewire)".into();
    assert_eq!(pick_device(&d, Some(0)), Ok(1));
}

#[test]
fn pick_device_falls_back_to_default() {
    let d = [dev(0, "ALSA", 2), dev(1, "ALSA", 6)];
    assert_eq!(pick_device(&d, Some(1)), Ok(1));
    // A default index that is not an output device does not count.
    assert!(pick_device(&d, Some(7)).is_err());
}

#[test]
fn pick_device_errors_without_devices() {
    assert_eq!(
        pick_device(&[], None),
        Err(
            "No audio output device available. Check that PipeWire/JACK or ALSA exposes an output."
                .to_string()
        )
    );
}

const SR: f64 = 48000.0;
const BLOCK: usize = 512;
const T: f64 = BLOCK as f64 / SR;

#[test]
fn late_tracker_ignores_burst_of_two() {
    // JACK-style: two blocks delivered together every 2*T, timestamps 0.1 ms apart.
    let mut lt = LateTracker::new(SR);
    let mut xruns = 0;
    for k in 0..200 {
        let t = (k / 2) as f64 * 2.0 * T + (k % 2) as f64 * 1e-4;
        xruns += usize::from(lt.observe(t, BLOCK).xrun);
    }
    assert_eq!(xruns, 0);
}

#[test]
fn late_tracker_counts_real_delay() {
    let mut lt = LateTracker::new(SR);
    let mut events = Vec::new();
    for k in 0..20 {
        // Callback 10 and later arrive 3 blocks late.
        let t = k as f64 * T + if k >= 10 { 3.0 * T } else { 0.0 };
        events.push(lt.observe(t, BLOCK));
    }
    let xruns: Vec<usize> = (0..20).filter(|&k| events[k].xrun).collect();
    assert_eq!(xruns, [10]);
    assert!(
        (events[10].late_ms - 3.0 * T * 1000.0).abs() < 1e-6,
        "{}",
        events[10].late_ms
    );
}

#[test]
fn late_tracker_reanchors_after_xrun() {
    let mut lt = LateTracker::new(SR);
    for k in 0..5 {
        lt.observe(k as f64 * T, BLOCK);
    }
    assert!(lt.observe(5.0 * T + 0.2, BLOCK).xrun);
    // Back on a regular cadence from the new anchor: no more xruns, ~0 lateness.
    for k in 6..20 {
        let e = lt.observe(k as f64 * T + 0.2, BLOCK);
        assert!(!e.xrun && e.late_ms.abs() < 1e-6, "k {k}: {}", e.late_ms);
    }
}

#[test]
fn list_devices_does_not_panic() {
    let devices = list_output_devices();
    for (i, d) in devices.iter().enumerate() {
        assert_eq!(d.index, i);
        assert!(d.channels >= 1);
    }
}

#[test]
#[ignore = "requires an audio device"]
fn plays_one_second_on_default_device() {
    use nomad_core::engine::Engine;
    let (index, rate) = negotiate(None, 48000).unwrap();
    let (core, mut ctl) = nomad_runtime::create(Engine::new(f64::from(rate), 8), f64::from(rate));
    ctl.send(nomad_runtime::Command::NoteOn {
        note: 60,
        velocity: 0.5,
    });
    let mut out = AudioOutput::start(core, Some(index), rate, 512).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    out.stop();
    let t = ctl.telemetry();
    assert!(t.callbacks.load(std::sync::atomic::Ordering::Relaxed) > 10);
    assert_eq!(t.errors.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert!(!out.device_lost());
}
