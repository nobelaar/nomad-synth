//! `nomad devices | demo [--device N] [--seconds S]`.

use std::process::ExitCode;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use nomad_app::{Args, scene};
use nomad_runtime::output::{AudioOutput, list_output_devices, negotiate};
use nomad_runtime::{Telemetry, create};

const USAGE: &str = "usage: nomad devices | nomad demo [--device N] [--seconds S]";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = match argv.first().map(String::as_str) {
        Some("devices") => devices(),
        Some("demo") => Args::parse(&argv[1..]).and_then(|a| demo(&a)),
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
