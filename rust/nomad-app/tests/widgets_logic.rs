use nomad_app::widgets::knob_model::KnobModel;
use nomad_app::widgets::scope_math::{find_trigger, to_points};
use nomad_core::params::{DRUM_PARAMS, ParamSpec, SYNTH_PARAMS, find};

fn reference() -> serde_json::Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/gui_logic.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn spec(reg: &str, name: &str) -> &'static ParamSpec {
    find(
        if reg == "drums" {
            &DRUM_PARAMS
        } else {
            &SYNTH_PARAMS
        },
        name,
    )
    .unwrap()
}

#[test]
fn format_matches_python() {
    let r = reference();
    let cases = r["format"].as_array().unwrap();
    assert!(cases.len() > 700);
    let mut bad = Vec::new();
    for c in cases {
        let s = spec(c["reg"].as_str().unwrap(), c["param"].as_str().unwrap());
        let m = KnobModel::new(s, c["value"].as_f64().unwrap());
        let want = c["text"].as_str().unwrap();
        if m.format_value() != want {
            bad.push(format!(
                "{} {} -> {:?} vs {want:?}",
                s.name,
                c["value"],
                m.format_value()
            ));
        }
        assert!(
            (m.norm() - c["norm"].as_f64().unwrap()).abs() <= 1e-12,
            "{} norm",
            s.name
        );
    }
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn drag_wheel_reset_match_python() {
    let r = reference();
    for g in r["gestures"].as_array().unwrap() {
        let s = spec("synth", g["param"].as_str().unwrap());
        let mut m = KnobModel::new(s, s.default);
        for (op, want) in g["ops"]
            .as_array()
            .unwrap()
            .iter()
            .zip(g["steps"].as_array().unwrap())
        {
            match op[0].as_str().unwrap() {
                "drag" => m.set_from_drag(op[1].as_f64().unwrap(), op[2].as_bool().unwrap()),
                "wheel" => {
                    m.set_from_wheel(op[1].as_i64().unwrap() as i32, op[2].as_bool().unwrap())
                }
                _ => m.reset(),
            }
            let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 + 1e-12 * b.abs();
            assert!(
                close(m.norm(), want["norm"].as_f64().unwrap()),
                "{} {op}",
                s.name
            );
            assert!(
                close(m.value(), want["value"].as_f64().unwrap()),
                "{} {op}",
                s.name
            );
            assert_eq!(m.format_value(), want["text"].as_str().unwrap());
        }
    }
}

#[test]
fn non_finite_is_rejected() {
    let s = spec("synth", "filt_cutoff");
    let mut m = KnobModel::new(s, 1000.0);
    let before = m.norm();
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(m.set_norm(x).is_err() && m.set_value(x).is_err());
        assert_eq!(m.norm(), before);
    }
    assert!(m.set_norm(2.0).is_ok());
    assert_eq!(m.norm(), 1.0);
    m.set_from_drag(0.0, false);
    assert_eq!(m.norm(), 1.0);
}

/// Minimal reader for the `gui_scope` blob (tools/refio.py layout).
struct Blob {
    data: Vec<f64>,
    entries: serde_json::Map<String, serde_json::Value>,
}

impl Blob {
    fn open() -> Blob {
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../reference/gui_scope");
        let raw = std::fs::read(format!("{base}.f64")).unwrap();
        let data = raw
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| f64::from_le_bytes(*c))
            .collect();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(format!("{base}.json")).unwrap())
                .unwrap();
        Blob {
            data,
            entries: json["entries"].as_object().unwrap().clone(),
        }
    }
    fn get(&self, label: &str) -> &[f64] {
        let e = &self.entries[label];
        let (o, n) = (
            e["offset"].as_u64().unwrap() as usize,
            e["len"].as_u64().unwrap() as usize,
        );
        &self.data[o..o + n]
    }
}

fn point_entries(b: &Blob) -> Vec<(String, serde_json::Value)> {
    b.entries
        .iter()
        .filter(|(k, _)| k.starts_with("pts/"))
        .map(|(k, v)| (k.clone(), v["meta"].clone()))
        .collect()
}

#[test]
fn trigger_matches_python() {
    let b = Blob::open();
    let entries = point_entries(&b);
    assert_eq!(entries.len(), 24);
    for (label, m) in &entries {
        let sig: Vec<f32> = b
            .get(m["signal"].as_str().unwrap())
            .iter()
            .map(|&x| x as f32)
            .collect();
        assert_eq!(
            find_trigger(&sig) as u64,
            m["trigger"].as_u64().unwrap(),
            "{label}"
        );
    }
}

#[test]
fn points_match_python() {
    let b = Blob::open();
    let mut pts = Vec::new();
    for (label, m) in &point_entries(&b) {
        let sig: Vec<f32> = b
            .get(m["signal"].as_str().unwrap())
            .iter()
            .map(|&x| x as f32)
            .collect();
        let (w, h) = (
            m["width"].as_u64().unwrap() as usize,
            m["height"].as_u64().unwrap() as usize,
        );
        to_points(&sig, w, h, &mut pts);
        let want = b.get(label);
        assert_eq!(pts.len() * 2, want.len(), "{label}");
        for (i, (&(x, y), xy)) in pts.iter().zip(want.chunks(2)).enumerate() {
            let close = |a: f32, b: f64| (f64::from(a) - b).abs() <= 1e-6 * b.abs().max(1.0);
            assert!(
                close(x, xy[0]) && close(y, xy[1]),
                "{label}[{i}]: ({x}, {y}) vs {xy:?}"
            );
        }
    }
}

#[test]
fn scope_handles_empty_and_nan() {
    let mut pts = Vec::new();
    assert_eq!(find_trigger(&[]), 0);
    to_points(&[], 100, 50, &mut pts);
    assert_eq!(pts, [(0.0, 25.0), (100.0, 25.0)]);
    let nan = [f32::NAN; 300];
    assert_eq!(find_trigger(&nan), 0);
    to_points(&nan, 100, 50, &mut pts);
    assert!(pts.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
    to_points(&[0.5; 7], 0, 50, &mut pts);
}
