//! Token-span semantic rewriting without AST materialization.
//! A forward lexer retains state across network chunks. Nothing is released
//! until binding ambiguity and supported syntax checks have completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind { Word, Number, Literal, Punct }
#[derive(Clone, Copy, Debug)]
struct Token { start: usize, end: usize, kind: Kind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode { Code, Word, Number, Quote(u8), Slash, Regex, RegexFlags,
    LineComment, BlockComment }
#[derive(Default)]
struct Patch { before: String, replace: Option<String>, after: String }

fn ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' || b == b'$' }
fn ident_cont(b: u8) -> bool { ident_start(b) || b.is_ascii_digit() }
fn unsafe_name(s: &str) -> bool { matches!(s, "parent" | "top" | "location" | "eval") }
fn not_object(s: &str) -> bool {
    matches!(s, "return" | "throw" | "case" | "delete" | "void" |
        "typeof" | "new" | "await" | "yield" | "of" | "in" |
        "instanceof" | "else" | "do" | "function" | "class" |
        "const" | "let" | "var" | "export" | "import" | "extends")
}

pub struct SemanticProgram {
    source: String, tokens: Vec<Token>, cursor: usize, start: usize,
    mode: Mode, escaped: bool, in_class: bool, prev_star: bool,
    unsupported: bool, wrapfn: String, propertybase: String,
    propertyfn: String, disable_computed: bool,
}
impl SemanticProgram {
    pub fn new(wrapfn: impl Into<String>, propertybase: impl Into<String>,
        propertyfn: impl Into<String>, disable_computed: bool) -> Self {
        Self { source: String::new(), tokens: Vec::new(), cursor: 0, start: 0,
            mode: Mode::Code, escaped: false, in_class: false, prev_star: false,
            unsupported: false, wrapfn: wrapfn.into(), propertybase: propertybase.into(),
            propertyfn: propertyfn.into(), disable_computed }
    }
    fn emit(&mut self, kind: Kind) {
        self.tokens.push(Token { start: self.start, end: self.cursor, kind });
        self.mode = Mode::Code;
    }
    fn regex_possible(&self) -> bool {
        let Some(t) = self.tokens.last() else { return true; };
        let s = &self.source[t.start..t.end];
        match t.kind {
            Kind::Word => matches!(s, "return" | "throw" | "case" |
                "delete" | "void" | "typeof" | "yield" | "await" |
                "of" | "in" | "instanceof" | "else"),
            Kind::Punct => matches!(s, "=" | ":" | "," | ";" | "(" |
                "[" | "{" | "!" | "?" | "&" | "|" | "~" | "+" | "-" | "*"),
            _ => false,
        }
    }
    pub fn write(&mut self, chunk: &str) {
        self.source.push_str(chunk);
        if self.unsupported { return; }
        while self.cursor < self.source.len() && !self.unsupported {
            let b = self.source.as_bytes()[self.cursor];
            match self.mode {
                Mode::Code => {
                    if b.is_ascii_whitespace() { self.cursor += 1; }
                    else if ident_start(b) {
                        self.start = self.cursor; self.cursor += 1; self.mode = Mode::Word;
                    } else if b.is_ascii_digit() {
                        self.start = self.cursor; self.cursor += 1; self.mode = Mode::Number;
                    } else if b == b'\'' || b == b'"' {
                        self.start = self.cursor; self.cursor += 1;
                        self.mode = Mode::Quote(b); self.escaped = false;
                    } else if b == b'/' {
                        self.start = self.cursor; self.cursor += 1; self.mode = Mode::Slash;
                    } else if b.is_ascii_graphic() && b != 96 && b != b'\\' {
                        self.start = self.cursor; self.cursor += 1; self.emit(Kind::Punct);
                    } else { self.unsupported = true; }
                }
                Mode::Word => {
                    if ident_cont(b) { self.cursor += 1; }
                    else { self.emit(Kind::Word); }
                }
                Mode::Number => {
                    if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' {
                        self.cursor += 1;
                    } else { self.emit(Kind::Number); }
                }
                Mode::Quote(quote) => {
                    self.cursor += 1;
                    if self.escaped { self.escaped = false; }
                    else if b == b'\\' { self.escaped = true; }
                    else if b == quote { self.emit(Kind::Literal); }
                    else if b == b'\r' || b == b'\n' { self.unsupported = true; }
                }
                Mode::Slash => {
                    if b == b'/' { self.cursor += 1; self.mode = Mode::LineComment; }
                    else if b == b'*' {
                        self.cursor += 1; self.mode = Mode::BlockComment; self.prev_star = false;
                    } else if self.regex_possible() {
                        self.mode = Mode::Regex; self.escaped = false; self.in_class = false;
                    } else if self.tokens.last().is_some_and(|t|
                        matches!(&self.source[t.start..t.end], ")" | "}")
                    ) {
                        // Slash after a closing control condition may begin
                        // a regexp statement, but after a call it is division.
                        // Without grammar context, reject rather than corrupt.
                        self.unsupported = true;
                    } else { self.emit(Kind::Punct); }
                }
                Mode::Regex => {
                    self.cursor += 1;
                    if self.escaped { self.escaped = false; }
                    else if b == b'\\' { self.escaped = true; }
                    else if b == b'[' { self.in_class = true; }
                    else if b == b']' { self.in_class = false; }
                    else if b == b'/' && !self.in_class { self.mode = Mode::RegexFlags; }
                    else if b == b'\r' || b == b'\n' { self.unsupported = true; }
                }
                Mode::RegexFlags => {
                    if ident_cont(b) { self.cursor += 1; }
                    else { self.emit(Kind::Literal); }
                }
                Mode::LineComment => {
                    self.cursor += 1;
                    if b == b'\n' || b == b'\r' { self.mode = Mode::Code; }
                }
                Mode::BlockComment => {
                    self.cursor += 1;
                    if self.prev_star && b == b'/' {
                        self.mode = Mode::Code; self.prev_star = false;
                    } else { self.prev_star = b == b'*'; }
                }
            }
        }
    }
    fn close(&mut self) -> bool {
        if self.unsupported { return false; }
        match self.mode {
            Mode::Code | Mode::LineComment => {}
            Mode::Word => self.emit(Kind::Word),
            Mode::Number => self.emit(Kind::Number),
            Mode::RegexFlags => self.emit(Kind::Literal),
            Mode::Slash => self.emit(Kind::Punct),
            _ => return false,
        }
        true
    }
    fn word(&self, i: usize) -> &str {
        let t = self.tokens[i];
        &self.source[t.start..t.end]
    }
    fn is(&self, i: usize, s: &str) -> bool {
        self.tokens.get(i).is_some_and(|_| self.word(i) == s)
    }
    fn binding_safe(&self, pairs: &[Option<usize>]) -> bool {
        let n = self.tokens.len();
        for i in 0..n {
            if self.tokens[i].kind != Kind::Word { continue; }
            let w = self.word(i);
            if matches!(w, "import" | "export" | "with" | "class") { return false; }
            if matches!(w, "var" | "let" | "const" | "class") {
                if i+1 < n && unsafe_name(self.word(i+1)) { return false; }
                if self.is(i+1, "{") || self.is(i+1, "[") {
                    let Some(end) = pairs[i+1] else { return false };
                    if (i+2..end).any(|j| unsafe_name(self.word(j))) { return false; }
                }
            }
            if matches!(w, "function" | "catch") {
                let mut j = i+1;
                while j < n && j < i+6 && !self.is(j, "(") && !self.is(j, "{") {
                    j += 1;
                }
                if self.is(j, "(") {
                    let Some(end) = pairs[j] else { return false };
                    if (j+1..end).any(|k| unsafe_name(self.word(k))) { return false; }
                }
            }
            if unsafe_name(w) && self.is(i+1, "=") && self.is(i+2, ">") {
                return false;
            }
        }
        for i in 1..n {
            if self.is(i, "=") && self.is(i+1, ">") && self.is(i-1, ")") {
                let Some(start) = pairs[i-1] else { return false };
                if (start+1..i-1).any(|k| unsafe_name(self.word(k))) { return false; }
            }
        }
        true
    }
    pub fn finish(mut self) -> Option<String> {
        if !self.close() { return None; }
        let n = self.tokens.len();
        let mut pairs = vec![None; n];
        let mut stack = Vec::new();
        for i in 0..n {
            if matches!(self.word(i), "(" | "[" | "{") {
                stack.push(i);
            } else if matches!(self.word(i), ")" | "]" | "}") {
                let j = stack.pop()?;
                if !matches!((self.word(j),self.word(i)),
                    ("(",")") | ("[","]") | ("{","}")) { return None; }
                pairs[j] = Some(i);
                pairs[i] = Some(j);
            }
        }
        if !stack.is_empty() || !self.binding_safe(&pairs) { return None; }
        let mut patches: Vec<Patch> = (0..n).map(|_| Patch::default()).collect();
        for i in 0..n {
            if self.tokens[i].kind != Kind::Word { continue; }
            let w = self.word(i);
            if w == "debugger" || w == "postMessage" && i>0 && self.is(i-1, ".") {
                return None;
            }
            if !unsafe_name(w) { continue; }
            if i>0 && self.is(i-1, ".") {
                patches[i].replace = Some(format!("{}{}", self.propertybase, w));
                continue;
            }
            if self.is(i+1, ":") && i>0 && (self.is(i-1, "{") || self.is(i-1, ",")) {
                patches[i].replace = Some(format!("{}{}", self.propertybase, w));
                continue;
            }
            if i>0 && self.is(i-1, "#") { continue; }
            if i>0 && matches!(self.word(i-1),
                "var" | "let" | "const" | "function" | "class" | "new") {
                return None;
            }
            if (self.is(i+1, "=") && !self.is(i+2, "=") && !self.is(i+2, ">")) ||
                (i > 1 && (
                    self.is(i-1, "+") && self.is(i-2, "+") ||
                    self.is(i-1, "-") && self.is(i-2, "-") ||
                    self.is(i-1, "delete")
                )) {
                return None;
            }
            // Any compound assignment or update is an unsafe binding target.
            for op in ["+", "-", "*", "/", "%", "|", "&", "^", "<", ">", "?"] {
                if self.is(i+1, op) && (
                    self.is(i+2, "=") || self.is(i+2, op) &&
                    (self.is(i+3, "=") || op == "+" || op == "-")
                ) { return None; }
            }
            if w == "eval" && self.is(i+1, "(") { return None; }
            if self.is(i+1, ":") { return None; }
            if i>0 && (self.is(i-1, "{") || self.is(i-1, ",")) &&
                (self.is(i+1, "}") || self.is(i+1, ",")) { return None; }
            patches[i].replace = Some(format!("{}({})", self.wrapfn, w));
        }
        if !self.disable_computed {
            for i in 0..n {
                if !self.is(i, "[") { continue; }
                let end = pairs[i]?;
                if end <= i+1 { continue; }
                let member = i>0 && (
                    self.tokens[i-1].kind == Kind::Word && !not_object(self.word(i-1)) ||
                    self.is(i-1, ")") || self.is(i-1, "]") || self.is(i-1, ".")
                );
                if !member { continue; }
                let numeric = i+2 == end && self.tokens[i+1].kind == Kind::Number;
                let text = self.word(i+1);
                // Escaped spelling can resolve to a protected property at
                // runtime (e.g. a Unicode escape spelling of 'location').
                if i+2 == end && self.tokens[i+1].kind == Kind::Literal &&
                    text.contains('\\') { return None; }
                let plain_string = i+2 == end && self.tokens[i+1].kind == Kind::Literal &&
                    !unsafe_name(text.trim_matches(&['"', '\''][..]));
                if numeric || plain_string { continue; }
                patches[i+1].before.insert_str(0, &format!("{}((", self.propertyfn));
                patches[end-1].after.push_str("))");
            }
        }
        let mut out = String::with_capacity(self.source.len()+64);
        let mut at = 0;
        for (i,t) in self.tokens.iter().enumerate() {
            out.push_str(&self.source[at..t.start]);
            out.push_str(&patches[i].before);
            if let Some(rep) = &patches[i].replace { out.push_str(rep); }
            else { out.push_str(&self.source[t.start..t.end]); }
            out.push_str(&patches[i].after);
            at = t.end;
        }
        out.push_str(&self.source[at..]);
        Some(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rewrite(s: &str, size: usize) -> Option<String> {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$computed", false);
        for part in s.as_bytes().chunks(size.max(1)) {
            engine.write(std::str::from_utf8(part).unwrap());
        }
        engine.finish()
    }
    #[test]
    fn common_expressions() {
        assert_eq!(rewrite("const x = window.location.href;", 200),
            Some("const x = window.$prop$location.href;".into()));
        assert_eq!(rewrite("function f(x){return x.location + top.location;}", 200),
            Some("function f(x){return x.$prop$location + $wrap(top).$prop$location;}".into()));
        assert_eq!(rewrite("let x = obj[location];", 200),
            Some("let x = obj[$computed(($wrap(location)))];".into()));
        assert_eq!(rewrite("const x = /location/gi; // location", 200),
            Some("const x = /location/gi; // location".into()));
        assert_eq!(rewrite("let x = 10 / 2;", 200), Some("let x = 10 / 2;".into()));
    }
    #[test]
    fn partition_invariance() {
        for source in [
            "function f(x){return x.location + parent.top;}",
            "let x = obj[location]; const r = /location/gi;",
            "const s = 'escaped \\' quote'; window.location;",
            "const x = location; let location = 1;",
            "const y = /*comment*/ window.location;",
        ] {
            let baseline = rewrite(source, source.len());
            for step in 1..=source.len() {
                assert_eq!(rewrite(source,step),baseline,"{source:?}/{step}");
            }
        }
    }
    #[test]
    fn fails_closed() {
        for s in [
            "const location = 1;", "function f(location){return location}",
            "location = x;", "eval('x')", "import('./x.js')",
            "with(x){location}", "obj.postMessage('x')", "const {location} = x",
        ] { assert!(rewrite(s,s.len()).is_none(),"{s:?}"); }
    }
}
