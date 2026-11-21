//! Presets against `nomad/presets.py` (reference from `tools/ref_presets.py`).

use std::path::{Path, PathBuf};

use nomad_app::model::{AppModel, InstrumentView, initial_views};
use nomad_app::presets::{
    Origin, PresetKind, PresetStore, apply_commands, capture, check, dumps, factory, slug,
};
use nomad_core::drum_synth::render_one_shot;
use nomad_core::engine::{Engine, InstrumentId};
use nomad_runtime::{AudioCore, Command, create};
use serde_json::Value;

const SR: f64 = 48000.0;
const SYNTH: InstrumentId = 1;
const DRUMS: InstrumentId = 2;

fn reference() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/presets.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn files() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference/preset_files")
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nomad-presets-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn kind(s: &str) -> PresetKind {
    match s {
        "synth" => PresetKind::Synth,
        _ => PresetKind::Drums,
    }
}

/// Same structure and key order; numbers compared as values (Python mixes 2 and 2.0).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((k1, v1), (k2, v2))| k1 == k2 && same(v1, v2))
        }
        _ => a == b,
    }
}

struct Rig {
    model: AppModel,
    core: AudioCore,
}

impl Rig {
    fn new() -> Rig {
        let e = Engine::new(SR, 8);
        let views = initial_views(&e);
        let (core, ctl) = create(e, SR);
        let mut r = Rig {
            model: AppModel::new(ctl, SR, views),
            core,
        };
        r.render();
        r
    }
    fn view(&self, id: InstrumentId) -> InstrumentView {
        self.model.view(id).unwrap().clone()
    }
    fn render(&mut self) {
        let mut out = vec![0.0f32; 512];
        self.core.render_stereo(&mut out, 1);
    }
    fn apply(&mut self, id: InstrumentId, preset: &Value) {
        let inst = self.view(id);
        for c in apply_commands(&inst, preset, SR).unwrap() {
            assert!(self.model.send(c));
        }
        self.render();
    }
}

#[test]
fn factory_matches_python() {
    let r = reference();
    for k in ["synth", "drums"] {
        let want = r["factory"][k].as_array().unwrap();
        let got = factory(kind(k));
        assert_eq!(
            got.len(),
            want.len() + 1,
            "{k}: one Rust-only \"Dark techno\" at the end"
        );
        for (g, w) in got.iter().zip(want) {
            assert!(same(g, w), "{k} {}", w["name"]);
        }
    }
}

#[test]
fn check_matches_python() {
    for c in reference()["checks"].as_array().unwrap() {
        let got = check(kind(c["kind"].as_str().unwrap()), &c["data"]).err();
        assert_eq!(got.as_deref(), c["error"].as_str(), "{}", c["label"]);
    }
}

#[test]
fn slug_matches_python() {
    for p in reference()["slugs"].as_array().unwrap() {
        assert_eq!(slug(p[0].as_str().unwrap()), p[1].as_str().unwrap());
    }
}

#[test]
fn floats_are_written_like_python() {
    for p in reference()["floats"].as_array().unwrap() {
        let bits = u64::from_str_radix(p[0].as_str().unwrap(), 16).unwrap();
        let v = Value::from(f64::from_bits(bits));
        assert_eq!(dumps(&v), p[1].as_str().unwrap());
    }
    assert_eq!(
        dumps(&serde_json::json!({"a": [], "b": {}, "c": "é\"\n"})),
        "{\n \"a\": [],\n \"b\": {},\n \"c\": \"\\u00e9\\\"\\n\"\n}"
    );
}

#[test]
fn store_lists_factory_then_valid_user_files() {
    let store = PresetStore::new(Some(files()));
    let r = reference();
    for k in ["synth", "drums"] {
        let want: Vec<(String, Origin)> = r["listed"][k]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let o = if e[1] == "factory" {
                    Origin::Factory
                } else {
                    Origin::User
                };
                (e[0].as_str().unwrap().to_string(), o)
            })
            .collect();
        // the Rust-only factory "Dark techno" follows Python's factory presets
        let mut want = want;
        let at = want.iter().take_while(|e| e.1 == Origin::Factory).count();
        want.insert(at, ("Dark techno".into(), Origin::Factory));
        assert_eq!(store.list(kind(k)), want, "{k}");
    }
    assert!(!PresetStore::new(None).can_save());
    assert_eq!(PresetStore::new(None).list(PresetKind::Drums).len(), 4);
}

#[test]
fn corrupt_or_oversize_files_are_skipped() {
    let store = PresetStore::new(Some(files()));
    for name in ["Roto", "Otro", "Forma", "Grande"] {
        assert!(store.get(PresetKind::Synth, name).is_err(), "{name}");
    }
    assert!(store.get(PresetKind::Synth, "Acordes de prueba").is_ok());
    assert!(store.get(PresetKind::Synth, "Bajo").is_ok());
}

#[test]
fn store_rejects_reserved_empty_and_symbol_names() {
    let dir = temp_dir("names");
    let store = PresetStore::new(Some(dir.clone()));
    let data = factory(PresetKind::Synth).remove(0);
    let reserved = store.save(PresetKind::Synth, "pad DE acordes", &data);
    assert_eq!(
        reserved.unwrap_err(),
        "ese nombre es de un preset de fábrica"
    );
    for bad in ["", "   ", "!!!"] {
        assert_eq!(
            store.save(PresetKind::Synth, bad, &data).unwrap_err(),
            "nombre vacío"
        );
    }
    let wrong = store.save(PresetKind::Drums, "Ok", &data).unwrap_err();
    assert_eq!(wrong, "not a drums preset");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "nothing written"
    );
    let none = PresetStore::new(None).save(PresetKind::Synth, "Ok", &data);
    assert_eq!(none.unwrap_err(), "no hay carpeta de presets");
}

