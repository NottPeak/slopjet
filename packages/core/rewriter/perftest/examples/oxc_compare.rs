//! Benchmark the actual OXC visitor against the AST-free deferred fast path.
//! Run from packages/core/rewriter:
//! cargo run --release -p slopjet-perftest-scanner --example oxc_compare -- 20000
use std::{error::Error, time::Instant};
use js::{cfg::{Config, Flags, UrlRewriter}, Rewriter};
use oxc::allocator::{Allocator, StringBuilder};
use slopjet_perftest_scanner::stream::StreamingProgram;

struct Noop;
impl UrlRewriter for Noop {
    fn rewrite(
        &self, _: &Config, _: &Flags, url: &str,
        builder: &mut StringBuilder, _: bool
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        builder.push_str(url);
        Ok(())
    }
}
fn config() -> Config {
    Config {
        prefix: "/scramjet/".into(), wrapfn: "$wrap".into(),
        wrappropertybase: "$property$".into(),
        wrappropertyfn: "$wrapProperty".into(),
        cleanrestfn: "$cleanRest".into(), importfn: "$import".into(),
        rewritefn: "$rewrite".into(), wrappostmessagefn: "$postMessage".into(),
        metafn: "$meta".into(), pushsourcemapfn: "$pushmap".into(),
        trysetfn: "$tryset".into(), templocid: "$tempLoc".into(),
        tempunusedid: "$tempUnused".into(),
    }
}
fn flags() -> Flags {
    Flags {
        base: "https://example.com/".into(), sourcetag: "bench".into(),
        is_module: false, capture_errors: false, scramitize: false,
        do_sourcemaps: false, disable_computed_wrap: false,
        destructure_rewrites: false,
    }
}
fn fast(source: &str) -> String {
    let mut scanner = StreamingProgram::new("$wrap", "$property$");
    for chunk in source.as_bytes().chunks(4096) {
        scanner.write(std::str::from_utf8(chunk).unwrap());
    }
    scanner.finish().expect("fixture must be in fast subset")
}
fn main() {
    let count = std::env::args().nth(1)
        .and_then(|s| s.parse::<usize>().ok()).unwrap_or(10000);
    let samples = [
        ("single-member", "window.location;"),
        ("multi-statement", "'use strict'; window.location; top.location; document.cookie;"),
        ("member-chain", "window.location.href; this.location; parent.top;"),
    ];
    for (name, input) in samples {
        let mut allocator = Allocator::default();
        let rewriter = Rewriter::new();
        let baseline = rewriter.rewrite(&allocator, input, config(), flags(), &Noop)
            .expect("OXC baseline");
        assert_eq!(fast(input).as_bytes(), baseline.js.as_slice(), "fixture {name}");
        drop(baseline);

        let start = Instant::now();
        for _ in 0..count {
            let value = fast(input);
            std::hint::black_box(value);
        }
        let fast_time = start.elapsed();

        let start = Instant::now();
        for _ in 0..count {
            let value = rewriter.rewrite(&allocator, input, config(), flags(), &Noop)
                .expect("OXC rewrite");
            std::hint::black_box(value.js.as_slice());
            drop(value);
            // Reuse allocator for fair per-request memory behavior.
            allocator.reset();
        }
        let oxc_time = start.elapsed();
        println!("{name}: n={count} input_bytes={} fast={fast_time:?} oxc={oxc_time:?} speedup={:.2}x",
            input.len(), oxc_time.as_secs_f64() / fast_time.as_secs_f64());
    }
}
