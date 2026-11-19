use nomad_app::synth_layout::{
    SECTIONS, WidgetKind, selector_labels, selector_offset, short_label, widget_kind,
};
use nomad_core::params::{SYNTH_PARAMS, pid};

fn reference() -> serde_json::Value {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reference/synth_layout.json"
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn id(name: &str) -> usize {
    SYNTH_PARAMS.iter().position(|s| s.name == name).unwrap()
}

#[test]
fn sections_match_python() {
    let r = reference();
    let want = r["sections"].as_array().unwrap();
    assert_eq!(SECTIONS.len(), want.len());
    for (s, w) in SECTIONS.iter().zip(want) {
        assert_eq!(s.key, w["key"]);
        assert_eq!(s.title, w["title"]);
        let names: Vec<&str> = s.params.iter().map(|&p| SYNTH_PARAMS[p].name).collect();
        let want: Vec<&str> = w["params"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(names, want, "{}", s.key);
    }
}

#[test]
fn kinds_labels_offsets_short_match_python() {
    let r = reference();
    for (name, kind) in r["kinds"].as_object().unwrap() {
        let k = if kind == "selector" {
            WidgetKind::Selector
        } else {
            WidgetKind::Knob
        };
        assert_eq!(widget_kind(id(name)), k, "{name}");
    }
    for (name, labels) in r["labels"].as_object().unwrap() {
        let want: Vec<&str> = labels
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(selector_labels(id(name)), want.as_slice(), "{name}");
        assert_eq!(
            selector_offset(id(name)),
            r["offsets"][name].as_i64().unwrap(),
            "{name}"
        );
    }
    for (name, short) in r["short"].as_object().unwrap() {
        assert_eq!(short_label(id(name)), short.as_str().unwrap(), "{name}");
    }
}

#[test]
fn hidden_and_master_are_absent() {
    for s in SECTIONS {
        assert!(
            !s.params.contains(&pid::OSC1_SYNC) && !s.params.contains(&pid::MASTER_VOLUME),
            "{}",
            s.key
        );
    }
    assert_eq!(selector_labels(pid::LFO1_WAVE).len(), 5);
    assert_eq!(selector_labels(pid::OSC1_WAVE).len(), 4);
    assert_eq!(selector_offset(pid::POLYPHONY), 1);
}
