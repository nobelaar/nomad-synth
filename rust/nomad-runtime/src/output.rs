//! Audio output through `cpal`: device listing and choice (ported from
//! `nomad/audio/stream.py`), the late-callback xrun heuristic, and a stream that
//! runs an [`AudioCore`] in the device callback.
//!
//! The JACK host is not compiled in (cpal's `jack` feature): on Linux PipeWire
//! exposes its graph through the default ALSA device, which is what gets picked.
//! `pick_device` still applies the Python rule for hosts named "JACK".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig, StreamInstant,
    SupportedBufferSize,
};

use crate::core::AudioCore;
use crate::telemetry::Telemetry;

const NO_DEVICE: &str =
    "No audio output device available. Check that PipeWire/JACK or ALSA exposes an output.";
/// A callback more than this many blocks late counts one xrun.
pub const LATE_FACTOR: f64 = 1.5;
/// Scratch for devices whose sample format is not f32 (frames x channels).
const CONVERT: usize = 8192;

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub index: usize,
    pub name: String,
    pub host: String,
    pub channels: u16,
    pub default_sample_rate: u32,
}

fn info(index: usize, host: &str, d: &cpal::Device) -> Option<DeviceInfo> {
    let cfg = d.default_output_config().ok()?;
    let desc = d
        .description()
        .map(|x| x.name().to_string())
        .unwrap_or_else(|_| d.to_string());
    // Names repeat across ALSA aliases (hw:, plughw:, ...): show the device id too.
    let name = match d.id() {
        Ok(id) => format!("{desc} ({id})"),
        Err(_) => desc,
    };
    Some(DeviceInfo {
        index,
        name,
        host: host.to_string(),
        channels: cfg.channels(),
        default_sample_rate: cfg.sample_rate(),
    })
}

/// Output devices of every available host, indexed in enumeration order. The
/// system default comes first: ALSA's "default" (PipeWire on this kind of setup)
/// is not part of `output_devices()`.
fn enumerate() -> (Vec<(DeviceInfo, cpal::Device)>, Option<usize>) {
    let mut out: Vec<(DeviceInfo, cpal::Device)> = Vec::new();
    let mut default = None;
    let host = cpal::default_host();
    if let Some(d) = host.default_output_device()
        && let Some(i) = info(0, host.id().name(), &d)
    {
        out.push((i, d));
        default = Some(0);
    }
    for host_id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(host_id) else {
            continue;
        };
        let Ok(devices) = host.output_devices() else {
            continue;
        };
        for d in devices {
            if out.iter().any(|(_, x)| *x == d) {
                continue;
            }
            if let Some(i) = info(out.len(), host_id.name(), &d) {
                out.push((i, d));
            }
        }
    }
    (out, default)
}

/// Every output device (empty if there is none or the host fails; never panics).
pub fn list_output_devices() -> Vec<DeviceInfo> {
    enumerate().0.into_iter().map(|(i, _)| i).collect()
}

/// The system default output device's index, if any.
pub fn default_output_index() -> Option<usize> {
    enumerate().1
}

/// JACK (or PipeWire, which stands in for it without cpal's JACK host) with 2+
/// outputs, else the system default, else an error. Python's rule plus PipeWire.
pub fn pick_device(devices: &[DeviceInfo], default_index: Option<usize>) -> Result<usize, String> {
    let graph = |d: &DeviceInfo| d.host.eq_ignore_ascii_case("jack") || d.name.contains("PipeWire");
    if let Some(d) = devices.iter().find(|d| graph(d) && d.channels >= 2) {
        return Ok(d.index);
    }
    match default_index {
        Some(i) if devices.iter().any(|d| d.index == i) => Ok(i),
        _ => Err(NO_DEVICE.to_string()),
    }
}

fn resolve(device: Option<usize>) -> Result<(DeviceInfo, cpal::Device), String> {
    let (mut all, default) = enumerate();
    let infos: Vec<DeviceInfo> = all.iter().map(|(i, _)| i.clone()).collect();
    let index = match device {
        Some(i) if i < all.len() => i,
        Some(i) => return Err(format!("No output device with index {i}.")),
        None => pick_device(&infos, default)?,
    };
    Ok(all.swap_remove(index))
}

/// The device to open and the sample rate to build the engine with: `sample_rate`
/// if the device supports it, else its default rate.
pub fn negotiate(device: Option<usize>, sample_rate: u32) -> Result<(usize, u32), String> {
    let (info, d) = resolve(device)?;
    let supported = d
        .supported_output_configs()
        .map(|mut it| {
            it.any(|r| r.min_sample_rate() <= sample_rate && sample_rate <= r.max_sample_rate())
        })
        .unwrap_or(false);
    Ok((
        info.index,
        if supported {
            sample_rate
        } else {
            info.default_sample_rate
        },
    ))
}

