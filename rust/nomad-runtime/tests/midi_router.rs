use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use nomad_core::synth::Control;
use nomad_runtime::midi::parse::{MidiEvent, MidiKind};
use nomad_runtime::midi::router::{MidiRouter, RouterSink};

struct Counting;
thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
fn hit() {
    if ON.with(|f| f.get()) {
        COUNT.with(|c| c.set(c.get() + 1));
    }
}
// SAFETY: forwards every call to the system allocator unchanged; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        hit();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        hit();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        hit();
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

#[derive(Default)]
struct Recording(Vec<serde_json::Value>);

impl RouterSink for Recording {
    fn note_on(&mut self, note: u8, velocity: f64) {
        self.0.push(serde_json::json!(["note_on", note, velocity]));
    }
    fn note_off(&mut self, note: u8) {
        self.0.push(serde_json::json!(["note_off", note]));
    }
    fn set_control(&mut self, ctl: Control, value: f64) {
        let name = if ctl == Control::Bend {
            "bend"
        } else {
            "modwheel"
        };
        self.0.push(serde_json::json!(["control", name, value]));
    }
    fn panic(&mut self) {
        self.0.push(serde_json::json!(["panic"]));
    }
    fn other_cc(&mut self, cc: u8, value: u8, channel: u8) {
        self.0.push(serde_json::json!(["cc", cc, value, channel]));
    }
}

/// Discards everything (for the allocation test).
struct Null;
impl RouterSink for Null {
    fn note_on(&mut self, _: u8, _: f64) {}
    fn note_off(&mut self, _: u8) {}
    fn set_control(&mut self, _: Control, _: f64) {}
    fn panic(&mut self) {}
    fn other_cc(&mut self, _: u8, _: u8, _: u8) {}
}

fn event(v: &serde_json::Value) -> MidiEvent {
    let u = |k: &str| v[k].as_u64().unwrap() as u8;
    MidiEvent {
        kind: match v["kind"].as_str().unwrap() {
            "note_on" => MidiKind::NoteOn,
            "note_off" => MidiKind::NoteOff,
            "cc" => MidiKind::Cc,
            _ => MidiKind::Bend,
        },
        note: u("note"),
        velocity: v["velocity"].as_f64().unwrap(),
        cc: u("cc"),
        value: u("value"),
        channel: u("channel"),
        bend: v["bend"].as_f64().unwrap(),
    }
}

#[test]
fn traces_match_python() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/midi_router.json");
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let data = data.as_object().unwrap();
    assert_eq!(data.len(), 10);
    for (label, sc) in data {
        let (mut r, mut sink) = (MidiRouter::new(), Recording::default());
        for e in sc["events"].as_array().unwrap() {
            if e == "reset" {
                r.reset(&mut sink);
            } else {
                r.handle(&event(e), &mut sink);
            }
        }
        // Floats are velocity/127 and control values: the same arithmetic, so exact.
        assert_eq!(&serde_json::Value::Array(sink.0), &sc["trace"], "{label}");
    }
}

#[test]
fn no_allocation_per_event() {
    let mut events = Vec::with_capacity(10000);
    for k in 0..10000u32 {
        let n = (k * 7 % 128) as u8;
        let mut e = MidiEvent {
            kind: MidiKind::NoteOn,
            note: n,
            velocity: 0.5,
            cc: 0,
            value: 0,
            channel: 0,
            bend: 0.0,
        };
        match k % 6 {
            0 => {}
            1 => e.kind = MidiKind::NoteOff,
            2 => (e.kind, e.cc, e.value) = (MidiKind::Cc, 64, (k % 128) as u8),
            3 => (e.kind, e.cc, e.value) = (MidiKind::Cc, [1, 120, 123, 21][(k % 4) as usize], 64),
            4 => (e.kind, e.bend) = (MidiKind::Bend, 0.3),
            _ => e.kind = MidiKind::NoteOff,
        }
        events.push(e);
    }
    let mut r = MidiRouter::new();
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    for e in &events {
        r.handle(e, &mut Null);
    }
    r.reset(&mut Null);
    ON.with(|o| o.set(false));
    assert_eq!(COUNT.with(|c| c.get()), 0);
}

#[test]
fn reset_is_idempotent() {
    let (mut r, mut sink) = (MidiRouter::new(), Recording::default());
    let on = MidiEvent {
        kind: MidiKind::NoteOn,
        note: 60,
        velocity: 1.0,
        cc: 0,
        value: 0,
        channel: 0,
        bend: 0.0,
    };
    r.handle(&on, &mut sink);
    r.reset(&mut sink);
    let after_first = sink.0.len();
    r.reset(&mut sink);
    assert_eq!(sink.0.len(), after_first);
}
