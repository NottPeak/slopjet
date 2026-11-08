//! Narrow fast path: safely skip OXC only for inert scripts.
//! Unsupported constructs continue to OXC.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Code, Single, Double, Line, Block }
pub fn identity_safe(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut mode = Mode::Code;
    let mut escaped = false;
    let mut require_semicolon = false;
    while i < bytes.len() {
        let ch = bytes[i];
        match mode {
            Mode::Code => {
                if ch.is_ascii_whitespace() { i += 1; continue; }
                if ch == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    mode = Mode::Line; i += 2; continue;
                }
                if ch == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    mode = Mode::Block; i += 2; continue;
                }
                if ch == b';' { require_semicolon = false; i += 1; continue; }
                if require_semicolon { return false; }
                if ch == b'\'' { mode = Mode::Single; require_semicolon = true; i += 1; continue; }
                if ch == b'"' { mode = Mode::Double; require_semicolon = true; i += 1; continue; }
                return false;
            }
            Mode::Single | Mode::Double => {
                if escaped { escaped = false; i += 1; continue; }
                if ch == b'\\' { escaped = true; i += 1; continue; }
                if (mode == Mode::Single && ch == b'\'') || (mode == Mode::Double && ch == b'"') {
                    mode = Mode::Code;
                } else if ch == b'\n' || ch == b'\r' { return false; }
                i += 1;
            }
            Mode::Line => {
                if ch == b'\n' || ch == b'\r' { mode = Mode::Code; }
                i += 1;
            }
            Mode::Block => {
                if ch == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    mode = Mode::Code; i += 2;
                } else { i += 1; }
            }
        }
    }
    matches!(mode, Mode::Code | Mode::Line)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inert() {
        for s in ["", " ; ; ", "/*comment*/", "// hi", "'use strict';", "\"value\";", "'x'; /*comment*/ \"y\";"] {
            assert!(identity_safe(s), "{s:?}");
        }
    }
    #[test]
    fn fallback() {
        for s in ["window.location;", "new A()", "const x = 1;", "eval('x')", "a[b]", "import('x')",
            "function f() {}", "postMessage(x)", "1+2", "'unterminated", "/* unterminated", "'a' + 'b'"] {
            assert!(!identity_safe(s), "{s:?}");
        }
    }
}