/// Port of the lateness check in `AudioStream.callback`: a cumulative anchor, so
/// hosts that deliver blocks in bursts (JACK) do not look late.
pub struct LateTracker {
    sr: f64,
    anchor: Option<f64>,
    done: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LateEvent {
    pub xrun: bool,
    pub late_ms: f64,
}

impl LateTracker {
    pub fn new(sample_rate: f64) -> LateTracker {
        LateTracker {
            sr: sample_rate,
            anchor: None,
            done: 0,
        }
    }

    /// A callback of `frames` frames at time `t_seconds` (any fixed origin).
    // shortcut: assumes bursts of <= 2 blocks (JACK period <= 2*block); deeper
    // bursts can yield one false xrun each, as in Python. cpal's Xrun error is authoritative.
    pub fn observe(&mut self, t_seconds: f64, frames: usize) -> LateEvent {
        let nominal = frames as f64 / self.sr;
        let anchor = *self.anchor.get_or_insert(t_seconds);
        let expected = self.done as f64 / self.sr;
        let late = t_seconds - (anchor + expected);
        let xrun = late > nominal * LATE_FACTOR;
        if xrun || late < 0.0 {
            self.anchor = Some(t_seconds - expected);
        }
        self.done += frames as u64;
        LateEvent {
            xrun,
            late_ms: late * 1000.0,
        }
    }
}

/// The audio-thread core, shared by the device callback and whoever closes the
/// stream, so the engine (and every pattern in it) survives a device change.
#[derive(Clone)]
pub struct CoreSlot(Arc<Mutex<Option<AudioCore>>>);

impl CoreSlot {
    pub fn new(core: AudioCore) -> CoreSlot {
        CoreSlot(Arc::new(Mutex::new(Some(core))))
    }

    /// The callback's access: `None` (play silence) while the control side holds it.
    // shortcut: try_lock in the callback; only contended during a device change.
    pub fn try_core(&self) -> Option<MutexGuard<'_, Option<AudioCore>>> {
        self.0.try_lock().ok().filter(|g| g.is_some())
    }

    /// Render `out` like `AudioCore::render_stereo`, or silence when unavailable.
    pub fn render_stereo(&self, out: &mut [f32], channels: usize) {
        match self.try_core() {
            Some(mut g) => g.as_mut().expect("filtered").render_stereo(out, channels),
            None => out.fill(0.0),
        }
    }

    /// Take the core out (blocking; control side only).
    pub fn take(&self) -> Option<AudioCore> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

pub struct AudioOutput {
    stream: cpal::Stream,
    slot: CoreSlot,
    sample_rate: u32,
    channels: u16,
    device_lost: Arc<AtomicBool>,
}

impl AudioOutput {
    /// Open `device` (or the picked one) at `sample_rate` with a fixed `block` if the
    /// device allows it, and run `core` in its callback. The engine inside `core`
    /// must have been built for `sample_rate` (see [`negotiate`]).
    pub fn start(
        core: AudioCore,
        device: Option<usize>,
        sample_rate: u32,
        block: u32,
    ) -> Result<AudioOutput, String> {
        Self::start_or_return(core, device, sample_rate, block).map_err(|(e, _)| e)
    }

    /// `start`, but a device that does not open hands the core back with the error.
    // The core comes back by value on purpose (rare call: a device change).
    #[allow(clippy::result_large_err)]
    pub fn start_or_return(
        core: AudioCore,
        device: Option<usize>,
        sample_rate: u32,
        block: u32,
    ) -> Result<AudioOutput, (String, AudioCore)> {
        let slot = CoreSlot::new(core);
        Self::open(slot.clone(), device, sample_rate, block)
            .map_err(|e| (e, slot.take().expect("the core is still in the slot")))
    }

