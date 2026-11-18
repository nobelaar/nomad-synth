//! The audio-thread side ([`AudioCore`], owner of the `Engine`) and the control
//! side ([`ControlHandle`]), joined by two lock-free SPSC queues: commands in,
//! garbage out. Everything the audio thread needs is allocated in [`create`].

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use nomad_core::engine::Engine;
use rtrb::{Consumer, Producer, PushError, RingBuffer};

use crate::command::{Command, Garbage};
use crate::telemetry::Telemetry;

const COMMANDS: usize = 4096;
const GARBAGE: usize = 256;
/// Garbage kept on the audio side while the garbage queue is full.
const PENDING: usize = 64;
/// Largest engine render per step; bigger callbacks are processed in batches.
const BATCH: usize = 4096;
/// Allowed division values, as `nomad/presets.py` (`_DIVISIONS`).
const DIVISIONS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

pub struct AudioCore {
    engine: Engine,
    commands: Consumer<Command>,
    garbage: Producer<Garbage>,
    pending: Vec<Garbage>,
    mono: Vec<f64>,
    mono32: Vec<f32>,
    telemetry: Arc<Telemetry>,
}

pub struct ControlHandle {
    commands: Producer<Command>,
    garbage: Consumer<Garbage>,
    telemetry: Arc<Telemetry>,
}

/// Split an engine into its audio-thread core and the control handle.
pub fn create(engine: Engine, sample_rate: f64) -> (AudioCore, ControlHandle) {
    let (cmd_tx, cmd_rx) = RingBuffer::new(COMMANDS);
    let (gar_tx, gar_rx) = RingBuffer::new(GARBAGE);
    let telemetry = Arc::new(Telemetry::new(sample_rate));
    let core = AudioCore {
        engine,
        commands: cmd_rx,
        garbage: gar_tx,
        pending: Vec::with_capacity(PENDING),
        mono: vec![0.0; BATCH],
        mono32: vec![0.0; BATCH],
        telemetry: Arc::clone(&telemetry),
    };
    let ctl = ControlHandle {
        commands: cmd_tx,
        garbage: gar_rx,
        telemetry,
    };
    (core, ctl)
}

