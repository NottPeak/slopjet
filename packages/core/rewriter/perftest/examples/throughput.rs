//! Run: cargo run --release -p slopjet-perftest-scanner --example throughput -- 200
//! Measures the real semantic scanner under independent-request load.
//! This is not a browser, wire-network, or OXC comparison benchmark.
use slopjet_perftest_scanner::requests::{process_sequential, Request};
#[cfg(not(target_arch = "wasm32"))]
use slopjet_perftest_scanner::requests::process_parallel;
use std::time::Instant;

fn main() {
    let count = std::env::args().nth(1)
        .and_then(|s| s.parse::<usize>().ok()).unwrap_or(1000);
    let source = concat!(
        "const url = window.location.href;\n",
        "function f(top){return top + parent.location;}\n",
        "const value = obj[location];\n",
        "const message = \u{60}host=${top.location}\u{60};\n",
    ).repeat(100);
    let requests: Vec<_> = (0..count)
        .map(|i| Request { id: i as u64, source: source.clone() })
        .collect();
    let bytes = count * source.len();
    let start = Instant::now();
    let baseline = process_sequential(requests.clone());
    let elapsed = start.elapsed();
    println!("sequential: requests={} bytes={} elapsed={:?} MiB/s={:.2}",
        count, bytes, elapsed, bytes as f64 / 1048576.0 / elapsed.as_secs_f64());
    assert!(baseline.iter().all(|x| x.result.is_ok()));
    #[cfg(not(target_arch = "wasm32"))]
    for workers in [1, 2, 4, 8] {
        let start = Instant::now();
        let output = process_parallel(requests.clone(), workers);
        let elapsed = start.elapsed();
        assert_eq!(output, baseline);
        println!("workers={workers}: elapsed={elapsed:?} MiB/s={:.2}",
            bytes as f64 / 1048576.0 / elapsed.as_secs_f64());
    }
}
