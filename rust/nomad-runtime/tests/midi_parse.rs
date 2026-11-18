use nomad_runtime::midi::parse::{MidiEvent, MidiKind, parse_message};

fn kind(s: &str) -> MidiKind {
    match s {
        "note_on" => MidiKind::NoteOn,
        "note_off" => MidiKind::NoteOff,
        "cc" => MidiKind::Cc,
        "bend" => MidiKind::Bend,
        other => panic!("{other}"),
    }
}

#[test]
fn matches_python() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/midi_parse.json");
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = cases.as_array().unwrap();
    assert!(cases.len() > 50);
    for c in cases {
        let bytes: Vec<u8> = c["bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u8)
            .collect();
        let got = parse_message(&bytes);
        let e = &c["event"];
        if e.is_null() {
            assert_eq!(got, None, "{bytes:?}");
            continue;
        }
        let u = |k: &str| e[k].as_u64().unwrap() as u8;
        let want = MidiEvent {
            kind: kind(e["kind"].as_str().unwrap()),
            note: u("note"),
            velocity: e["velocity"].as_f64().unwrap(),
            cc: u("cc"),
            value: u("value"),
            channel: u("channel"),
            bend: e["bend"].as_f64().unwrap(),
        };
        let got = got.unwrap_or_else(|| panic!("{bytes:?}: None"));
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 + 1e-12 * b.abs();
        assert!(
            close(got.velocity, want.velocity) && close(got.bend, want.bend),
            "{bytes:?}"
        );
        assert_eq!(
            MidiEvent {
                velocity: want.velocity,
                bend: want.bend,
                ..got
            },
            want,
            "{bytes:?}"
        );
    }
}

#[test]
fn never_panics_on_any_short_input() {
    let vals = [0x00u8, 0x7F, 0x80, 0x90, 0xB0, 0xE0, 0xF0, 0xF8, 0xFF];
    let _ = parse_message(&[]);
    for &a in &vals {
        let _ = parse_message(&[a]);
        for &b in &vals {
            let _ = parse_message(&[a, b]);
            for &c in &vals {
                let _ = parse_message(&[a, b, c]);
            }
        }
    }
}
