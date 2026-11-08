//! Differential source-output tests against Slopjet's existing OXC visitor.
//! Expected equality here is necessary but not sufficient for full parity.
use std::error::Error;
use js::cfg::{Config, Flags, UrlRewriter};
use js::Rewriter;
use oxc::allocator::{Allocator, StringBuilder};
use slopjet_perftest_scanner::gate::{identity_safe, rewrite_simple_global};

struct NoopUrls;
impl UrlRewriter for NoopUrls {
    fn rewrite(
        &self, _: &Config, _: &Flags, url: &str,
        builder: &mut StringBuilder, _: bool,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        builder.push_str(url);
        Ok(())
    }
}
fn oxc_output(source: &str) -> String {
    let config = Config {
        prefix: "/scramjet/".into(), wrapfn: "$wrap".into(),
        wrappropertybase: "$property$".into(),
        wrappropertyfn: "$wrapProperty".into(),
        cleanrestfn: "$cleanRest".into(), importfn: "$import".into(),
        rewritefn: "$rewrite".into(), wrappostmessagefn: "$postMessage".into(),
        metafn: "$meta".into(), pushsourcemapfn: "$pushmap".into(),
        trysetfn: "$tryset".into(), templocid: "$tempLoc".into(),
        tempunusedid: "$tempUnused".into(),
    };
    let flags = Flags {
        base: "https://example.com/".into(),
        sourcetag: "testing".into(), is_module: false,
        capture_errors: false, scramitize: false,
        do_sourcemaps: false, disable_computed_wrap: false,
        destructure_rewrites: false,
    };
    let allocator = Allocator::default();
    let rewriter = Rewriter::new();
    let out = rewriter.rewrite(&allocator, source, config, flags, &NoopUrls).unwrap();
    String::from_utf8(out.js.as_slice().to_vec()).unwrap()
}
#[test]
fn inert_fast_path_matches_oxc() {
    for source in ["", ";", " ; ; ", "// hi", "/* comment */", "'use strict';", "\"value\";"] {
        assert!(identity_safe(source));
        assert_eq!(oxc_output(source), source, "{source:?}");
    }
}
#[test]
fn standalone_global_fast_path_matches_oxc() {
    for source in ["location;", " parent; ", "top", "eval;", "\nlocation ; "] {
        let fast = rewrite_simple_global(source, "$wrap").unwrap();
        assert_eq!(oxc_output(source), fast, "{source:?}");
    }
}

#[test]
fn static_member_fast_path_matches_oxc() {
    for source in [
        "window.location;",
        "top.location;",
        " parent . top . eval; ",
        "this.location",
        "document.cookie;",
        "window.location.href;",
        "obj . parent . location;",
    ] {
        let fast = slopjet_perftest_scanner::simple::rewrite(
            source, "$wrap", "$property$"
        ).expect("fixture recognized by fast path");
        assert_eq!(oxc_output(source), fast, "{source:?}");
    }
}