impl ControlHandle {
    /// Queue a command; `false` when the queue is full (dropped and counted, never blocks).
    pub fn send(&mut self, cmd: Command) -> bool {
        match self.commands.push(cmd) {
            Ok(()) => true,
            Err(PushError::Full(_)) => {
                self.telemetry
                    .dropped_commands
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Free everything the audio thread handed back; returns how many objects.
    pub fn reap(&mut self) -> usize {
        self.reap_with(drop)
    }

    /// Like `reap`, but lets the caller look at each object before it is freed.
    pub fn reap_with(&mut self, mut f: impl FnMut(Garbage)) -> usize {
        let mut n = 0;
        while let Ok(g) = self.garbage.pop() {
            f(g);
            n += 1;
        }
        n
    }

    pub fn telemetry(&self) -> Arc<Telemetry> {
        Arc::clone(&self.telemetry)
    }
}

/// Hand `g` to the control side without ever freeing it here: if the queue is
/// full it waits in `pending`.
fn trash(queue: &mut Producer<Garbage>, pending: &mut Vec<Garbage>, g: Garbage) {
    if let Err(PushError::Full(g)) = queue.push(g) {
        if pending.len() < pending.capacity() {
            pending.push(g);
        } else {
            // shortcut: both garbage stores full (the control side stopped reaping):
            // leak rather than free on the audio thread; reaping restores the flow.
            std::mem::forget(g);
        }
    }
}

impl AudioCore {
    pub fn telemetry(&self) -> Arc<Telemetry> {
        Arc::clone(&self.telemetry)
    }

    /// Device callback: apply queued commands, render, write `out` interleaved
    /// (`channels` >= 1; mono on channels 0 and 1, the rest silent). A panic is
    /// contained: the callback outputs silence and counts an error.
    pub fn render_stereo(&mut self, out: &mut [f32], channels: usize) {
        let result = catch_unwind(AssertUnwindSafe(|| self.callback(out, channels)));
        if result.is_err() {
            out.fill(0.0);
            self.telemetry.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.telemetry.callbacks.fetch_add(1, Ordering::Relaxed);
    }

    fn callback(&mut self, out: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        // Retry garbage that did not fit last time.
        while let Some(g) = self.pending.pop() {
            if let Err(PushError::Full(g)) = self.garbage.push(g) {
                self.pending.push(g);
                break;
            }
        }
        while let Ok(cmd) = self.commands.pop() {
            self.apply(cmd);
        }
        let start = Instant::now();
        let frames = out.len() / channels;
        out[frames * channels..].fill(0.0);
        let mut f0 = 0;
        while f0 < frames {
            let n = (frames - f0).min(BATCH);
            self.engine.render(&mut self.mono[..n]);
            for (j, frame) in out[f0 * channels..(f0 + n) * channels]
                .chunks_exact_mut(channels)
                .enumerate()
            {
                let v = self.mono[j] as f32;
                self.mono32[j] = v;
                frame[0] = v;
                if channels > 1 {
                    frame[1] = v;
                    frame[2..].fill(0.0);
                }
            }
            self.telemetry.scope.write(&self.mono32[..n]);
            f0 += n;
        }
        self.telemetry
            .render_time(start.elapsed().as_nanos() as u64);
    }

    fn apply(&mut self, cmd: Command) {
        let e = &mut self.engine;
        let (garbage, pending) = (&mut self.garbage, &mut self.pending);
        match cmd {
            Command::NoteOn { note, velocity } => e.note_on(i64::from(note), f64::from(velocity)),
            Command::NoteOff { note } => e.note_off(i64::from(note)),
            Command::Panic => e.panic(),
            Command::SetParam { id, param, value } => {
                if let Some(s) = e.synth_mut(id) {
                    s.set_param(usize::from(param), value);
                }
            }
            Command::SetControl { ctl, value } => e.set_control(ctl, value),
            Command::SetMaster(v) => e.set_master_volume(v),
            Command::SetTargetSynth(id) => {
                e.set_target_synth(id);
            }
            Command::Start(id) => {
                e.start(id);
            }
            Command::Stop(id) => {
                e.stop(id);
            }
            Command::SetBpm { id, bpm } => {
                e.set_bpm(id, bpm);
            }
            Command::SetMute { id, muted } => e.set_mute(id, muted),
            Command::SetLevel { id, level } => e.set_level(id, level),
            Command::SetSource { id, src } => {
                e.set_source(id, src);
            }
            Command::SeqSetStep {
                id,
                step,
                on,
                note,
                vel,
            } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.set_step(
                        usize::from(step),
                        on,
                        note.map(i64::from),
                        vel.map(i64::from),
                    );
                }
            }
            Command::SeqAddNote { id, step, note } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.add_note(usize::from(step), i64::from(note));
                }
            }
            Command::SeqTranspose { id, step, delta } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.transpose(usize::from(step), i64::from(delta));
                }
            }
            Command::SeqClear(id) => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.clear();
                }
            }
            Command::SeqGate { id, gate } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.set_gate(gate);
                }
            }
            Command::SeqDivision { id, division } => {
                // Snapped to the allowed values, as presets do; non-finite is ignored.
                if let (Some(s), true) = (e.synth_mut(id), division.is_finite()) {
                    let d = DIVISIONS.iter().copied().fold(DIVISIONS[0], |best, d| {
                        if (d - division).abs() < (best - division).abs() {
                            d
                        } else {
                            best
                        }
                    });
                    s.sequencer.grid.division = d;
                }
            }
            Command::SeqLength { id, length } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.grid.length = usize::from(length.clamp(1, 16));
                }
            }
            Command::SeqSwing { id, swing } => {
                if let (Some(s), true) = (e.synth_mut(id), swing.is_finite()) {
                    s.sequencer.grid.swing = swing.clamp(0.0, 0.6);
                }
            }
            Command::SeqEnabled { id, enabled } => {
                if let Some(s) = e.synth_mut(id) {
                    s.sequencer.enabled = enabled;
                }
            }
            Command::SeqArm { id, on } => {
                if let Some(s) = e.synth_mut(id) {
                    s.arm_rec(on);
                }
            }
            Command::SeqRest(id) => {
                if let Some(clock) = e.effective_clock(id).cloned()
                    && let Some(s) = e.synth_mut(id)
                {
                    s.rest(&clock);
                }
            }
            Command::DrumSetStep {
                id,
                track,
                step,
                level,
            } => {
                if let Some(d) = e.drums_mut(id) {
                    d.set_step(usize::from(track), usize::from(step), level);
                }
            }
            Command::DrumClear(id) => {
                if let Some(d) = e.drums_mut(id) {
                    d.clear_pattern();
                }
            }
            Command::DrumTrigger {
                id,
                track,
                velocity,
            } => {
                if let Some(d) = e.drums_mut(id) {
                    d.trigger(usize::from(track), velocity);
                }
            }
            Command::DrumValue { id, param, value } => {
                let effect = e
                    .drums_mut(id)
                    .map(|d| d.set_value(usize::from(param), value));
                if let Some(nomad_core::drums::ValueEffect::Bpm(b)) = effect {
                    e.set_bpm(id, b);
                }
            }
            Command::DrumSetBank {
                id,
                track,
                mut shot,
            } => {
                // Reuse the command's box to send the unused buffer back: nothing is freed here.
                let new = std::mem::take(&mut *shot);
                *shot = match e.set_drum_bank(id, usize::from(track), new) {
                    Ok(old) | Err(old) => old,
                };
                trash(garbage, pending, Garbage::Shot(shot));
            }
            Command::AddInstrument(inst) => {
                if let Err(inst) = e.insert_instance(inst) {
                    trash(garbage, pending, Garbage::Instance(inst));
                }
            }
            Command::RemoveInstrument(id) => {
                e.remove_instrument(id);
            }
            Command::DropClosed => {
                e.drop_closed(&mut |inst| trash(garbage, pending, Garbage::Instance(inst)))
            }
            Command::TestPanic => panic!("test panic inside the audio callback"),
        }
    }
}
