use std::path::PathBuf;

use nomad_core::params::SYNTH_PARAMS;
use nomad_runtime::midi::map::{MidiMap, default_config_dir};
use nomad_runtime::midi::parse::{MidiEvent, MidiKind};

fn reference() -> serde_json::Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/midi_map.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn files_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reference/midi_map_files"
    ))
}

/// A fresh, unique temporary directory (no dependency for it).
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nomad-midimap-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn bindings_json(m: &MidiMap) -> serde_json::Value {
    serde_json::Value::Array(
        m.bindings()
            .iter()
            .map(|((p, ch, cc), v)| serde_json::json!([p, ch, cc, v]))
            .collect(),
    )
}

fn cc(ch: u8, c: u8, value: u8, kind: &str) -> MidiEvent {
    let kind = if kind == "cc" {
        MidiKind::Cc
    } else {
        MidiKind::NoteOn
    };
    MidiEvent {
        kind,
        note: c,
        velocity: 0.5,
        cc: c,
        value,
        channel: ch,
        bend: 0.0,
    }
}

#[test]
fn scenarios_match_python() {
    let r = reference();
    let scen = r["scenarios"].as_object().unwrap();
    assert_eq!(scen.len(), 11);
    for (label, sc) in scen {
        let dir = temp_dir(label);
        let mut m = MidiMap::new(Some(dir.join(format!("{label}.json"))));
        for (op, want) in sc["ops"]
            .as_array()
            .unwrap()
            .iter()
            .zip(sc["steps"].as_array().unwrap())
        {
            let op = op.as_array().unwrap();
            let mut result = serde_json::Value::Null;
            match op[0].as_str().unwrap() {
                "arm" => m.arm(op[1].as_str()).unwrap(),
                "unbind" => m.unbind(op[1].as_str().unwrap()),
                "save" => m.save().unwrap(),
                "load" => m.load(),
                _ => {
                    let u = |k: usize| op[k].as_u64().unwrap() as u8;
                    let e = cc(u(2), u(3), u(4), op[5].as_str().unwrap());
                    if let Some((pid, v)) = m.handle(op[1].as_str().unwrap(), &e) {
                        result = serde_json::json!([SYNTH_PARAMS[pid].name, v]);
                    }
                }
            }
            match (&result, &want["result"]) {
                (serde_json::Value::Array(g), serde_json::Value::Array(w)) => {
                    assert_eq!(g[0], w[0], "{label}");
                    let (a, b) = (g[1].as_f64().unwrap(), w[1].as_f64().unwrap());
                    assert!(
                        (a - b).abs() <= 1e-12 + 1e-12 * b.abs(),
                        "{label}: {a} vs {b}"
                    );
                }
                (g, w) => assert_eq!(g, w, "{label} {op:?}"),
            }
            assert_eq!(bindings_json(&m), want["bindings"], "{label} {op:?}");
            assert_eq!(
                m.learning().map(String::from),
                want["learning"].as_str().map(String::from),
                "{label}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn loads_python_written_file() {
    let r = reference();
    for (name, want) in r["files"].as_object().unwrap() {
        let mut m = MidiMap::new(Some(files_dir().join(name)));
        m.load();
        assert_eq!(bindings_json(&m), want["bindings"], "{name}");
        assert_eq!(
            m.error.is_some(),
            want["error"].as_bool().unwrap(),
            "{name}: {:?}",
            m.error
        );
    }
}

#[test]
fn bad_files_give_empty_map_with_error_and_untouched_file() {
    let dir = temp_dir("bad");
    for name in [
        "corrupt.json",
        "wrong_version.json",
        "bindings_not_list.json",
        "no_version.json",
    ] {
        let path = dir.join(name);
        std::fs::copy(files_dir().join(name), &path).unwrap();
        let (before, mtime) = (
            std::fs::read(&path).unwrap(),
            std::fs::metadata(&path).unwrap().modified().unwrap(),
        );
        let mut m = MidiMap::new(Some(path.clone()));
        m.load();
        assert!(
            m.bindings().is_empty() && m.error.as_deref().is_some_and(|e| e.contains(name)),
            "{name}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn bad_entries_are_dropped_good_kept() {
    let mut m = MidiMap::new(Some(files_dir().join("bad_entries.json")));
    m.load();
    assert!(m.error.is_none());
    let names: Vec<&str> = m.bindings().iter().map(|(_, p)| p.as_str()).collect();
    // The duplicate key keeps its first position with the last value, like a dict.
    assert_eq!(names, ["filt_reso", "glide"]);
}

#[test]
fn save_roundtrips_and_matches_python_format() {
    let dir = temp_dir("save");
    let path = dir.join("midi-map.json");
    let mut m = MidiMap::new(Some(path.clone()));
    m.arm(Some("filt_cutoff")).unwrap();
    m.handle("TEST:TEST MIDI 1 24:0", &cc(0, 21, 1, "cc"));
    m.arm(Some("glide")).unwrap();
    m.handle("OTHER:OTHER MIDI 2 28:0", &cc(3, 7, 1, "cc"));
    m.save().unwrap();
    // Byte-identical to what Python's json.dumps(indent=2) wrote.
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(files_dir().join("saved_after_learn.json")).unwrap()
    );
    let mut back = MidiMap::new(Some(path));
    back.load();
    assert_eq!(back.bindings(), m.bindings());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_creates_directory_and_leaves_no_tmp() {
    let dir = temp_dir("mkdir");
    let path = dir.join("a").join("b").join("midi-map.json");
    MidiMap::new(Some(path.clone())).save().unwrap();
    assert!(path.exists());
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_failure_keeps_old_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("ro");
    let path = dir.join("midi-map.json");
    std::fs::write(&path, b"old contents").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut m = MidiMap::new(Some(path.clone()));
    m.arm(Some("filt_reso")).unwrap();
    m.handle("P", &cc(0, 1, 1, "cc"));
    let r = m.save();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(r.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"old contents");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn path_none_is_noop() {
    let mut m = MidiMap::new(None);
    m.arm(Some("glide")).unwrap();
    m.handle("P", &cc(0, 5, 64, "cc"));
    assert!(m.save().is_ok());
    m.load();
    assert_eq!(
        m.bindings().len(),
        1,
        "load without a path keeps the bindings"
    );
}

#[test]
fn arm_unknown_param_errors() {
    let mut m = MidiMap::new(None);
    assert!(m.arm(Some("nope")).is_err());
    assert_eq!(m.learning(), None);
    assert!(m.arm(Some("filt_cutoff")).is_ok());
    assert!(m.arm(None).is_ok());
    assert_eq!(m.learning(), None);
}

#[test]
fn default_config_dir_honors_xdg() {
    // Only test in this binary that touches the environment.
    let old = std::env::var_os("XDG_CONFIG_HOME");
    // SAFETY: no other test in this binary reads or writes the environment.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test") };
    assert_eq!(
        default_config_dir(),
        PathBuf::from("/tmp/xdg-test/nomad-synth")
    );
    unsafe { std::env::set_var("XDG_CONFIG_HOME", "") };
    let home = std::env::var_os("HOME").unwrap();
    assert_eq!(
        default_config_dir(),
        PathBuf::from(home).join(".config").join("nomad-synth")
    );
    match old {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
}
