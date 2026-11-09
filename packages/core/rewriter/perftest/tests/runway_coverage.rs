use slopjet_perftest_scanner::semantic::SemanticProgram;

fn analyze(source: &str) -> Option<String> {
    let mut engine = SemanticProgram::new("$wrap", "$property$", "$computed", false)
        .with_helpers("$import", "$rewrite", "$postmessage", "http://example.test/")
        .with_assignment("$tryset");
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


#[test]
fn isolate_emit_binding_construct() {
    let cases = [
        ("empty", "function emitBinding(name, payload) {}"),
        ("local-number", "function emitBinding(name,payload) { var fn = 1; }"),
        ("local-ident", "function emitBinding(name,payload) { var fn = globalThis; }"),
        ("local-static-member", "function emitBinding(name,payload) { var fn = globalThis.name; }"),
        ("local-computed-member", "function emitBinding(name,payload) { var fn = globalThis[name]; }"),
        ("bare-computed-member", "globalThis[name];"),
        ("bare-var-computed-member", "var fn = globalThis[name];"),
        ("bare-function-computed-member", "function emitBinding(name,payload) {globalThis[name];}"),
        ("if", "function emitBinding(name, payload) { var fn = globalThis[name]; if (typeof fn === 'function') {} }"),
        ("call", "function emitBinding(name, payload) { var fn = globalThis[name]; if (typeof fn === 'function') { fn(JSON.stringify(payload)); } }"),
    ];
    let failures: Vec<_> = cases.iter().filter_map(|(label,src)| {
        if analyze(src).is_none() {Some(*label)} else {None}
    }).collect();
    assert!(failures.is_empty(), "unsupported variants: {failures:?}");
}


#[test]
fn location_runway_scripts_are_semantically_supported() {
    for (name, js) in [
        ("hash", r##"runTest(async () => {
            location.hash = "start";
            assertEqual(location.hash, "#start", "precondition");
            location.hash = "target";
            await new Promise((r) => setTimeout(r, 300));
            assertEqual(location.hash, "#target", "after");
            location.hash = "";
        }, true);"##),
        ("stringify", r##"runTest(async () => {
            assertEqual(location.toString(), location.href);
            assertEqual(String(location), location.href);
            assertEqual([location].join(), location.href);
        }, true);"##),
        ("identity", r##"runTest(async () => {
            assertEqual(window.location, location);
            assertEqual(document.location, location);
            assertEqual(self.location, location);
            assertEqual(globalThis.location, location);
        }, true);"##),
        ("bare-assign", r##"runTest(async () => {
            location.hash = "start";
            location = "#target";
            await new Promise((r) => setTimeout(r, 300));
            assertEqual(location.hash, "#target");
        }, true);"##),
    ] {
        assert!(analyze(js).is_some(), "Runway location program unsupported: {name}");
    }
}
#[test]
fn eval_argument_runway_scripts_are_semantically_supported() {
    for (name, js) in [
        ("extra", r##"runTest(async () => {
            assertEqual(eval("1+1", "2+2"), 2);
            assertEqual(eval("1+1",), 2);
        }, true);"##),
        ("spread", r##"runTest(async () => {
            assertEqual(eval(...["1+1"]), 2);
        }, true);"##),
        ("indirect", r##"runTest(async () => {
            assertEqual((0,eval)("1+1", "2+2"), 2);
        }, true);"##),
    ] {
        assert!(analyze(js).is_some(), "Runway eval program unsupported: {name}");
    }
}


#[test]
fn locate_bare_location_assignment_gap() {
    let variants = [
        ("top", r##"location = "#target";"##),
        ("inside-block", r##"{ location = "#target"; }"##),
        ("inside-function", r##"function f() { location = "#target"; }"##),
        ("inside-arrow", r##"const f = () => { location = "#target"; };"##),
        ("inside-async-arrow", r##"runTest(async () => { location = "#target"; }, true);"##),
        ("after-member-write", r##"runTest(async () => { location.hash="start"; location="#target"; }, true);"##),
        ("with-promise", r##"runTest(async () => { location="#target"; await new Promise((r) => setTimeout(r, 300)); }, true);"##),
    ];
    let failures: Vec<_> = variants.iter().filter_map(|(name, src)| {
        if analyze(src).is_none() { Some(*name) } else { None }
    }).collect();
    assert!(failures.is_empty(), "unsupported bare location variants: {failures:?}");
}
