mod common;

use common::{Blob, assert_close};

#[test]
fn blob_loads_python_fixture() {
    let b = Blob::open("refio_selftest");
    let ramp: Vec<f64> = (0..8).map(f64::from).collect();
    assert_eq!(b.get("ramp"), ramp.as_slice());
    assert_eq!(b.get("half"), &[0.5]);
    assert_eq!(b.meta("ramp")["tag"], "x");
    assert_eq!(b.labels(), ["ramp", "half"]);
}

#[test]
#[should_panic]
fn assert_close_rejects_far_values() {
    assert_close(&[1.0], &[1.0 + 1e-8], "far");
}

#[test]
fn assert_close_accepts_near_values() {
    assert_close(&[1.0, 0.0], &[1.0 + 1e-10, 5e-10], "near");
}
