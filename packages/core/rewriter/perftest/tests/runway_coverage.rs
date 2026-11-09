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
    assert!(analyze(source).is_some(), "real runway encoding helper rejected");
}

#[test]
fn runway_encoding_script_is_rewritable() {
    let source = r#"runTest(async () => {
        const el = document.getElementById("test");
        assertEqual(el.textContent, "café", "Content-Type charset=utf-8 should decode correctly");
    });"#;
    assert!(analyze(source).is_some(), "real encoding assertion script rejected");
}
