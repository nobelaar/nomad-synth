//! The audio-thread API must not allocate: a counting global allocator that only
//! counts while a thread-local flag is on, plus a backtrace of the first hit.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;

use nomad_core::clock::Clock;
use nomad_core::engine::{Engine, InstrumentId, Kind, MAX_INSTANCES, build_instance};
use nomad_core::params::{DRUM_PARAMS, pid};
use nomad_core::synth::Control;

struct Counting;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}
static FIRST: Mutex<Option<String>> = Mutex::new(None);

fn hit() {
    if ON.with(|f| f.get()) {
        ON.with(|f| f.set(false)); // nested allocations below are not counted
        COUNT.with(|c| c.set(c.get() + 1));
        let mut first = FIRST.lock().unwrap();
        if first.is_none() {
            *first = Some(std::backtrace::Backtrace::force_capture().to_string());
        }
        drop(first);
        ON.with(|f| f.set(true));
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

/// Run `f` and fail with the first allocation's backtrace if it allocated or freed.
fn assert_no_alloc<R>(what: &str, f: impl FnOnce() -> R) -> R {
    *FIRST.lock().unwrap() = None;
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    let r = f();
    ON.with(|o| o.set(false));
    let n = COUNT.with(|c| c.get());
    if n > 0 {
        let bt = FIRST.lock().unwrap().take().unwrap_or_default();
        panic!("{what}: {n} allocator calls; first at:\n{bt}");
    }
    r
}

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

fn busy_engine() -> Engine {
    let mut e = Engine::new(SR, 8);
    let sq = &mut e.synth_mut(SYNTH).unwrap().sequencer;
    sq.enabled = true;
    for s in 0..16 {
        sq.set_step(s, true, Some(48 + s as i64), None);
        sq.add_note(s, 55 + s as i64);
    }
    for n in [48, 52, 55, 59, 62, 65, 69, 72] {
        e.note_on(n, 0.8);
    }
    e.start(SYNTH);
    e.start(DRUMS);
    e
}

#[test]
fn render_is_allocation_free() {
    for block in [37, 256, 512] {
        let mut e = busy_engine();
        let mut out = vec![0.0; block];
        let blocks = 2 * 48000 / block;
        assert_no_alloc(&format!("render {block}"), || {
            for _ in 0..blocks {
                e.render(&mut out);
            }
        });
        assert!(out.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn commands_are_allocation_free() {
    let mut e = busy_engine();
    let swing = DRUM_PARAMS.iter().position(|s| s.name == "swing").unwrap();
    let mut out = vec![0.0; 256];
    assert_no_alloc("commands", || {
        for n in 0..128 {
            e.note_on(n, 0.5);
            e.note_off(n);
        }
        for id in 0..pid::COUNT {
            e.set_param(id, 0.37);
        }
        let s = e.synth_mut(SYNTH).unwrap();
        s.set_control(Control::Bend, 0.5);
        s.set_control(Control::ModWheel, 1.0);
        let sq = &mut s.sequencer;
        sq.set_step(3, true, Some(64), Some(90));
        sq.add_note(3, 67);
        sq.transpose(3, 2);
        sq.set_gate(0.7);
        sq.arm(true);
        sq.record(60, 100, &Clock::new(SR));
        sq.record_off(&Clock::new(SR));
        sq.arm(false);
        sq.clear();
        e.stop(SYNTH);
        e.start(SYNTH);
        e.set_bpm(DRUMS, 133.0);
        e.set_mute(SYNTH, true);
        e.set_mute(SYNTH, false);
        e.set_level(DRUMS, 0.4);
        e.set_master_volume(0.5);
        e.set_source(SYNTH, Some(DRUMS));
        e.render(&mut out);
        e.set_source(SYNTH, None);
        let d = e.drums_mut(DRUMS).unwrap();
        d.set_step(2, 5, 2);
        d.trigger(3, 0.9);
        e.set_drum_value(DRUMS, swing, 0.3);
        e.set_target_synth(SYNTH);
        e.render(&mut out);
    });
}

#[test]
fn insert_and_close_are_allocation_free() {
    let mut e = busy_engine();
    let (id, name) = e.next_id(Kind::Drums);
    let boxed = build_instance(SR, 8, Kind::Drums, id, name);
    let mut graveyard = Vec::with_capacity(MAX_INSTANCES);
    let mut out = vec![0.0; 256];
    assert_no_alloc("insert/remove/drop", || {
        assert!(e.insert_instance(boxed).is_ok());
        e.start(id);
        e.render(&mut out);
        assert!(e.remove_instrument(id));
        assert!(e.remove_instrument(SYNTH));
        e.render(&mut out);
        e.drop_closed(&mut |inst| graveyard.push(inst));
        e.render(&mut out);
    });
    assert_eq!(graveyard.len(), 2);
}

#[test]
fn set_bank_hands_back_old_shot() {
    let mut e = busy_engine();
    let mut out = vec![0.0; 256];
    e.render(&mut out);
    // Tom (track 3) is not in the default pattern, so nothing plays it: the old shot comes back.
    let shot = vec![0.25_f32; 1000];
    let old = assert_no_alloc("set_bank idle", || e.set_drum_bank(DRUMS, 3, shot)).unwrap();
    assert!(old.len() > 1000);
    // The kick is sounding: its voice keeps the old shot, the unused slot comes back.
    let shot = vec![0.5_f32; 10];
    let spare = assert_no_alloc("set_bank playing", || e.set_drum_bank(DRUMS, 0, shot)).unwrap();
    assert!(spare.is_empty());
    e.render(&mut out);
    assert!(e.set_drum_bank(99, 0, vec![1.0]).is_none());
}

#[test]
fn insert_rejects_when_full() {
    let mut e = Engine::new(SR, 8);
    while e.instruments().len() < MAX_INSTANCES {
        let (id, name) = e.next_id(Kind::Synth);
        assert!(
            e.insert_instance(build_instance(SR, 1, Kind::Synth, id, name))
                .is_ok()
        );
    }
    let (id, name) = e.next_id(Kind::Synth);
    let back = e.insert_instance(build_instance(SR, 1, Kind::Synth, id, name));
    assert!(back.is_err());
}