    fn open(
        slot: CoreSlot,
        device: Option<usize>,
        sample_rate: u32,
        block: u32,
    ) -> Result<AudioOutput, String> {
        let (info, d) = resolve(device)?;
        let default = d.default_output_config().map_err(|e| e.to_string())?;
        let channels = default.channels();
        let ranges: Vec<_> = d
            .supported_output_configs()
            .map(|it| it.collect())
            .unwrap_or_default();
        let fits = |r: &cpal::SupportedStreamConfigRange| {
            r.channels() == channels
                && r.min_sample_rate() <= sample_rate
                && sample_rate <= r.max_sample_rate()
        };
        let range = ranges
            .iter()
            .find(|r| fits(r) && r.sample_format() == SampleFormat::F32)
            .or_else(|| ranges.iter().find(|r| fits(r)));
        let format = range.map_or(default.sample_format(), |r| r.sample_format());
        let fixed = range.is_some_and(|r| match r.buffer_size() {
            SupportedBufferSize::Range { min, max } => *min <= block && block <= *max,
            SupportedBufferSize::Unknown => false,
        });
        let device_lost = Arc::new(AtomicBool::new(false));
        let telemetry = slot
            .try_core()
            .and_then(|g| g.as_ref().map(AudioCore::telemetry))
            .ok_or("audio core unavailable")?;
        // No retry with another buffer size: a failed build consumes the core.
        let buffer_size = if fixed {
            BufferSize::Fixed(block)
        } else {
            BufferSize::Default
        };
        let cfg = StreamConfig {
            channels,
            sample_rate,
            buffer_size,
        };
        let lost = Arc::clone(&device_lost);
        let stream = match format {
            SampleFormat::F32 => stream::<f32>(&d, cfg, slot.clone(), telemetry, lost),
            SampleFormat::I16 => stream::<i16>(&d, cfg, slot.clone(), telemetry, lost),
            SampleFormat::I32 => stream::<i32>(&d, cfg, slot.clone(), telemetry, lost),
            SampleFormat::U16 => stream::<u16>(&d, cfg, slot.clone(), telemetry, lost),
            other => Err(format!(
                "Unsupported sample format {other} on {}",
                info.name
            )),
        }?;
        stream
            .play()
            .map_err(|e| format!("Could not start audio on {}: {e}", info.name))?;
        Ok(AudioOutput {
            stream,
            slot,
            sample_rate,
            channels,
            device_lost,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The device reported that it went away.
    pub fn device_lost(&self) -> bool {
        self.device_lost.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        let _ = self.stream.pause();
    }

    /// Stop and drop the stream and return the core with all its state.
    pub fn close(self) -> AudioCore {
        let _ = self.stream.pause();
        drop(self.stream); // the callback no longer runs after this
        self.slot
            .take()
            .expect("the core lives in the slot until close")
    }
}

fn stream<T>(
    d: &cpal::Device,
    cfg: StreamConfig,
    slot: CoreSlot,
    tel: Arc<Telemetry>,
    lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(cfg.channels);
    let mut late = LateTracker::new(f64::from(cfg.sample_rate));
    let mut origin: Option<StreamInstant> = None;
    let mut scratch = vec![0.0f32; CONVERT * channels.max(1)];
    let tel_cb = Arc::clone(&tel);
    let data = move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
        let now = info.timestamp().callback;
        let base = *origin.get_or_insert(now);
        let frames = out.len() / channels.max(1);
        let ev = late.observe(now.saturating_duration_since(base).as_secs_f64(), frames);
        if ev.xrun {
            tel_cb.xruns.fetch_add(1, Ordering::Relaxed);
        }
        tel_cb
            .late_us_max
            .fetch_max((ev.late_ms.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
        match slot.try_core() {
            Some(mut g) => write(g.as_mut().expect("filtered"), out, channels, &mut scratch),
            None => out.fill(T::EQUILIBRIUM),
        }
    };
    let errors = move |e: cpal::Error| {
        match e.kind() {
            ErrorKind::Xrun => tel.xruns.fetch_add(1, Ordering::Relaxed),
            ErrorKind::DeviceNotAvailable => {
                lost.store(true, Ordering::Relaxed);
                tel.errors.fetch_add(1, Ordering::Relaxed)
            }
            _ => tel.errors.fetch_add(1, Ordering::Relaxed),
        };
    };
    d.build_output_stream::<T, _, _>(cfg, data, errors, None)
        .map_err(|e| format!("Could not open the audio device: {e}"))
}

/// Render into the device buffer through the fixed f32 scratch (one copy, no
/// allocation; for f32 devices `from_sample` is the identity). The scratch holds
/// a whole number of frames, so chunks never split a frame.
fn write<T: SizedSample + FromSample<f32>>(
    core: &mut AudioCore,
    out: &mut [T],
    channels: usize,
    scratch: &mut [f32],
) {
    for part in out.chunks_mut(scratch.len()) {
        let s = &mut scratch[..part.len()];
        core.render_stereo(s, channels);
        for (o, &v) in part.iter_mut().zip(s.iter()) {
            *o = T::from_sample(v);
        }
    }
}
