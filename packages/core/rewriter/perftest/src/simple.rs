//! No-AST fast path for a single static-member expression statement.
//! This parser intentionally recognizes a tiny JavaScript subset with exact
//! source spans; everything else falls back to the OXC implementation.
//! A candidate is safe only when the source consists solely of an identifier
//! (or "this") followed by zero or more dot+identifier properties, with
//! optional trailing semicolon. No calls, imports, assignments, computed
//! members, optional chaining, comments, regexps, or complex statements.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Name<'a> { start: usize, end: usize, value: &'a str }

fn is_id_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}
fn is_id_continue(c: u8) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}
fn ws(c: u8) -> bool { matches!(c, b' ' | b'\t' | b'\r' | b'\n') }
fn skip_ws(input: &[u8], at: &mut usize) {
    while *at < input.len() && ws(input[*at]) { *at += 1; }
}
fn name<'a>(s: &'a str, at: &mut usize) -> Option<Name<'a>> {
    let bytes = s.as_bytes();
    if !is_id_start(*bytes.get(*at)?) { return None; }
    let start = *at;
    *at += 1;
    while *at < bytes.len() && is_id_continue(bytes[*at]) { *at += 1; }
    Some(Name { start, end: *at, value: &s[start..*at] })
}
fn unsafe_property(s: &str) -> bool {
    matches!(s, "location" | "parent" | "top" | "eval")
}
fn invalid_root(s: &str) -> bool {
    matches!(s, "new" | "delete" | "void" | "typeof" | "return" |
        "throw" | "await" | "yield" | "function" | "class" |
        "let" | "const" | "var" | "import" | "export" | "default" |
        "async" | "debugger" | "try" | "catch" | "finally" |
        "if" | "else" | "while" | "for" | "switch" | "case" |
        "break" | "continue" | "do" | "with" | "in" | "of" |
        "instanceof" | "super" | "null" | "true" | "false")
}

/// Return an AST-free rewrite if the input is a safe, single member
/// expression statement. None means OXC must handle the original source.
/// This deliberately follows the current Slopjet visitor's behavior,
/// including wrapping unsafe identifier references even if shadowed.
pub fn rewrite(
    source: &str,
    wrapfn: &str,
    wrappropertybase: &str,
) -> Option<String> {
    if !source.is_ascii() { return None; }
    let bytes = source.as_bytes();
    let mut pos = 0;
    skip_ws(bytes, &mut pos);
    let root = name(source, &mut pos)?;
    if invalid_root(root.value) { return None; }
    let mut chain = Vec::new();
    loop {
        skip_ws(bytes, &mut pos);
        if bytes.get(pos) != Some(&b'.') { break; }
        pos += 1;
        skip_ws(bytes, &mut pos);
        let prop = name(source, &mut pos)?;
        // postMessage has special object wrapping; leave it to OXC.
        if prop.value == "postMessage" { return None; }
        chain.push(prop);
    }
    skip_ws(bytes, &mut pos);
    if bytes.get(pos) == Some(&b';') {
        pos += 1;
        skip_ws(bytes, &mut pos);
    }
    if pos != source.len() { return None; }

    let mut out = String::with_capacity(source.len() + 32);
    let mut cursor = 0;
    if unsafe_property(root.value) {
        out.push_str(&source[..root.start]);
        out.push_str(wrapfn);
        out.push('(');
        out.push_str(root.value);
        out.push(')');
        cursor = root.end;
    }
    for prop in chain {
        if unsafe_property(prop.value) {
            out.push_str(&source[cursor..prop.start]);
            out.push_str(wrappropertybase);
            out.push_str(prop.value);
            cursor = prop.end;
        }
    }
    out.push_str(&source[cursor..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_rewriting() {
        assert_eq!(rewrite(" window.location.href; ", "$wrap", "$prop$"),
            Some(" window.$prop$location.href; ".into()));
        assert_eq!(rewrite("top.location;", "$wrap", "$prop$"),
            Some("$wrap(top).$prop$location;".into()));
        assert_eq!(rewrite("parent . top . eval;", "$wrap", "$prop$"),
            Some("$wrap(parent) . $prop$top . $prop$eval;".into()));
        assert_eq!(rewrite("this.location", "$wrap", "$prop$"),
            Some("this.$prop$location".into()));
        assert_eq!(rewrite("other.property", "$wrap", "$prop$"),
            Some("other.property".into()));
    }
    #[test]
    fn dangerous_complex_forms_fall_back() {
        for src in [
            "window[location]", "obj.postMessage", "location = 1",
            "const location = 1", "function f(location) {return location;}",
            "new top()", "eval('x')", "import.meta",
            "foo?.location", "window.location + 1",
            "a; b;", "foo/*x*/.location", "window.\\u006cocation",
            "await location", "location++",
        ] {
            assert_eq!(rewrite(src, "$wrap", "$prop$"), None, "{src:?}");
        }
    }
}
