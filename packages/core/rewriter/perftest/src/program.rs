//! Multi-statement no-AST fast path with source-preserving edits.
//!
//! A single forward scan recognizes JavaScript statement boundaries while
//! tracking quotes and comments. It does not guess about ASI, scope, nested
//! functions, declarations, or expression grammar. Each complete statement
//! must be validated by the restricted expression rewriter or inert checker;
//! otherwise the *entire file* is handed to OXC before any output is emitted.
use crate::{gate::identity_safe, simple};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Code, Single, Double, LineComment, BlockComment }

fn transform_statement(segment: &str, wrapfn: &str, propbase: &str) -> Option<String> {
    if identity_safe(segment) { Some(segment.to_string()) }
    else { simple::rewrite(segment, wrapfn, propbase) }
}

/// A correctness-conservative source-preserving fast path.
/// Returns None for any unsupported syntax; no partial results escape.
pub fn rewrite_program(
    source: &str,
    wrapfn: &str,
    propbase: &str,
) -> Option<String> {
    if !source.is_ascii() { return None; }
    let bytes = source.as_bytes();
    let mut mode = Mode::Code;
    let mut escaped = false;
    let mut at = 0usize;
    let mut start = 0usize;
    let mut result = String::with_capacity(source.len());
    while at < bytes.len() {
        let ch = bytes[at];
        match mode {
            Mode::Code => {
                if ch == b'\'' { mode = Mode::Single; }
                else if ch == b'"' { mode = Mode::Double; }
                else if ch == b'/' && bytes.get(at + 1) == Some(&b'/') {
                    mode = Mode::LineComment; at += 1;
                } else if ch == b'/' && bytes.get(at + 1) == Some(&b'*') {
                    mode = Mode::BlockComment; at += 1;
                } else if ch == b';' {
                    result.push_str(&transform_statement(
                        &source[start..at + 1], wrapfn, propbase
                    )?);
                    start = at + 1;
                }
            }
            Mode::Single | Mode::Double => {
                if escaped { escaped = false; }
                else if ch == b'\\' { escaped = true; }
                else if (mode == Mode::Single && ch == b'\'')
                    || (mode == Mode::Double && ch == b'"') {
                    mode = Mode::Code;
                } else if ch == b'\n' || ch == b'\r' {
                    return None; // unterminated ordinary string
                }
            }
            Mode::LineComment => {
                if ch == b'\n' || ch == b'\r' { mode = Mode::Code; }
            }
            Mode::BlockComment => {
                if ch == b'*' && bytes.get(at + 1) == Some(&b'/') {
                    mode = Mode::Code; at += 1;
                }
            }
        }
        at += 1;
    }
    if !matches!(mode, Mode::Code | Mode::LineComment) { return None; }
    result.push_str(&transform_statement(&source[start..], wrapfn, propbase)?);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_static_statements() {
        assert_eq!(
            rewrite_program("'use strict'; window.location; top.location;", "$w", "$p$"),
            Some("'use strict'; window.$p$location; $w(top).$p$location;".into())
        );
        assert_eq!(rewrite_program("a; b; this.location", "$w", "$p$"),
            Some("a; b; this.$p$location".into()));
        assert_eq!(rewrite_program("\";\"; top;", "$w", "$p$"),
            Some("\";\"; $w(top);".into()));
    }
    #[test]
    fn whole_file_fallback() {
        for source in [
            "window.location; let window = custom;",
            "foo(); top;",
            "obj['location'];",
            "const x = 1;",
            "foo// comment\n.location;",
            "x = location;",
            "a; function f() { return top; }",
            "'incomplete",
            "/* incomplete",
        ] {
            assert_eq!(rewrite_program(source, "$w", "$p$"), None, "{source:?}");
        }
    }
}
