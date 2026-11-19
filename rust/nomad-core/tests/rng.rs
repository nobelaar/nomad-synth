mod common;

use common::Blob;
use nomad_core::rng::{NumpyRng, generate_state, seed_sequence_pool};

fn u64s(v: &serde_json::Value) -> Vec<u64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap())
        .collect()
}

fn u128_of(v: &serde_json::Value) -> u128 {
    v.as_str().unwrap().parse().unwrap()
}

#[test]
fn seed_stages_match_numpy() {
    let blob = Blob::open("lfo");
    for label in blob.labels().iter().filter(|l| l.starts_with("rng/")) {
        let m = blob.meta(label);
        let seed = m["seed"].as_u64().unwrap();
        let pool = seed_sequence_pool(seed);
        assert_eq!(
            pool.map(u64::from).to_vec(),
            u64s(&m["pool"]),
            "{label} pool"
        );
        assert_eq!(
            generate_state(&pool).to_vec(),
            u64s(&m["words"]),
            "{label} words"
        );
        let mut rng = NumpyRng::new(seed);
        assert_eq!(
            rng.state(),
            (u128_of(&m["state"]), u128_of(&m["inc"])),
            "{label} state"
        );
        let raw: Vec<u64> = (0..5).map(|_| rng.next_u64()).collect();
        assert_eq!(raw, u64s(&m["raw"]), "{label} raw");
    }
}

#[test]
fn first_1000_match_numpy() {
    let blob = Blob::open("lfo");
    let labels: Vec<String> = blob
        .labels()
        .into_iter()
        .filter(|l| l.starts_with("rng/"))
        .collect();
    assert_eq!(labels.len(), 5);
    for label in &labels {
        let mut rng = NumpyRng::new(blob.meta(label)["seed"].as_u64().unwrap());
        for (i, &want) in blob.get(label).iter().enumerate() {
            assert_eq!(rng.random(), want, "{label}[{i}]");
        }
    }
}
