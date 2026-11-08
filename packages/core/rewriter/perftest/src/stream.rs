//! Stateful, one-pass, chunk-invariant source rewriter for the supported subset.
//!
//! The scanner carries lexical state between arbitrary chunk boundaries.
//! Statements are rewritten when complete but held until EOF, because later
//! unsupported syntax forces an all-or-nothing OXC fallback. Nothing escapes
//! to the browser until the whole file has passed the restricted grammar.
use crate::{gate::identity_safe, simple};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode { Code, Slash, Single, Double, LineComment, BlockComment, BlockStar }

pub struct StreamingProgram {
    wrapfn: String,
    propertybase: String,
    mode: Mode,
    escaped: bool,
    raw_len: usize,
    segment: String,
    completed: String,
    unsupported: bool,
}

impl StreamingProgram {
    pub fn new(wrapfn: impl Into<String>, propertybase: impl Into<String>) -> Self {
        Self {
            wrapfn: wrapfn.into(), propertybase: propertybase.into(),
            mode: Mode::Code, escaped: false,
            raw_len: 0, segment: String::new(),
            completed: String::new(), unsupported: false,
        }
    }
    pub fn bytes_seen(&self) -> usize { self.raw_len }
    pub fn buffered_bytes(&self) -> usize { self.segment.len() + self.completed.len() }
    pub fn is_unsupported(&self) -> bool { self.unsupported }

    fn complete_statement(&mut self) {
        if self.unsupported { return; }
        let transformed = if identity_safe(&self.segment) {
            Some(self.segment.clone())
        } else {
            simple::rewrite(&self.segment, &self.wrapfn, &self.propertybase)
        };
        match transformed {
            Some(value) => self.completed.push_str(&value),
            None => self.unsupported = true,
        }
        self.segment.clear();
    }

    /// A fragment may end in the middle of a string/comment or slash marker.
    /// For UTF-8 input split by arbitrary bytes, decode with TextDecoder
    /// {stream:true} before calling this API.
    pub fn write(&mut self, fragment: &str) {
        self.raw_len += fragment.len();
        if self.unsupported { return; }
        for ch in fragment.chars() {
            if !ch.is_ascii() {
                self.unsupported = true;
                return;
            }
            self.segment.push(ch);
            match self.mode {
                Mode::Code => match ch {
                    '\'' => self.mode = Mode::Single,
                    '"' => self.mode = Mode::Double,
                    '/' => self.mode = Mode::Slash,
                    ';' => self.complete_statement(),
                    _ => {}
                },
                Mode::Slash => match ch {
                    '/' => self.mode = Mode::LineComment,
                    '*' => self.mode = Mode::BlockComment,
                    _ => {
                        // Regexp/division is context sensitive. Don't guess.
                        self.unsupported = true;
                        return;
                    }
                },
                Mode::Single | Mode::Double => {
                    if self.escaped { self.escaped = false; }
                    else if ch == '\\' { self.escaped = true; }
                    else if (self.mode == Mode::Single && ch == '\'')
                        || (self.mode == Mode::Double && ch == '"') {
                        self.mode = Mode::Code;
                    } else if ch == '\n' || ch == '\r' {
                        self.unsupported = true;
                        return;
                    }
                }
                Mode::LineComment => {
                    if ch == '\n' || ch == '\r' { self.mode = Mode::Code; }
                }
                Mode::BlockComment => {
                    if ch == '*' { self.mode = Mode::BlockStar; }
                }
                Mode::BlockStar => {
                    if ch == '/' { self.mode = Mode::Code; }
                    else if ch != '*' { self.mode = Mode::BlockComment; }
                }
            }
        }
    }

    /// None signals that the entire original file must use OXC.
    pub fn finish(mut self) -> Option<String> {
        if self.unsupported || !matches!(self.mode, Mode::Code | Mode::LineComment) {
            return None;
        }
        self.complete_statement();
        if self.unsupported { None }
        else { Some(self.completed) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan_chunks(source: &str, width: usize) -> Option<String> {
        let mut stream = StreamingProgram::new("$wrap", "$prop$");
        for chunk in source.as_bytes().chunks(width) {
            stream.write(std::str::from_utf8(chunk).unwrap());
        }
        stream.finish()
    }
    #[test]
    fn chunk_invariance() {
        for src in [
            "'use strict';window.location;top.location;",
            "\";\"; top;",
            "'escaped \\' text';window.location;",
            "window.location; let window = custom;",
            "/* hi */ ; this.location;",
            "// comment\n;\n top;",
            "window. \n location;",
            "window[location]",
        ] {
            let expected = scan_chunks(src, src.len());
            for width in 1..=src.len() {
                assert_eq!(scan_chunks(src, width), expected, "source={src:?} width={width}");
            }
        }
    }
    #[test]
    fn falls_back_on_late_unsupported() {
        let mut stream = StreamingProgram::new("$wrap", "$prop$");
        stream.write("window.location;");
        assert!(!stream.is_unsupported());
        stream.write(" let window = 1;");
        assert!(stream.finish().is_none());
    }
}
