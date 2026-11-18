mod common;

use common::Blob;
use nomad_core::rng::NumpyRng;

#[test]
fn first_10000_match_numpy_exactly() {
    for seed in 1..=6u64 {
        let blob = Blob::open(&format!("normal_{seed}"));
        let mut rng = NumpyRng::new(seed);
        for (i, &want) in blob.get("normal").iter().enumerate() {
            let got = rng.standard_normal();
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "seed {seed} [{i}]: {got} vs {want}"
            );
        }
    }
}

#[test]
fn tail_hits_match_numpy() {
    let blob = Blob::open("normal_tail");
    let m = blob.meta("tail_hits");
    let n = m["n"].as_u64().unwrap() as usize;
    let idx: Vec<usize> = m["indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect();
    assert_eq!(idx.len(), 20);
    let mut rng = NumpyRng::new(m["seed"].as_u64().unwrap());
    let all: Vec<f64> = (0..n).map(|_| rng.standard_normal()).collect();
    for (&i, &want) in idx.iter().zip(blob.get("tail_hits")) {
        assert_eq!(all[i].to_bits(), want.to_bits(), "[{i}]");
        assert!(want.abs() > 3.65);
    }
}

#[test]
fn mean_and_variance_are_sane() {
    let mut rng = NumpyRng::new(42);
    let x: Vec<f64> = (0..100_000).map(|_| rng.standard_normal()).collect();
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / x.len() as f64;
    assert!(
        mean.abs() < 0.02 && (0.97..1.03).contains(&var),
        "{mean} {var}"
    );
}
