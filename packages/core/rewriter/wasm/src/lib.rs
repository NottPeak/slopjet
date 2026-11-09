//! Slopjet experimental Rust/WASM rewriter.
//!
//! The rewrite path is implemented by the independent lexical engine crate.
//! No OXC parser, allocator, or fallback is linked into this WASM module.
//! Unrecognized syntax is rejected rather than emitted without protection.
use js_sys::{Array, Function, Math, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use slopjet_perftest_scanner::semantic::SemanticProgram;

#[wasm_bindgen(typescript_custom_section)]
const REWRITER_OUTPUT: &str = r#"
export type JsRewriterOutput = {
    js: Uint8Array,
    map: Uint8Array,
    scramtag: string,
    errors: string[],
};
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "JsRewriterOutput")]
    pub type JsRewriterOutput;
}

fn get_str(obj: &Object, field: &str) -> Result<String, JsValue> {
    Reflect::get(obj, &JsValue::from_str(field))?
        .as_string()
        .ok_or_else(|| JsValue::from_str(&format!("missing string config: {field}")))
}

fn get_flag(obj: &Object, field: &str) -> Result<bool, JsValue> {
    Reflect::get(obj, &JsValue::from_str(field))?
        .as_bool()
        .ok_or_else(|| JsValue::from_str(&format!("missing boolean flag: {field}")))
}

fn set(obj: &Object, name: &str, value: &JsValue) -> Result<(), JsValue> {
    if Reflect::set(obj, &JsValue::from_str(name), value)? { Ok(()) }
    else { Err(JsValue::from_str("failed to create WASM output")) }
}

fn unsupported_flags(flags: &Object) -> Result<(), JsValue> {
    for name in ["sourcemaps", "captureErrors", "scramitize", "destructureRewrites"] {
        if get_flag(flags, name)? {
            return Err(JsValue::from_str(&format!(
                "{name} is not yet implemented in the no-parser rewrite engine"
            )));
        }
    }
    Ok(())
}

fn create_output(source: &str) -> Result<JsRewriterOutput, JsValue> {
    let obj = Object::new();
    set(&obj, "js", &Uint8Array::from(source.as_bytes()).into())?;
    set(&obj, "map", &Uint8Array::from(&[0u8, 0, 0, 0][..]).into())?;
    set(&obj, "scramtag", &JsValue::from_str(&format!("slopjet-{}", Math::random())))?;
    set(&obj, "errors", &Array::new().into())?;
    Ok(JsRewriterOutput::from(JsValue::from(obj)))
}

fn rewrite(jsconfig: &Object, flags: &Object, source: &str, base: &str, module: bool, url_rewriter: &Function) -> Result<JsRewriterOutput, JsValue> {
    unsupported_flags(flags)?;
    let mut engine = SemanticProgram::new(
        get_str(jsconfig, "wrapfn")?,
        get_str(jsconfig, "wrappropertybase")?,
        get_str(jsconfig, "wrappropertyfn")?,
        get_flag(flags, "disableComputedWrap")?,
    ).with_helpers(
        get_str(jsconfig, "importfn")?,
        get_str(jsconfig, "rewritefn")?,
        get_str(jsconfig, "wrappostmessagefn")?,
        base,
    ).with_module_meta(get_str(jsconfig, "metafn")?, module)
     .with_assignment(get_str(jsconfig, "trysetfn")?);
    engine.write(source);
    let mut url_error = None;
    let rewritten = engine.finish_with_url_rewriter(|specifier| {
        match url_rewriter.call1(&JsValue::NULL, &JsValue::from_str(specifier)) {
            Ok(value) => value.as_string(),
            Err(error) => {
                url_error = Some(error);
                None
            }
        }
    });
    if let Some(error) = url_error { return Err(error); }
    let rewritten = rewritten.ok_or_else(|| JsValue::from_str(
        "Unsupported JavaScript syntax in standalone Slopjet rewriter",
    ))?;
    create_output(&rewritten)
}

#[wasm_bindgen]
pub struct Rewriter {}

#[wasm_bindgen]
impl Rewriter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Rewriter { Rewriter {} }

    pub fn rewrite_js(
        &mut self,
        jsconfig: &Object,
        jsflags: &Object,
        rewrite_url: Function,
        js: String,
        base: String,
        _url: String,
        module: bool,
    ) -> Result<JsRewriterOutput, JsValue> {
        rewrite(jsconfig, jsflags, &js, &base, module, &rewrite_url)
    }

    pub fn rewrite_js_bytes(
        &mut self,
        jsconfig: &Object,
        jsflags: &Object,
        rewrite_url: Function,
        js: Vec<u8>,
        base: String,
        _url: String,
        module: bool,
    ) -> Result<JsRewriterOutput, JsValue> {
        let source = String::from_utf8(js)
            .map_err(|error| JsValue::from_str(&format!("invalid UTF-8: {error}")))?;
        rewrite(jsconfig, jsflags, &source, &base, module, &rewrite_url)
    }
}

/// One forward scanner with deferred output and request-private state.
/// Unsupported syntax yields null. Network handlers must reject such requests.
#[wasm_bindgen]
pub struct PerftestRewriteSession {
    scanner: Option<SemanticProgram>,
}

#[wasm_bindgen]
impl PerftestRewriteSession {
    #[wasm_bindgen(constructor)]
    pub fn new(
        wrapfn: String,
        propertybase: String,
        propertyfn: String,
        disable_computed_wrap: bool,
        importfn: String,
        rewritefn: String,
        postmessagefn: String,
        base: String,
        metafn: String,
        module: bool,
        trysetfn: String,
    ) -> PerftestRewriteSession {
        PerftestRewriteSession {
            scanner: Some(SemanticProgram::new(
                wrapfn, propertybase, propertyfn, disable_computed_wrap
            ).with_helpers(importfn, rewritefn, postmessagefn, base)
             .with_module_meta(metafn, module)
             .with_assignment(trysetfn)),
        }
    }

    pub fn write(&mut self, chunk: &str) -> Result<(), JsValue> {
        self.scanner.as_mut()
            .ok_or_else(|| JsValue::from_str("scanner already finalized"))?
            .write(chunk);
        Ok(())
    }

    pub fn finish(&mut self, rewrite_url: Option<Function>) -> Result<Option<String>, JsValue> {
        let scanner = self.scanner.take()
            .ok_or_else(|| JsValue::from_str("scanner already finalized"))?;
        let Some(callback) = rewrite_url else { return Ok(scanner.finish()); };
        let mut url_error = None;
        let rewritten = scanner.finish_with_url_rewriter(|specifier| {
            match callback.call1(&JsValue::NULL, &JsValue::from_str(specifier)) {
                Ok(value) => value.as_string(),
                Err(error) => { url_error = Some(error); None }
            }
        });
        if let Some(error) = url_error { return Err(error); }
        Ok(rewritten)
    }
}
