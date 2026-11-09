use slopjet_perftest_scanner::semantic::SemanticProgram;

fn analyze(source: &str) -> Option<String> {
    let mut engine = SemanticProgram::new("$wrap", "$property$", "$computed", false)
        .with_helpers("$import", "$rewrite", "$postmessage", "http://example.test/");
    engine.write(source);
    engine.finish()
}

#[test]
fn runway_encoding_helpers_are_rewritable() {
    let source = include_str!("../fixtures/encoding-helper.js");
    if analyze(source).is_none() {
        // Isolate which top-level helper is unsupported without an AST.
        let names = [
            "function emitBinding(", "function pass(", "function fail(",
            "function assertConsistent(", "function assert(",
            "function assertEqual(", "function waitForTestFunctions(",
            "async function runTest("
        ];
        let mut offsets: Vec<(&str, usize)> = names.iter()
            .map(|name| (*name, source.find(name).expect("missing helper")))
            .collect();
        offsets.sort_by_key(|(_, offset)| *offset);
        for i in 0..offsets.len() {
            let start = offsets[i].1;
            let end = offsets.get(i+1).map(|p| p.1).unwrap_or(source.len());
            let segment = &source[start..end];
            assert!(analyze(segment).is_some(), "unsupported helper: {}", offsets[i].0);
        }
        panic!("combined runway helpers unsupported despite each helper passing");
    }
}

#[test]
fn runway_encoding_script_is_rewritable() {
    let source = r#"runTest(async () => {
        const el = document.getElementById("test");
        assertEqual(el.textContent, "café", "Content-Type charset=utf-8 should decode correctly");
    });"#;
    assert!(analyze(source).is_some(), "real encoding assertion script rejected");
}
