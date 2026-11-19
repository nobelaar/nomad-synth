use nomad_app::bench::{BenchOpts, parse_bench, run_bench};

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

#[test]
fn bench_runs_and_reports() {
    let r = run_bench(8, 1, 1.0, 512);
    assert!(r.blocks > 50, "{}", r.blocks);
    for v in [r.mean_pct, r.p50_pct, r.p99_pct, r.max_pct] {
        assert!(v.is_finite() && v > 0.0);
    }
    assert!(r.p50_pct <= r.p99_pct && r.p99_pct <= r.max_pct);
}

#[test]
fn bench_scales_with_voices() {
    // Median, not mean: robust to a stray scheduler hiccup in a 1 s run.
    let one = run_bench(1, 1, 1.0, 512);
    let eight = run_bench(8, 1, 1.0, 512);
    assert!(
        eight.p50_pct > one.p50_pct,
        "1 voice {} vs 8 voices {}",
        one.p50_pct,
        eight.p50_pct
    );
}

#[test]
fn bench_arg_parsing() {
    assert_eq!(
        parse_bench(&[]),
        Ok(BenchOpts {
            voices: 8,
            synths: 2,
            seconds: 10.0,
            block: 512
        })
    );
    assert_eq!(
        parse_bench(&args("--voices 4 --synths 1 --seconds 2.5 --block 256")),
        Ok(BenchOpts {
            voices: 4,
            synths: 1,
            seconds: 2.5,
            block: 256
        })
    );
    assert!(parse_bench(&args("--voices")).is_err());
    assert!(parse_bench(&args("--voices x")).is_err());
    assert!(parse_bench(&args("--wat 3")).is_err());
    assert!(parse_bench(&args("--voices 0")).is_err());
    assert!(parse_bench(&args("--block 0")).is_err());
    assert!(parse_bench(&args("--synths 99")).is_err());
}
