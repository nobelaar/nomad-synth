//! `nomad devices | demo | bench | soak` (see USAGE).

use std::process::ExitCode;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nomad_app::bench::{bench_engine, parse_bench, run_bench};
use nomad_app::{Args, scene};
use nomad_runtime::midi::input::{MidiInput, list_input_ports};
use nomad_runtime::midi::map::{MidiMap, default_config_dir};
use nomad_runtime::output::{AudioOutput, list_output_devices, negotiate};
use nomad_runtime::{ControlHandle, Telemetry, create};

const USAGE: &str = "usage: nomad [gui] [--mcp [PORT]]
       nomad --version
       nomad devices
       nomad midi  [--port TEXT] [--seconds 20]
       nomad demo  [--device N] [--seconds S] [--midi [TEXT]] [--midi-map PATH]
       nomad bench [--voices 8] [--synths 2] [--seconds 10] [--block 512]
       nomad soak  [--minutes 10] [--device N] [--voices 8] [--synths 2]";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = match argv.first().map(String::as_str) {
        None | Some("gui") => gui(argv.get(1..).unwrap_or(&[])),
        Some("--mcp") => gui(&argv),
        Some("--version" | "-V") => {
            println!("nomad {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("devices") => devices(),
        Some("midi") => Args::parse(&argv[1..]).and_then(|a| midi(&a)),
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

/// Open the MIDI input for `demo` (the map file is only read, never saved).
fn open_midi(a: &Args, ctl: &mut ControlHandle) -> Result<Option<MidiInput>, String> {
    let Some(text) = a.opt::<String>("midi")? else {
        return Ok(None);
    };
    let path = a
        .opt::<String>("midi-map")?
        .map_or_else(|| default_config_dir().join("midi-map.json"), Into::into);
    let mut map = MidiMap::new(Some(path.clone()));
    map.load();
    match &map.error {
        Some(e) => println!("midi map: {e} (empty map)"),
        None => println!(
            "midi map: {} binding(s) from {}",
            map.bindings().len(),
            path.display()
        ),
    }
    let sender = ctl.midi_sender().ok_or("MIDI sender already taken")?;
    let port = (!text.is_empty()).then_some(text.as_str());
    match MidiInput::open(port, sender, Arc::new(Mutex::new(map))) {
        Ok(m) => {
            println!("midi in: {}", m.port_name());
            Ok(Some(m))
        }
        Err(e) => {
            println!("midi in: {e} (playing without MIDI)");
            Ok(None)
        }
    }
}

fn midi(a: &Args) -> Result<ExitCode, String> {
    a.only(&["port", "seconds"])?;
    let port: Option<String> = a.opt("port")?;
    let seconds: f64 = a.get("seconds", 20.0)?;
    let ports = list_input_ports();
    println!("MIDI input ports:");
    for p in &ports {
        println!("  {p}");
    }
    let (mut core, mut ctl) = create(scene::demo_engine(48000.0), 48000.0);
    let sender = ctl.midi_sender().ok_or("MIDI sender already taken")?;
    let map = MidiMap::new(Some(default_config_dir().join("midi-map.json")));
    let mut map = map;
    map.load();
    let input = MidiInput::open(
        port.as_deref().filter(|p| !p.is_empty()),
        sender,
        Arc::new(Mutex::new(map)),
    )?;
    println!(
        "listening on {} for {seconds} s (Ctrl-C to stop)",
        input.port_name()
    );
    let lines = input.monitor();
    let start = Instant::now();
    let mut buf = vec![0.0f32; 480];
    let mut events = 0;
    let mut checked = 0.0;
    while start.elapsed().as_secs_f64() < seconds {
        std::thread::sleep(Duration::from_millis(10));
        core.render_stereo(&mut buf, 1); // drains the MIDI queue into the engine (no device)
        ctl.reap();
        for l in lines.try_iter() {
            events += 1;
            println!("{:>7.3} s  {l}", start.elapsed().as_secs_f64());
        }
        let t = start.elapsed().as_secs_f64();
        if t - checked >= 1.0 {
            checked = t;
            if !input.is_alive() {
                input.reset();
                println!("port {} went away", input.port_name());
                return Ok(ExitCode::FAILURE);
            }
        }
    }
    if events == 0 {
        println!("no events in {seconds} s");
    }
    let tel = ctl.telemetry();
    println!(
        "midi events {} errors {} dropped {}",
        tel.midi_events.load(Relaxed),
        tel.midi_errors.load(Relaxed),
        tel.midi_dropped.load(Relaxed)
    );
    Ok(ExitCode::SUCCESS)
}

fn demo(a: &Args) -> Result<ExitCode, String> {
    a.only(&["device", "seconds", "midi", "midi-map"])?;
    let device: Option<usize> = a.opt("device")?;
    let seconds: f64 = a.get("seconds", 10.0)?;
    let (index, rate) = negotiate(device, 48000)?;
    println!("device {index} at {rate} Hz");
    let (core, mut ctl) = create(scene::demo_engine(f64::from(rate)), f64::from(rate));
    let tel = ctl.telemetry();
    let midi_in = open_midi(a, &mut ctl)?;
    let mut out = AudioOutput::start(core, Some(index), rate, 512)?;
    let start = Instant::now();
    let mut next = 1.0;
    while start.elapsed().as_secs_f64() < seconds && !out.device_lost() {
        std::thread::sleep(Duration::from_millis(50));
        ctl.reap();
        if start.elapsed().as_secs_f64() >= next {
            let midi = midi_in.as_ref().map_or(String::new(), |m| {
                let alive = m.is_alive();
                if !alive {
                    m.reset(); // nothing may hang when the port goes away
                }
                let state = if alive { "ok" } else { "lost" };
                format!("  midi {} ({state})", tel.midi_events.load(Relaxed))
            });
            println!("{:>4.0} s  {}{midi}", next, line(&tel));
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

/// The window: engine + audio + MIDI (read-only map), as `demo --midi`; if audio
/// or MIDI fail the window still opens and says so in its status line.
fn gui(args: &[String]) -> Result<ExitCode, String> {
    use nomad_app::model::{AppModel, initial_views};
    use nomad_app::ui::NomadApp;
    use nomad_core::engine::Engine;

    let mcp_port = nomad_app::mcp::mcp_port(args)?;
    let mut status = Vec::new();
    let negotiated = negotiate(None, 48000);
    let rate = negotiated.as_ref().map_or(48000, |&(_, r)| r);
    let engine = Engine::new(f64::from(rate), 8);
    let views = initial_views(&engine);
    let (core, mut ctl) = create(engine, f64::from(rate));
    let telemetry = ctl.telemetry();
    let mut map = MidiMap::new(Some(default_config_dir().join("midi-map.json")));
    map.load();
    if let Some(e) = &map.error {
        status.push(format!("MIDI map: {e}"));
    }
    let map = Arc::new(Mutex::new(map));
    // A port or device that does not open hands back its sender/core: the header
    // menus can then open another one without restarting.
    let mut parked_midi = None;
    let midi = match ctl
        .midi_sender()
        .map(|s| MidiInput::open_or_return(None, s, Arc::clone(&map)))
    {
        Some(Ok(m)) => Some(m),
        Some(Err((e, s))) => {
            status.push(format!("MIDI: {e}"));
            parked_midi = Some(s);
            None
        }
        None => None,
    };
    let mut parked_core = None;
    let device = negotiated.as_ref().ok().map(|&(i, _)| i);
    let output = match negotiated {
        Ok((index, rate)) => match AudioOutput::start_or_return(core, Some(index), rate, 512) {
            Ok(o) => Some(o),
            Err((e, core)) => {
                status.push(format!("AUDIO: {e}"));
                parked_core = Some(core);
                None
            }
        },
        Err(e) => {
            status.push(format!("AUDIO: {e}"));
            parked_core = Some(core);
            None
        }
    };
    let model = AppModel::new(ctl, f64::from(rate), views);
    let mut app = NomadApp::new(model, output, midi, map, telemetry);
    app.sample_rate = f64::from(rate);
    app.opener = nomad_app::devices::real_opener(rate);
    app.audio_device = device;
    app.parked_core = parked_core;
    app.parked_midi = parked_midi;
    app.presets = nomad_app::presets::PresetStore::new(Some(default_config_dir().join("presets")));
    app.status = status.join("  ");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NOMAD SYNTH")
            .with_inner_size([1366.0, 768.0])
            .with_min_inner_size([1000.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "NOMAD SYNTH",
        options,
        Box::new(move |cc| {
            nomad_app::theme::apply(&cc.egui_ctx);
            if let Some(port) = mcp_port {
                match nomad_app::mcp::Mcp::start(port, cc.egui_ctx.clone()) {
                    Ok(m) => {
                        let msg = format!("MCP en http://127.0.0.1:{}/mcp", m.port());
                        nomad_app::mcp::append_status(&mut app.status, &msg);
                        app.mcp = Some(m);
                    }
                    Err(e) => nomad_app::mcp::append_status(&mut app.status, &format!("MCP: {e}")),
                }
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| format!("window: {e}"))?;
    Ok(ExitCode::SUCCESS)
}
