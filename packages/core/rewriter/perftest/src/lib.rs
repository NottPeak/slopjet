//! Incremental scanner experiment. Not yet a rewrite engine.
//! This stage separates lexical spans; ambiguous syntax is never rewritten.
pub mod deferred;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind { Identifier, Space, String, Comment, Punctuation }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span { pub kind: Kind, pub start: usize, pub end: usize, pub source: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode { Code, Ident, Space, Quote(char), Slash, LineComment, BlockComment }
pub struct Scanner {
    mode: Mode, offset: usize, start: usize, buf: String,
    escaped: bool, previous_star: bool, unsafe_syntax: Vec<usize>,
}
impl Default for Scanner {
    fn default() -> Self {
        Self { mode: Mode::Code, offset: 0, start: 0, buf: String::new(),
            escaped: false, previous_star: false, unsafe_syntax: Vec::new() }
    }
}
impl Scanner {
    pub fn new() -> Self { Self::default() }
    pub fn unsafe_syntax_offsets(&self) -> &[usize] { &self.unsafe_syntax }
    fn take(&mut self, ch: char) { self.buf.push(ch); self.offset += ch.len_utf8(); }
    fn begin(&mut self, mode: Mode) { self.mode = mode; self.start = self.offset; }
    fn emit(&mut self, result: &mut Vec<Span>, kind: Kind) {
        if self.buf.is_empty() { return; }
        result.push(Span { kind, start: self.start, end: self.offset,
            source: std::mem::take(&mut self.buf) });
        self.start = self.offset;
    }
    pub fn write(&mut self, fragment: &str) -> Vec<Span> {
        let mut result = Vec::new();
        for ch in fragment.chars() {
            let mut retry = true;
            while retry {
                retry = false;
                match self.mode {
                    Mode::Code => {
                        if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
                            self.begin(Mode::Ident); self.take(ch);
                        } else if ch.is_ascii_whitespace() {
                            self.begin(Mode::Space); self.take(ch);
                        } else if ch == '\'' || ch == '"' {
                            self.begin(Mode::Quote(ch)); self.take(ch);
                        } else if ch == '/' {
                            self.begin(Mode::Slash); self.take(ch);
                        } else {
                            // Template literals, regexp and non-ASCII identifiers require
                            // additional grammar; mark unsafe rather than mis-rewrite.
                            if ch == '\u{60}' || !ch.is_ascii() {
                                self.unsafe_syntax.push(self.offset);
                            }
                            self.begin(Mode::Code); self.take(ch);
                            self.emit(&mut result, Kind::Punctuation);
                        }
                    }
                    Mode::Ident => {
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' { self.take(ch); }
                        else { self.emit(&mut result, Kind::Identifier);
                            self.mode = Mode::Code; retry = true; }
                    }
                    Mode::Space => {
                        if ch.is_ascii_whitespace() { self.take(ch); }
                        else { self.emit(&mut result, Kind::Space);
                            self.mode = Mode::Code; retry = true; }
                    }
                    Mode::Quote(delim) => {
                        self.take(ch);
                        if self.escaped { self.escaped = false; }
                        else if ch == '\\' { self.escaped = true; }
                        else if ch == delim {
                            self.emit(&mut result, Kind::String); self.mode = Mode::Code;
                        }
                    }
                    Mode::Slash => {
                        if ch == '/' { self.take(ch); self.mode = Mode::LineComment; }
                        else if ch == '*' { self.take(ch); self.mode = Mode::BlockComment; }
                        else {
                            self.unsafe_syntax.push(self.start);
                            self.emit(&mut result, Kind::Punctuation);
                            self.mode = Mode::Code; retry = true;
                        }
                    }
                    Mode::LineComment => {
                        if ch == '\n' || ch == '\r' {
                            self.emit(&mut result, Kind::Comment);
                            self.mode = Mode::Code; retry = true;
                        } else { self.take(ch); }
                    }
                    Mode::BlockComment => {
                        self.take(ch);
                        if self.previous_star && ch == '/' {
                            self.emit(&mut result, Kind::Comment);
                            self.mode = Mode::Code; self.previous_star = false;
                        } else { self.previous_star = ch == '*'; }
                    }
                }
            }
        }
        result
    }
    pub fn finish(&mut self) -> Result<Vec<Span>, String> {
        let mut result = Vec::new();
        match self.mode {
            Mode::Code => {},
            Mode::Ident => self.emit(&mut result, Kind::Identifier),
            Mode::Space => self.emit(&mut result, Kind::Space),
            Mode::LineComment => self.emit(&mut result, Kind::Comment),
            Mode::Slash => {
                self.unsafe_syntax.push(self.start);
                self.emit(&mut result, Kind::Punctuation);
            }
            _ => return Err(format!("unterminated lexical unit at byte {}", self.start)),
        }
        self.mode = Mode::Code;
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scan(s: &str, chunk: usize) -> (Vec<Span>, Vec<usize>) {
        let mut scanner = Scanner::new();
        let mut result = Vec::new();
        for piece in s.as_bytes().chunks(chunk) {
            result.extend(scanner.write(std::str::from_utf8(piece).unwrap()));
        }
        result.extend(scanner.finish().unwrap());
        (result, scanner.unsafe_syntax_offsets().to_vec())
    }
    #[test]
    fn partition_independent_ascii() {
        for source in [
            "window.location.href = '/dashboard';",
            "function f(window) { return window.location; }",
            "const a = 'escape \\' quote'; // comment\nfetch('/a');",
            "/* block comment */ const name = 123;",
        ] {
            let baseline = scan(source, source.len());
            for size in 1..=source.len() {
                assert_eq!(scan(source, size), baseline);
            }
            assert_eq!(baseline.0.iter().map(|x| x.source.as_str()).collect::<String>(), source);
        }
    }
    #[test]
    fn unterminated_rejected() {
        let mut scanner = Scanner::new();
        scanner.write("let x = 'bad");
        assert!(scanner.finish().is_err());
    }
}
