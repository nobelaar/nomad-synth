//! `nomad devices | demo | bench | soak` (see USAGE).

use std::process::ExitCode;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use nomad_app::bench::{bench_engine, parse_bench, run_bench};
use nomad_app::{Args, scene};
use nomad_runtime::output::{AudioOutput, list_output_devices, negotiate};
use nomad_runtime::{Telemetry, create};

const USAGE: &str = "usage: nomad devices
       nomad demo  [--device N] [--seconds S]
       nomad bench [--voices 8] [--synths 2] [--seconds 10] [--block 512]
       nomad soak  [--minutes 10] [--device N] [--voices 8] [--synths 2]";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = match argv.first().map(String::as_str) {
        Some("devices") => devices(),
        Some("demo") => Args::parse(&argv[1..]).and_then(|a| demo(&a)),
        Some("bench") => {
            parse_bench(&argv[1..]).map(|o| bench(o.voices, o.synths, o.seconds, o.block))
        }
        Some("soak") => Args::parse(&argv[1..]).and_then(|a| soak(&a)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn devices() -> Result<ExitCode, String> {
    let list = list_output_devices();
    if list.is_empty() {
        println!("no output devices");
    }
    for d in list {
        println!(
            "{:>3}  {:<5} {:>2} ch {:>6} Hz  {}",
            d.index, d.host, d.channels, d.default_sample_rate, d.name
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn line(t: &Telemetry) -> String {
    format!(
        "callbacks {} xruns {} errors {} render_max {:.2} ms late_max {:.2} ms",
        t.callbacks.load(Relaxed),
        t.xruns.load(Relaxed),
        t.errors.load(Relaxed),
        t.render_ns_max.load(Relaxed) as f64 / 1e6,
        t.late_us_max.load(Relaxed) as f64 / 1e3,
    )
}

fn demo(a: &Args) -> Result<ExitCode, String> {
    a.only(&["device", "seconds"])?;
    let device: Option<usize> = a.opt("device")?;
    let seconds: f64 = a.get("seconds", 10.0)?;
    let (index, rate) = negotiate(device, 48000)?;
    println!("device {index} at {rate} Hz");
    let (core, mut ctl) = create(scene::demo_engine(f64::from(rate)), f64::from(rate));
    let tel = ctl.telemetry();
    let mut out = AudioOutput::start(core, Some(index), rate, 512)?;
    let start = Instant::now();
    let mut next = 1.0;
    while start.elapsed().as_secs_f64() < seconds && !out.device_lost() {
        std::thread::sleep(Duration::from_millis(50));
        ctl.reap();
        if start.elapsed().as_secs_f64() >= next {
            println!("{:>4.0} s  {}", next, line(&tel));
            next += 1.0;
        }
    }
    out.stop();
    let lost = out.device_lost();
    println!(
        "summary: {}{}",
        line(&tel),
        if lost { " (device lost)" } else { "" }
    );
    Ok(if lost || tel.errors.load(Relaxed) > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn bench(voices: usize, synths: usize, seconds: f64, block: usize) -> ExitCode {
    let r = run_bench(voices, synths, seconds, block);
    println!(
        "bench: {synths} synth(s) x {voices} voices + drums, {seconds} s, block {block} @ 48 kHz"
    );
    println!(
        "blocks {}  mean {:.1}%  p50 {:.1}%  p99 {:.1}%  max {:.1}%  over budget {}",
        r.blocks, r.mean_pct, r.p50_pct, r.p99_pct, r.max_pct, r.over_budget
    );
    if r.max_pct > 100.0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn soak(a: &Args) -> Result<ExitCode, String> {
    a.only(&["minutes", "device", "voices", "synths"])?;
    let minutes: f64 = a.get("minutes", 10.0)?;
    let device: Option<usize> = a.opt("device")?;
    let (voices, synths): (usize, usize) = (a.get("voices", 8)?, a.get("synths", 2)?);
    parse_bench(&[
        "--voices".into(),
        voices.to_string(),
        "--synths".into(),
        synths.to_string(),
    ])?;
    let (index, rate) = negotiate(device, 48000)?;
    println!(
        "soak: {minutes} min on device {index} at {rate} Hz, {synths} synth(s) x {voices} voices + drums"
    );
    let (core, mut ctl) = create(
        bench_engine(f64::from(rate), voices, synths),
        f64::from(rate),
    );
    let tel = ctl.telemetry();
    let mut out = AudioOutput::start(core, Some(index), rate, 512)?;
    let start = Instant::now();
    let total = minutes * 60.0;
    let mut next = 10.0;
    while start.elapsed().as_secs_f64() < total && !out.device_lost() {
        std::thread::sleep(Duration::from_millis(100));
        ctl.reap();
        if start.elapsed().as_secs_f64() >= next {
            println!("{:>5.0} s  {}", next, line(&tel));
            next += 10.0;
        }
    }
    out.stop();
    let lost = out.device_lost();
    println!(
        "summary: {}{}",
        line(&tel),
        if lost { " (device lost)" } else { "" }
    );
    let clean = !lost && tel.xruns.load(Relaxed) == 0 && tel.errors.load(Relaxed) == 0;
    Ok(if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
