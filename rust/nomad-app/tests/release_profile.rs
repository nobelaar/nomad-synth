//! `nomad --version` and the release profile (the audio callback needs unwinding).

use std::process::Command;

#[test]
fn version_flag_prints_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_nomad"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(out.status.success(), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("nomad {}", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn release_profile_keeps_unwinding() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml");
    let text = std::fs::read_to_string(p).unwrap();
    let release = text
        .split("[profile.release]")
        .nth(1)
        .expect("a [profile.release] section");
    let release = release.split("\n[").next().unwrap();
    assert!(
        !release.replace(' ', "").contains("panic=\"abort\""),
        "catch_unwind in the audio callback needs panic = \"unwind\""
    );
    for key in ["lto = \"thin\"", "codegen-units = 1", "strip = \"symbols\""] {
        assert!(release.contains(key), "missing {key}");
    }
}
