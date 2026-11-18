//! Real MIDI input with `midir`: the callback runs on midir's thread (not the
//! audio thread), parses, routes notes and controls through [`MidiRouter`],
//! looks other CCs up in the shared [`MidiMap`] and pushes `Command`s on the
//! MIDI queue towards the audio thread.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};

use nomad_core::params::pid;
use nomad_core::synth::Control;

use super::map::MidiMap;
use super::parse::parse_message;
use super::router::{MidiRouter, RouterSink};
use crate::command::Command;
use crate::core::MidiSender;
use crate::telemetry::Telemetry;

/// Run `f`, counting a panic in `midi_errors` instead of unwinding into midir.
pub fn contain(tel: &Telemetry, f: impl FnOnce()) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        tel.midi_errors.fetch_add(1, Relaxed);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding the lock was already contained and counted.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Everything the callback needs: router state, the sender and the map.
pub struct MidiHandler {
    router: MidiRouter,
    sender: MidiSender,
    map: Arc<Mutex<MidiMap>>,
    port: String,
    monitor: Option<Sender<String>>,
}

/// `RouterSink` that turns router decisions into commands for the audio thread.
struct Sink<'a> {
    sender: &'a mut MidiSender,
    map: &'a Mutex<MidiMap>,
    port: &'a str,
    monitor: &'a Option<Sender<String>>,
}

impl Sink<'_> {
    fn send(&mut self, cmd: Command, what: impl FnOnce() -> String) {
        let ok = self.sender.send(cmd);
        if let Some(m) = self.monitor {
            let _ = m.send(format!(
                "{}{}",
                what(),
                if ok { "" } else { "  (queue full: dropped)" }
            ));
        }
    }
}

impl RouterSink for Sink<'_> {
    fn note_on(&mut self, note: u8, velocity: f64) {
        self.send(
            Command::NoteOn {
                note,
                velocity: velocity as f32,
            },
            || format!("note on  {note:>3} vel {velocity:.3}"),
        );
    }
    fn note_off(&mut self, note: u8) {
        self.send(Command::NoteOff { note }, || format!("note off {note:>3}"));
    }
    fn set_control(&mut self, ctl: Control, value: f64) {
        self.send(Command::SetControl { ctl, value }, || {
            format!("{ctl:?} {value:+.3}")
        });
    }
    fn panic(&mut self) {
        self.send(Command::Panic, || "panic (CC 120)".into());
    }
    fn other_cc(&mut self, cc: u8, value: u8, channel: u8) {
        let e = super::parse::MidiEvent {
            kind: super::parse::MidiKind::Cc,
            note: 0,
            velocity: 0.0,
            cc,
            value,
            channel,
            bend: 0.0,
        };
        let hit = lock(self.map).handle(self.port, &e);
        match hit {
            Some((p, v)) if p == pid::MASTER_VOLUME => {
                self.send(Command::SetMaster(v), || {
                    format!("cc {cc:>3} = {value:>3} ch {channel} -> master_volume {v:.4}")
                });
            }
            Some((p, v)) => {
                let id = self.sender.telemetry().target_synth.load(Relaxed);
                let name = nomad_core::params::SYNTH_PARAMS[p].name;
                self.send(
                    Command::SetParam {
                        id,
                        param: p as u16,
                        value: v,
                    },
                    || format!("cc {cc:>3} = {value:>3} ch {channel} -> synth {id} {name} {v:.4}"),
                );
            }
            None => {
                if let Some(m) = self.monitor {
                    let _ = m.send(format!("cc {cc:>3} = {value:>3} ch {channel} (unmapped)"));
                }
            }
        }
    }
}

impl MidiHandler {
    pub fn new(sender: MidiSender, map: Arc<Mutex<MidiMap>>, port: String) -> MidiHandler {
        MidiHandler {
            router: MidiRouter::new(),
            sender,
            map,
            port,
            monitor: None,
        }
    }

    /// One raw MIDI message.
    pub fn feed(&mut self, bytes: &[u8]) {
        let Some(e) = parse_message(bytes) else {
            return;
        };
        self.sender.telemetry().midi_events.fetch_add(1, Relaxed);
        let mut sink = Sink {
            sender: &mut self.sender,
            map: &self.map,
            port: &self.port,
            monitor: &self.monitor,
        };
        self.router.handle(&e, &mut sink);
    }

    /// Release every held key and the pedal.
    pub fn reset(&mut self) {
        let mut sink = Sink {
            sender: &mut self.sender,
            map: &self.map,
            port: &self.port,
            monitor: &self.monitor,
        };
        self.router.reset(&mut sink);
    }
}

/// Input port names (empty if ALSA/the backend is unavailable; never panics).
pub fn list_input_ports() -> Vec<String> {
    let Ok(mi) = midir::MidiInput::new("NOMAD list") else {
        return Vec::new();
    };
    mi.ports()
        .iter()
        .filter_map(|p| mi.port_name(p).ok())
        .collect()
}

pub struct MidiInput {
    _conn: midir::MidiInputConnection<()>,
    handler: Arc<Mutex<MidiHandler>>,
    port_name: String,
    port_id: String,
}

impl MidiInput {
    /// Open the first port whose name contains `port` (or, with `None`, the first
    /// that is not "Midi Through") and start feeding the handler.
    pub fn open(
        port: Option<&str>,
        sender: MidiSender,
        map: Arc<Mutex<MidiMap>>,
    ) -> Result<MidiInput, String> {
        let mut mi =
            midir::MidiInput::new("NOMAD").map_err(|e| format!("MIDI unavailable: {e}"))?;
        mi.ignore(midir::Ignore::All);
        let ports = mi.ports();
        let names: Vec<String> = ports
            .iter()
            .map(|p| mi.port_name(p).unwrap_or_default())
            .collect();
        let pick = match port {
            Some(text) => names.iter().position(|n| n.contains(text)),
            None => names.iter().position(|n| !n.contains("Midi Through")),
        };
        let Some(i) = pick else {
            return Err(format!("no matching MIDI input port; available: {names:?}"));
        };
        let port_name = names[i].clone();
        let port_id = ports[i].id();
        let tel = Arc::clone(sender.telemetry());
        let handler = Arc::new(Mutex::new(MidiHandler::new(sender, map, port_name.clone())));
        let h = Arc::clone(&handler);
        let conn = mi
            .connect(
                &ports[i],
                "nomad-in",
                move |_stamp, bytes, _| contain(&tel, || lock(&h).feed(bytes)),
                (),
            )
            .map_err(|e| format!("cannot open MIDI port {port_name:?}: {e}"))?;
        Ok(MidiInput {
            _conn: conn,
            handler,
            port_name,
            port_id,
        })
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    /// Release held keys and the pedal (call when the port changes or goes away).
    pub fn reset(&self) {
        lock(&self.handler).reset();
    }

    /// Whether the port still exists (midir has no disconnect callback, so this
    /// asks the backend; call it every second or so, not per event).
    pub fn is_alive(&self) -> bool {
        midir::MidiInput::new("NOMAD check")
            .is_ok_and(|mi| mi.find_port_by_id(&self.port_id).is_some())
    }

    /// Text lines describing each event and what it became (for `nomad midi`).
    pub fn monitor(&self) -> Receiver<String> {
        let (tx, rx) = channel();
        lock(&self.handler).monitor = Some(tx);
        rx
    }
}