#[test]
fn save_is_atomic_and_leaves_no_tmp() {
    let dir = temp_dir("atomic");
    let store = PresetStore::new(Some(dir.clone()));
    let data = factory(PresetKind::Drums).remove(1);
    assert_eq!(
        store.save(PresetKind::Drums, " Mi/ritmo! ", &data).unwrap(),
        "Miritmo"
    );
    store.save(PresetKind::Drums, "Miritmo", &data).unwrap(); // overwrite is silent
    let names: Vec<String> = std::fs::read_dir(dir.join("drums"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["Miritmo.json"]);
    let got = store.get(PresetKind::Drums, "Miritmo").unwrap();
    assert_eq!(got["name"], "Miritmo");
    assert_eq!(
        store.list(PresetKind::Drums).last().unwrap(),
        &("Miritmo".to_string(), Origin::User)
    );
}

#[test]
fn save_is_byte_identical_to_python() {
    let r = reference();
    let src = PresetStore::new(Some(files()));
    for (k, id) in [("synth", SYNTH), ("drums", DRUMS)] {
        let name = r["saved"][k].as_str().unwrap();
        let path = files().join(k).join(format!("{name}.json"));
        let bytes = std::fs::read(&path).unwrap();
        let loaded = src.get(kind(k), name).unwrap();
        assert_eq!(dumps(&loaded).as_bytes(), &bytes[..], "plain round trip");
        // Through the engine: apply, render, capture from the mirror, save.
        let mut rig = Rig::new();
        rig.apply(id, &loaded);
        let captured = capture(&rig.view(id), "otro");
        let dir = temp_dir(&format!("bytes-{k}"));
        let store = PresetStore::new(Some(dir.clone()));
        assert_eq!(store.save(kind(k), name, &captured).unwrap(), name);
        let mine = std::fs::read(dir.join(k).join(format!("{name}.json"))).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&mine),
            String::from_utf8_lossy(&bytes),
            "{k}"
        );
    }
}

#[test]
fn capture_apply_roundtrip_through_engine() {
    let mut rig = Rig::new();
    let pad = factory(PresetKind::Synth).remove(0);
    rig.model.send(Command::SetParam {
        id: SYNTH,
        param: nomad_core::params::pid::MASTER_VOLUME as u16,
        value: 0.3,
    });
    rig.apply(SYNTH, &pad);
    let got = capture(&rig.view(SYNTH), "Pad de acordes");
    assert!(same(&got, &pad), "{got:#}");
    let m = &rig.view(SYNTH).shared;
    assert!(
        !m.seq_enabled.load(std::sync::atomic::Ordering::Relaxed),
        "SEQ stays off"
    );
    // An off step keeps its chord, like Python's array copy.
    let mut p = pad.clone();
    p["seq"]["on"][0] = 0.into();
    rig.apply(SYNTH, &p);
    let again = capture(&rig.view(SYNTH), "");
    assert_eq!(again["seq"]["extra"][0], pad["seq"]["extra"][0]);
}

#[test]
fn drum_apply_regenerates_shots_and_keeps_bpm() {
    let mut rig = Rig::new();
    rig.model.send(Command::SetBpm {
        id: DRUMS,
        bpm: 128.0,
    });
    rig.render();
    let breakbeat = factory(PresetKind::Drums).remove(1);
    let cmds = apply_commands(&rig.view(DRUMS), &breakbeat, SR).unwrap();
    let banks: Vec<(u8, usize)> = cmds
        .iter()
        .filter_map(|c| match c {
            Command::DrumSetBank { track, shot, .. } => Some((*track, shot.len())),
            _ => None,
        })
        .collect();
    assert_eq!(banks.len(), 6);
    let snare = render_one_shot(1, SR, 0.0, 0.4);
    assert_eq!(banks[1], (1, snare.len()));
    for c in cmds {
        rig.model.send(c);
    }
    rig.render();
    let v = rig.view(DRUMS);
    assert_eq!(v.shared.eff_bpm.get(), 128.0);
    assert!(same(&capture(&v, "Breakbeat"), &breakbeat));
    // The six replaced shots come back to be freed on this side.
    assert!(rig.model.tick() >= 6);
}

#[test]
fn apply_rejects_invalid_and_sends_nothing() {
    let rig = Rig::new();
    let mut bad = factory(PresetKind::Synth).remove(1);
    bad["seq"]["vel"][2] = 0.into();
    assert_eq!(
        apply_commands(&rig.view(SYNTH), &bad, SR).err().as_deref(),
        Some("bad sequence")
    );
    let drums = factory(PresetKind::Drums).remove(0);
    assert_eq!(
        apply_commands(&rig.view(SYNTH), &drums, SR)
            .err()
            .as_deref(),
        Some("not a synth preset")
    );
}

#[test]
fn every_factory_preset_is_valid() {
    for k in [PresetKind::Synth, PresetKind::Drums] {
        for p in factory(k) {
            assert_eq!(check(k, &p), Ok(()), "{}", p["name"]);
        }
    }
}
