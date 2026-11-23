//! Spiral tab: one turn per octave, every note at its own angle (C on top). Two
//! layers over the same spiral: what the listened synth sounds like (spectrum of
//! its own output, tapped from the engine) and what its voices play (exact
//! frequencies from the mirror, theoretical harmonics shaped by wave and filter).

use egui::{Align2, Color32, FontId, Pos2, Sense, Stroke, Vec2};
use nomad_core::engine::{InstrumentId, Kind};
use nomad_core::mirror::{VOICE_SLOTS, VoiceView};
use nomad_core::oscillator::Wave;
use nomad_core::params::pid;
use nomad_runtime::{Command, ScopeRing};

use crate::help::{reg, reg_rect};
use crate::model::{AppModel, InstrumentView};
use crate::theme::{ACCENT, BG, GREEN, GREY, TROUGH};

/// Lowest and highest MIDI notes on the spiral: C1..C9, eight turns.
pub const LOW: f64 = 24.0;
pub const HIGH: f64 = 120.0;
/// Spectrum samples along the spiral per semitone.
const STEPS_PER_SEMI: usize = 6;
/// Spectrum floor and ceiling in dBFS (mapped to 0..1).
const DB_FLOOR: f32 = -45.0;
const DB_TOP: f32 = -6.0;
/// Per-frame decay of the drawn spectrum (peaks linger a little).
const DECAY: f32 = 0.85;
const MAX_HARMONIC: usize = 32;
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn midi_of(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

pub fn hz_of(midi: f64) -> f64 {
    440.0 * ((midi - 69.0) / 12.0).exp2()
}

/// Point of MIDI note `m` (fractional) on a spiral centred at `c`, from radius
/// `r_in` at `LOW` to `r_out` at `HIGH`, C straight up, clockwise.
pub fn spiral_pos(m: f64, c: Pos2, r_in: f32, r_out: f32) -> Pos2 {
    let t = ((m - LOW) / (HIGH - LOW)) as f32;
    let r = r_in + t * (r_out - r_in);
    let a = std::f64::consts::TAU * m.rem_euclid(12.0) / 12.0;
    c + Vec2::new(a.sin() as f32, -a.cos() as f32) * r
}

/// Relative amplitude of harmonic `n` (1 = fundamental) of an ideal wave.
// shortcut: Square assumes a 50% pulse (odd harmonics only), ignoring PW and sync.
pub fn harmonic_weight(wave: Wave, n: usize) -> f64 {
    let k = n as f64;
    match wave {
        Wave::Sine => f64::from(u8::from(n == 1)),
        Wave::Saw => 1.0 / k,
        Wave::Square if n % 2 == 1 => 1.0 / k,
        Wave::Triangle if n % 2 == 1 => 1.0 / (k * k),
        _ => 0.0,
    }
}

/// Gain of the 4-pole low-pass at `hz` for cutoff `fc` (24 dB/oct, no resonance).
pub fn lowpass_gain(hz: f64, fc: f64) -> f64 {
    if fc <= 0.0 {
        return 1.0;
    }
    1.0 / (1.0 + (hz / fc).powi(8)).sqrt()
}

/// `"A4 +12¢"`: the nearest note and how far from it, in cents.
pub fn note_label(hz: f64) -> String {
    let m = midi_of(hz);
    let n = m.round();
    let cents = ((m - n) * 100.0).round() as i32;
    let name = format!(
        "{}{}",
        NAMES[(n as i64).rem_euclid(12) as usize],
        (n as i64).div_euclid(12) - 1
    );
    if cents == 0 {
        name
    } else {
        format!("{name} {cents:+}¢")
    }
}

/// In-place radix-2 FFT (`re.len()` a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Analysis state of the open spiral tab.
pub struct SpiralState {
    /// Synth listened to (`None` = pick the MIDI target or the first synth).
    pub source: Option<InstrumentId>,
    /// What the engine was last told to tap.
    pub tapped: Option<InstrumentId>,
    n: usize,
    samples: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    /// Amplitude (linear, sine peak = 1) per FFT bin.
    mags: Vec<f32>,
    /// Drawn level 0..1 per spiral step.
    level: Vec<f32>,
}

impl SpiralState {
    /// FFT size: the power of two nearest 170 ms (8192 at 48 kHz).
    pub fn new(sample_rate: f64) -> SpiralState {
        let n = ((sample_rate * 0.17) as usize)
            .next_power_of_two()
            .clamp(1024, 32768);
        let steps = (HIGH - LOW) as usize * STEPS_PER_SEMI + 1;
        SpiralState {
            source: None,
            tapped: None,
            n,
            samples: vec![0.0; n],
            re: vec![0.0; n],
            im: vec![0.0; n],
            mags: vec![0.0; n / 2],
            level: vec![0.0; steps],
        }
    }

    /// Analyse the latest `n` samples of `ring` and update the drawn levels.
    pub fn analyse(&mut self, ring: &ScopeRing, sample_rate: f64) {
        let n = self.n;
        let got = ring.window(n, &mut self.samples);
        self.samples[got..].fill(0.0);
        for (i, ((r, im), &x)) in self
            .re
            .iter_mut()
            .zip(self.im.iter_mut())
            .zip(&self.samples)
            .enumerate()
        {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            *r = x * w;
            *im = 0.0;
        }
        fft(&mut self.re, &mut self.im);
        // Hann: a sine of amplitude A peaks at A * n / 4.
        let scale = 4.0 / n as f32;
        for (k, m) in self.mags.iter_mut().enumerate() {
            *m = self.re[k].hypot(self.im[k]) * scale;
        }
        let bin = |m: f64| (hz_of(m) * n as f64 / sample_rate) as f32;
        let h = 0.5 / STEPS_PER_SEMI as f64;
        let last = self.mags.len() - 1;
        for (i, lv) in self.level.iter_mut().enumerate() {
            let m = LOW + i as f64 / STEPS_PER_SEMI as f64;
            let (lo, hi) = (bin(m - h), bin(m + h));
            let amp = if hi - lo >= 1.0 {
                // Several bins per step (high notes): the loudest one.
                let (a, b) = (
                    (lo.round() as usize).min(last),
                    (hi.round() as usize).min(last),
                );
                self.mags[a..=b].iter().copied().fold(0.0, f32::max)
            } else {
                // Less than a bin (low notes): linear interpolation.
                let x = bin(m).min(last as f32);
                let k = (x as usize).min(last - 1);
                let f = x - k as f32;
                self.mags[k] * (1.0 - f) + self.mags[k + 1] * f
            };
            let db = 20.0 * amp.max(1e-9).log10();
            let v = ((db - DB_FLOOR) / (DB_TOP - DB_FLOOR)).clamp(0.0, 1.0);
            *lv = v.max(*lv * DECAY);
        }
    }

    /// Drawn level per spiral step (tests).
    pub fn levels(&self) -> &[f32] {
        &self.level
    }
}

/// The synth to listen to: the chosen one if it still exists, else the MIDI
/// target, else the first synth.
pub fn resolve_source(state: &SpiralState, model: &AppModel) -> Option<InstrumentId> {
    let synth = |id: InstrumentId| model.view(id).is_some_and(|v| v.kind == Kind::Synth);
    state
        .source
        .filter(|&id| synth(id))
        .or(model.target_synth().filter(|&id| synth(id)))
        .or_else(|| {
            model
                .instruments()
                .iter()
                .find(|v| v.kind == Kind::Synth)
                .map(|v| v.id)
        })
}

/// Point the engine's tap at `want` (one command per change).
pub fn sync_tap(state: &mut SpiralState, model: &mut AppModel, want: Option<InstrumentId>) {
    if state.tapped != want && model.send(Command::SetTap(want)) {
        state.tapped = want;
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// The tab: source picker, the spiral and its legend.
pub fn spiral_ui(
    ui: &mut egui::Ui,
    state: &mut SpiralState,
    model: &mut AppModel,
    ring: &ScopeRing,
    sample_rate: f64,
) {
    let source = resolve_source(state, model);
    sync_tap(state, model, source);
    let synths: Vec<(InstrumentId, String)> = model
        .instruments()
        .iter()
        .filter(|v| v.kind == Kind::Synth)
        .map(|v| (v.id, v.name.clone()))
        .collect();
    let view: Option<InstrumentView> = source.and_then(|id| model.view(id)).cloned();
    ui.horizontal(|ui| {
        ui.label("ESCUCHANDO");
        let name = view.as_ref().map_or("(sin synth)", |v| v.name.as_str());
        let r = egui::ComboBox::from_id_salt("spiral-source")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (id, n) in &synths {
                    if ui.selectable_label(source == Some(*id), n).clicked() {
                        state.source = Some(*id);
                        model.set_target(*id);
                    }
                }
            });
        reg(r.response, "spiral_source");
    });
    state.analyse(ring, sample_rate);

    let side = ui
        .available_width()
        .min(ui.available_height() - 40.0)
        .max(200.0);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), side), Sense::hover());
    reg_rect(ui.ctx(), resp.rect, "spiral_view");
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r_out = side * 0.5 - 24.0;
    let r_in = r_out * 0.12;
    let at = |m: f64| spiral_pos(m, c, r_in, r_out);

    let voices: [VoiceView; VOICE_SLOTS] = view
        .as_ref()
        .map_or([VoiceView::default(); VOICE_SLOTS], |v| {
            v.shared.voice_views()
        });
    let live: Vec<VoiceView> = voices.into_iter().filter(|v| v.amp > 1e-4).collect();
    let param = |id: usize| view.as_ref().map_or(0.0, |v| v.shared.param(id));
    // Filter mark: the loudest voice's cutoff (envelope and LFO included), else the knob.
    let cutoff = live
        .iter()
        .max_by(|a, b| a.amp.total_cmp(&b.amp))
        .map_or(param(pid::FILT_CUTOFF), |v| v.cutoff);
    let cut_m = midi_of(cutoff.max(1.0));

    // Guide: 12 spokes with note names, the spiral track dimmer above the cutoff.
    for (k, name) in NAMES.iter().enumerate() {
        let col = if k == 0 { GREY } else { TROUGH };
        p.line_segment(
            [at(LOW + k as f64), at(HIGH - 12.0 + k as f64)],
            Stroke::new(1.0, col),
        );
        let a = std::f32::consts::TAU * k as f32 / 12.0;
        let lp = c + Vec2::new(a.sin(), -a.cos()) * (r_out + 14.0);
        p.text(
            lp,
            Align2::CENTER_CENTER,
            *name,
            FontId::proportional(13.0),
            GREY,
        );
    }
    for o in 1..=8 {
        let m = LOW + 12.0 * (o - 1) as f64;
        p.text(
            at(m) + Vec2::new(6.0, 0.0),
            Align2::LEFT_CENTER,
            format!("C{o}"),
            FontId::proportional(10.0),
            GREY,
        );
    }
    let steps = state.level.len();
    let step_m = |i: usize| LOW + i as f64 / STEPS_PER_SEMI as f64;
    for i in 0..steps - 1 {
        let (m0, m1) = (step_m(i), step_m(i + 1));
        let col = if m0 > cut_m { BG } else { TROUGH };
        p.line_segment([at(m0), at(m1)], Stroke::new(1.0, mix(col, TROUGH, 0.5)));
    }
    // Sound layer: brightness and width follow the spectrum.
    for i in 0..steps - 1 {
        let v = state.level[i].max(state.level[i + 1]);
        if v < 0.02 {
            continue;
        }
        let stroke = Stroke::new(
            1.0 + 7.0 * v,
            mix(TROUGH, GREEN, v).gamma_multiply(0.4 + 0.6 * v),
        );
        p.line_segment([at(step_m(i)), at(step_m(i + 1))], stroke);
    }
    // Filter cutoff: a radial tick across the track.
    if (LOW..=HIGH).contains(&cut_m) {
        let q = at(cut_m);
        let dir = (q - c).normalized();
        let gap = (r_out - r_in) / 8.0 * 0.45;
        p.line_segment([q - dir * gap, q + dir * gap], Stroke::new(2.0, ACCENT));
        p.text(
            q + dir * (gap + 8.0),
            Align2::CENTER_CENTER,
            "filtro",
            FontId::proportional(11.0),
            ACCENT,
        );
    }
    // Played layer: fundamentals and their theoretical harmonics.
    let osc = [
        (param(pid::OSC1_LEVEL), param(pid::OSC1_WAVE)),
        (param(pid::OSC2_LEVEL), param(pid::OSC2_WAVE)),
    ];
    let mut labels: Vec<f64> = Vec::new();
    for v in &live {
        for (k, &(lvl, w)) in osc.iter().enumerate() {
            let f0 = v.hz[k];
            if lvl < 1e-3 || f0.is_nan() || f0 <= 0.0 {
                continue;
            }
            let wave = Wave::from_index(w as i64).unwrap_or(Wave::Saw);
            for n in 2..=MAX_HARMONIC {
                let m = midi_of(f0 * n as f64);
                if m > HIGH {
                    break;
                }
                let g = (harmonic_weight(wave, n) * lowpass_gain(f0 * n as f64, v.cutoff)) as f32;
                if g < 0.02 || m < LOW {
                    continue;
                }
                let col = Color32::WHITE.gamma_multiply(0.25 + 0.75 * g);
                p.circle_stroke(at(m), 3.0 + 5.0 * g, Stroke::new(1.0, col));
            }
            let m = midi_of(f0);
            if (LOW..=HIGH).contains(&m) {
                p.circle_filled(at(m), 6.0, Color32::WHITE);
                if !labels.iter().any(|&x| (x - m).abs() < 0.5) {
                    labels.push(m);
                    p.text(
                        at(m) + Vec2::new(9.0, -9.0),
                        Align2::LEFT_BOTTOM,
                        note_label(f0),
                        FontId::proportional(13.0),
                        Color32::WHITE,
                    );
                }
            }
        }
    }
    let legend = "Verde = lo que suena · punto = nota tocada (Hz reales) · \
                  anillos = sus armónicos (onda y filtro) · marca = corte del filtro";
    let lr = ui.label(egui::RichText::new(legend).small().color(GREY));
    reg_rect(ui.ctx(), lr.rect, "spiral_view");
}
