//! Token-span semantic rewriting without AST materialization.
//! A forward lexer retains state across network chunks. Nothing is released
//! until binding ambiguity and supported syntax checks have completed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind { Word, Number, Literal, Punct, InterpolationOpen }
#[derive(Clone, Copy, Debug)]
struct Token { start: usize, end: usize, kind: Kind }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode { Code, Word, Number, Quote(u8), Template, Slash, Regex, RegexFlags,
    LineComment, BlockComment }
#[derive(Clone, Copy, Debug)]
struct TemplateFrame { depth: usize, in_expression: bool }
#[derive(Default)]
struct Patch { before: String, replace: Option<String>, after: String }
fn patch(slots: &mut [Option<Patch>], index: usize) -> &mut Patch {
    slots[index].get_or_insert_with(Patch::default)
}

fn ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' || b == b'$' }
fn ident_cont(b: u8) -> bool { ident_start(b) || b.is_ascii_digit() }
fn unicode_start(c: char) -> bool { c.is_alphabetic() || c == '_' || c == '$' }
fn unicode_continue(c: char) -> bool {
    unicode_start(c) || c.is_numeric() || c == '\u{200c}' || c == '\u{200d}'
}
fn unsafe_name(s: &str) -> bool { matches!(s, "parent" | "top" | "location" | "eval") }
fn not_object(s: &str) -> bool {
    matches!(s, "return" | "throw" | "case" | "delete" | "void" |
        "typeof" | "new" | "await" | "yield" | "of" | "in" |
        "instanceof" | "else" | "do" | "function" | "class" |
        "const" | "let" | "var" | "export" | "import" | "extends")
}


fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len()+2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub struct SemanticProgram {
    source: String, tokens: Vec<Token>, cursor: usize, start: usize,
    mode: Mode, escaped: bool, in_class: bool, prev_star: bool,
    unsupported: bool, templates: Vec<TemplateFrame>,
    wrapfn: String, propertybase: String,
    propertyfn: String, disable_computed: bool,
    importfn: String, rewritefn: String, postmessagefn: String, base: String,
    metafn: String, is_module: bool, trysetfn: String,
}
impl SemanticProgram {
    pub fn new(wrapfn: impl Into<String>, propertybase: impl Into<String>,
        propertyfn: impl Into<String>, disable_computed: bool) -> Self {
        Self { source: String::new(), tokens: Vec::new(), cursor: 0, start: 0,
            mode: Mode::Code, escaped: false, in_class: false, prev_star: false,
            unsupported: false, templates: Vec::new(),
            wrapfn: wrapfn.into(), propertybase: propertybase.into(),
            propertyfn: propertyfn.into(), disable_computed,
            importfn: String::new(), rewritefn: String::new(),
            postmessagefn: String::new(), base: String::new(),
            metafn: String::new(), is_module: false, trysetfn: String::new() }
    }
    pub fn with_helpers(
        mut self, importfn: impl Into<String>, rewritefn: impl Into<String>,
        postmessagefn: impl Into<String>, base: impl Into<String>,
    ) -> Self {
        self.importfn = importfn.into();
        self.rewritefn = rewritefn.into();
        self.postmessagefn = postmessagefn.into();
        self.base = base.into();
        self
    }

    pub fn with_module_meta(mut self, metafn: impl Into<String>, is_module: bool) -> Self {
        self.metafn = metafn.into();
        self.is_module = is_module;
        self
    }

    pub fn with_assignment(mut self, trysetfn: impl Into<String>) -> Self {
        self.trysetfn = trysetfn.into();
        self
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
                "[" | "{" | "!" | "?" | "&" | "|" | "~" | "+" | "-" | "*" |
                "<" | ">" | "%" | "^"),
            _ => false,
        }
    }
    /// Disambiguate a slash following ')' without building an AST.
    /// After an ordinary call, it is division; after a control condition,
    /// it begins a regexp statement. Ambiguous syntax is still rejected.
    fn after_control_condition(&self) -> bool {
        let Some(last) = self.tokens.last() else { return false; };
        if &self.source[last.start..last.end] != ")" { return false; }
        let mut depth: usize = 0;
        for i in (0..self.tokens.len()).rev() {
            let tok = self.tokens[i];
            let word = &self.source[tok.start..tok.end];
            if word == ")" { depth += 1; }
            else if word == "(" {
                if depth == 0 { return false; }
                depth -= 1;
                if depth == 0 {
                    if i == 0 { return false; }
                    let prior = self.tokens[i-1];
                    return matches!(&self.source[prior.start..prior.end],
                        "if" | "while" | "for" | "with" | "switch");
                }
            }
        }
        false
    }
    pub fn write(&mut self, chunk: &str) {
        if self.unsupported { return; }
        self.source.push_str(chunk);
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
                    } else if b == 96 {
                        self.cursor += 1;
                        self.templates.push(TemplateFrame { depth: 0, in_expression: false });
                        self.mode = Mode::Template;
                        self.escaped = false;
                    } else if b == b'{' && self.templates.last().is_some_and(|f| f.in_expression) {
                        if let Some(frame) = self.templates.last_mut() { frame.depth += 1; }
                        self.start = self.cursor; self.cursor += 1; self.emit(Kind::Punct);
                    } else if b == b'}' && self.templates.last().is_some_and(|f| f.in_expression) {
                        let ends_interpolation = self.templates.last().is_some_and(|f| f.depth == 0);
                        if !ends_interpolation {
                            if let Some(frame) = self.templates.last_mut() { frame.depth -= 1; }
                        }
                        self.start = self.cursor; self.cursor += 1; self.emit(Kind::Punct);
                        if ends_interpolation {
                            if let Some(frame) = self.templates.last_mut() { frame.in_expression = false; }
                            self.mode = Mode::Template;
                        }
                    } else if b == 0xef &&
                        self.source[self.cursor..].starts_with('\u{feff}') {
                        self.cursor += '\u{feff}'.len_utf8();
                    } else if b >= 0x80 {
                        let ch = self.source[self.cursor..].chars().next().unwrap();
                        if ch.is_whitespace() {
                            self.cursor += ch.len_utf8();
                        } else if unicode_start(ch) {
                            self.start = self.cursor;
                            self.cursor += ch.len_utf8();
                            self.mode = Mode::Word;
                        } else { self.unsupported = true; }
                    } else if b.is_ascii_graphic() && b != 96 && b != b'\\' {
                        self.start = self.cursor; self.cursor += 1; self.emit(Kind::Punct);
                    } else { self.unsupported = true; }
                }
                Mode::Word => {
                    if ident_cont(b) {
                        self.cursor += 1;
                    } else if b >= 0x80 {
                        let ch = self.source[self.cursor..].chars().next().unwrap();
                        if unicode_continue(ch) { self.cursor += ch.len_utf8(); }
                        else { self.emit(Kind::Word); }
                    } else { self.emit(Kind::Word); }
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
                Mode::Template => {
                    let next = self.source.as_bytes().get(self.cursor+1).copied();
                    if self.escaped {
                        self.escaped = false;
                        self.cursor += 1;
                    } else if b == b'\\' {
                        self.escaped = true;
                        self.cursor += 1;
                    } else if b == 96 {
                        self.cursor += 1;
                        self.templates.pop();
                        self.mode = Mode::Code;
                    } else if b == b'$' && next.is_none() {
                        break;
                    } else if b == b'$' && next == Some(b'{') {
                        self.cursor += 1;
                        self.start = self.cursor;
                        self.cursor += 1;
                        self.emit(Kind::InterpolationOpen);
                        if let Some(frame) = self.templates.last_mut() {
                            frame.in_expression = true;
                            frame.depth = 0;
                        }
                    } else { self.cursor += 1; }
                }
                Mode::Slash => {
                    if b == b'/' { self.cursor += 1; self.mode = Mode::LineComment; }
                    else if b == b'*' {
                        self.cursor += 1; self.mode = Mode::BlockComment; self.prev_star = false;
                    } else if self.regex_possible() {
                        self.mode = Mode::Regex; self.escaped = false; self.in_class = false;
                    } else if self.after_control_condition() {
                        self.mode = Mode::Regex;
                        self.escaped = false;
                        self.in_class = false;
                    } else if self.tokens.last().is_some_and(|t|
                        &self.source[t.start..t.end] == "}"
                    ) {
                        // Braces can close an object expression or statement.
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
        if self.unsupported || !self.templates.is_empty() { return false; }
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
    fn grammar_safe(&self) -> bool {
        let n = self.tokens.len();
        for i in 0..n {
            if self.tokens[i].kind != Kind::Word { continue; }
            let w = self.word(i);
            if w == "with" && (i == 0 || !self.is(i-1, ".")) &&
                !(self.is(i+1, ":") && i>0 && self.is(i-1, "{")) {
                return false;
            }
            // Classes with protected class names need class-environment
            // binding resolution and are conservatively rejected.
            if w == "class" && self.tokens.get(i+1).is_some_and(|_| unsafe_name(self.word(i+1))) {
                return false;
            }
            if w == "export" {
                if !self.is_module || !matches!(
                    self.tokens.get(i+1).map(|_| self.word(i+1)),
                    Some("const" | "let" | "var" | "function" | "default" | "async" | "{" | "*")
                ) { return false; }
            }
            if w == "import" && i>0 && self.is(i-1, ".") { continue; }
            if w == "import" && !(self.is(i+1, "(") ||
                self.is_module && (self.is(i+1, ".") && self.is(i+2, "meta")
                    && !self.metafn.is_empty() || !self.is(i+1, "."))) {
                return false;
            }
        }
        true
    }
    fn object_shorthand(&self, at: usize, pairs: &[Option<usize>]) -> bool {
        let Some(open) = (0..at).rev().find(|&i| {
            self.is(i, "{") && pairs[i].is_some_and(|end| end > at)
        }) else { return false };
        if open == 0 { return false; }
        // Distinguish expression objects from declarations, blocks,
        // class bodies and function bodies without materializing an AST.
        matches!(self.word(open-1),
            "=" | ":" | "(" | "[" | "," | "return" | "=>" )
    }
    pub fn finish(self) -> Option<String> {
        self.finish_with_url_rewriter(|_| None)
    }

    /// URL rewriting is provided by the host, preserving the proxy URL codec.
    pub fn finish_with_url_rewriter<F>(mut self, mut rewrite_url: F) -> Option<String>
    where F: FnMut(&str) -> Option<String> {
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
        if !stack.is_empty() || !self.grammar_safe() { return None; }
        let words: Vec<&str> = (0..n).map(|i| self.word(i)).collect();
        let is_word: Vec<bool> = self.tokens.iter().map(|t| t.kind == Kind::Word).collect();
        let scopes = crate::scopes::ScopeIndex::new(&words, &is_word, &pairs)?;
        // Destructuring declarations are safe to preserve as source text if
        // the pattern contains no protected bindings. ScopeIndex already
        // rejects unsafe binding names in declaration patterns. Destructuring
        // *assignments* need separate setter semantics and remain rejected.
        for i in 0..n {
            if self.is(i, "{") || self.is(i, "[") {
                let end = pairs[i]?;
                if self.is(end+1, "=") && !self.is(end+2, "=") {
                    let variable_binding = i > 0 && matches!(
                        self.word(i-1), "let" | "const" | "var"
                    );
                    if !variable_binding { return None; }
                }
            }
        }
        let mut patches: Vec<Option<Patch>> = (0..n).map(|_| None).collect();
        let mut module_clause = vec![false; n];
        // Handle static module declarations in the existing token stream.
        for i in 0..n {
            let word = self.word(i);
            let is_import = word == "import" &&
                (i==0 || !self.is(i-1, ".")) &&
                !self.is(i+1, "(") && !self.is(i+1, ".");
            let is_export_from = word == "export" &&
                (self.is(i+1, "{") || self.is(i+1, "*"));
            if !is_import && !is_export_from { continue; }
            if !self.is_module { return None; }
            let mut specifier = None;
            if is_import && self.tokens.get(i+1).is_some_and(|t| t.kind == Kind::Literal) {
                specifier = Some(i+1);
            } else {
                let mut j = i+1;
                while j < n && !self.is(j, ";") {
                    if self.is(j, "from") {
                        if j+1 >= n || self.tokens[j+1].kind != Kind::Literal {
                            return None;
                        }
                        specifier = Some(j+1);
                        break;
                    }
                    if matches!(self.word(j), "(" | "[" | "{") {
                        j = pairs[j]?+1;
                    } else { j += 1; }
                }
            }
            if is_export_from && specifier.is_none() {
                // A local export clause does not reference globals. Its
                // specifier names must remain syntactically unchanged.
                let close = if self.is(i+1, "{") { pairs[i+1]? } else {
                    return None; // export * requires "from" and a source
                };
                if (i+2..close).any(|j| self.tokens[j].kind == Kind::Word &&
                    unsafe_name(self.word(j))) { return None; }
                for index in i..=close { module_clause[index] = true; }
                continue;
            }
            let specifier_index = specifier?;
            if (i+1..specifier_index).any(|j|
                self.tokens[j].kind == Kind::Word && unsafe_name(self.word(j))
            ) { return None; }
            for index in i..specifier_index { module_clause[index] = true; }
            let value = self.word(specifier_index);
            let bytes = value.as_bytes();
            if bytes.len() < 2 || !matches!(bytes[0], b'\'' | b'"') ||
                bytes[bytes.len()-1] != bytes[0] { return None; }
            let spec = &value[1..value.len()-1];
            if spec.contains('\\') { return None; }
            if spec.starts_with('.') || spec.starts_with('/') || spec.contains(':') {
                let rewritten = rewrite_url(spec)?;
                patch(&mut patches, specifier_index).replace = Some(js_string(&rewritten));
            }
            if self.is(specifier_index+1, "with") || self.is(specifier_index+1, "assert") {
                return None;
            }
        }
        for i in 0..n {
            if self.tokens[i].kind != Kind::Word || module_clause[i] { continue; }
            let w = self.word(i);
            if w == "debugger" {
                // Remove the debugger statement without perturbing its
                // surrounding semicolon or comments. An identifier used as a
                // property key is not a debugger statement.
                if i>0 && self.is(i-1, ".") ||
                    self.is(i+1, ":") || self.is(i+1, "(") {
                    continue;
                }
                if i+1 == n || self.is(i+1, ";") || self.is(i+1, "}") {
                    patch(&mut patches, i).replace = Some(String::new());
                    continue;
                }
                return None;
            }

            if w == "import" && (i==0 || !self.is(i-1, ".")) && self.is(i+1, ".") && self.is(i+2, "meta") {
                if self.metafn.is_empty() || !self.is_module { return None; }
                patch(&mut patches, i).before.insert_str(0, &format!("{}(", self.metafn));
                patch(&mut patches, i+2).after.push_str(&format!(",{})", js_string(&self.base)));
                continue;
            }
            if w == "import" && (i==0 || !self.is(i-1, ".")) && self.is(i+1, "(") {
                if self.importfn.is_empty() || self.base.is_empty() { return None; }
                let end = pairs[i+1]?;
                if end <= i+2 { return None; }
                // The helper currently accepts one specifier argument.
                // Reject import attributes/options until implemented.
                let mut j = i+2;
                while j < end {
                    if self.is(j, ",") { return None; }
                    if self.is(j, "(") || self.is(j, "[") || self.is(j, "{") {
                        j = pairs[j]? + 1;
                    } else { j += 1; }
                }
                patch(&mut patches, i).replace = Some(self.importfn.clone());
                patch(&mut patches, i+1).after.push_str(&js_string(&self.base));
                patch(&mut patches, i+1).after.push(',');
                continue;
            }

            if w == "postMessage" && i>0 && self.is(i-1, ".") {
                // Wrap the complete receiver expression, never just the
                // final property in a chain. The helper binds postMessage
                // to its actual owner and preserves optional null checks.
                if self.postmessagefn.is_empty() || i<2 { return None; }
                // The upstream visitor wraps the receiver for reads
                // *and* assignments. Rejecting all assignments prevents
                // otherwise valid scripts from loading in the proxy.
                let optional = i>1 && self.is(i-2, "?");
                let receiver_end = if optional { i.checked_sub(3)? } else { i-2 };
                let mut start = receiver_end;
                if self.tokens[start].kind != Kind::Word {
                    if self.is(start, "]") || self.is(start, ")") {
                        let open = pairs[start]?;
                        start = if open>0 && self.tokens[open-1].kind == Kind::Word {
                            open-1
                        } else { open };
                    } else { return None; }
                }
                while start >= 2 && self.is(start-1, ".") &&
                    self.tokens[start-2].kind == Kind::Word {
                    start -= 2;
                }
                if start>0 && self.is(start-1, ".") ||
                    start>0 && self.is(start-1, "?") ||
                    start>0 && self.is(start-1, "new") ||
                    self.is(start, "super") {
                    return None;
                }
                patch(&mut patches, start).before.insert_str(
                    0, &format!("{}(", self.postmessagefn)
                );
                patch(&mut patches, receiver_end).after.push(')');
                continue;
            }

            // A parenthesized eval callee is a direct eval under ECMAScript
            // semantics, but has different token boundaries. Reject rather
            // than accidentally make it indirect by wrapping the identifier.
            if w == "eval" && i>0 && self.is(i-1, "(") &&
                self.is(i+1, ")") && self.is(i+2, "(") {
                return None;
            }
            if w == "eval" && self.is(i+1, "(") &&
                (i==0 || !self.is(i-1, ".")) {
                if scopes.shadowed(i, "eval") { continue; }
                if self.rewritefn.is_empty() { return None; }
                let end = pairs[i+1]?;
                if end > i+2 {
                    // Direct eval only uses its first argument as code.
                    // Preserve evaluation of subsequent arguments and their
                    // side effects, while wrapping exactly the first value.
                    if self.is(i+2, ".") { return None; } // spread argument
                    let mut j = i+2;
                    let mut first_end = end;
                    while j < end {
                        if self.is(j, ",") {
                            first_end = j;
                            break;
                        }
                        if self.is(j, "(") || self.is(j, "[") || self.is(j, "{") {
                            j = pairs[j]? + 1;
                        } else { j += 1; }
                    }
                    if first_end > i+2 {
                        patch(&mut patches, i+2).before.insert_str(
                            0, &format!("{}(", self.rewritefn)
                        );
                        patch(&mut patches, first_end-1).after.push(')');
                    }
                }
                continue; // preserve the direct eval callee
            }

            if w == "new" && i+2 < n &&
                self.tokens[i+1].kind == Kind::Word &&
                unsafe_name(self.word(i+1)) && self.is(i+2, "(") {
                patch(&mut patches, i+1).before.push('(');
                patch(&mut patches, i+1).after.push(')');
                continue;
            }

            if !unsafe_name(w) { continue; }
            // Object literal methods are property keys, not bare references.
            if i>0 && (
                self.is(i-1, "{") || self.is(i-1, ",") ||
                self.is(i-1, "get") || self.is(i-1, "set") ||
                self.is(i-1, "async")
            ) && self.is(i+1, "(") {
                return None;
            }
            if i>0 && self.is(i-1, ".") {
                if i>1 && self.is(i-2, ".") {
                    if i>2 && self.is(i-3, ".") {
                        patch(&mut patches, i).replace = Some(
                            format!("{}({})", self.wrapfn, w)
                        );
                        continue;
                    }
                    return None;
                }
                patch(&mut patches, i).replace = Some(format!("{}{}", self.propertybase, w));
                continue;
            }
            if self.is(i+1, ":") && i>0 && (self.is(i-1, "{") || self.is(i-1, ",")) {
                // Object literal keys, destructuring keys and statement labels
                // are source-defined property names, not member accesses.
                // Rewriting them changes Object.keys() and normal property
                // semantics. Only a member expression gets the protected
                // property-name substitution.
                continue;
            }
            // Bindings discovered later in the same scope shadow this name.
            if scopes.declaration(i) || scopes.shadowed(i, w) { continue; }
            if i>0 && self.is(i-1, "#") { continue; }
            if i>0 && matches!(self.word(i-1),
                "var" | "let" | "const" | "function" | "class" | "new") {
                return None;
            }
            // Standalone location assignments use the proxy's dedicated
            // setter helper. RHS evaluation occurs once inside the wrapper.
            if w == "location" && self.is(i+1, "=") && !self.trysetfn.is_empty()
                && (i == 0 || self.is(i-1, ";") || self.is(i-1, "{"))
                && i+2 < n && !self.is(i+2, "=") && !self.is(i+2, ">") {
                // A simple statement can have an entire RHS expression, not
                // only one token. Scan to its top-level terminating semicolon
                // or block end while skipping balanced nested expressions.
                // We do not guess about ASI or unparenthesized comma operators.
                let mut end = i+2;
                while end < n && !self.is(end, ";") && !self.is(end, "}") {
                    if self.is(end, ",") || self.is(end, ")") || self.is(end, "]") {
                        return None;
                    }
                    if matches!(self.word(end), "(" | "[" | "{") {
                        end = pairs[end]? + 1;
                    } else { end += 1; }
                }
                if end == i+2 { return None; }
                let rhs = &self.source[self.tokens[i+2].start..self.tokens[end-1].end];
                if rhs.contains('\n') || rhs.contains('\r') { return None; }
                patch(&mut patches, i).replace = Some(format!(
                    "((t)=>{}(location,\"=\",t)||(location=t))(",
                    self.trysetfn
                ));
                patch(&mut patches, i+1).replace = Some(String::new());
                patch(&mut patches, end-1).after.push(')');
                continue;
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
            if w == "eval" && self.is(i+1, "?") { return None; }
            if self.is(i+1, ":") { return None; }
            if i>0 && (
                self.is(i-1, "{") && self.tokens[i-1].kind != Kind::InterpolationOpen ||
                self.is(i-1, ",")
            ) && (self.is(i+1, "}") || self.is(i+1, ",")) {
                if !self.object_shorthand(i, &pairs) { return None; }
                patch(&mut patches, i).after.push_str(
                    &format!(":{}({})", self.wrapfn, w)
                );
                continue;
            }
            patch(&mut patches, i).replace = Some(format!("{}({})", self.wrapfn, w));
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
                patch(&mut patches, i+1).before.insert_str(0, &format!("{}((", self.propertyfn));
                patch(&mut patches, end-1).after.push_str("))");
            }
        }
        // Most fetched bundles have no protected references; avoid a second
        // source copy when the token validation produced zero edits.
        if patches.iter().all(Option::is_none) { return Some(self.source); }
        let mut out = String::with_capacity(self.source.len()+64);
        let mut at = 0;
        for (i,t) in self.tokens.iter().enumerate() {
            out.push_str(&self.source[at..t.start]);
            if let Some(change) = &patches[i] {
                out.push_str(&change.before);
                if let Some(rep) = &change.replace { out.push_str(rep); }
                else { out.push_str(&self.source[t.start..t.end]); }
                out.push_str(&change.after);
            } else {
                out.push_str(&self.source[t.start..t.end]);
            }
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
    fn unicode_identifiers_and_comments() {
        let original = "const π = 3; const naïve = window.location; // 你好\nπ + naïve;";
        let expected = "const π = 3; const naïve = window.$prop$location; // 你好\nπ + naïve;";
        for chunk in 1..=original.len() {
            let mut engine = SemanticProgram::new("$wrap", "$prop$", "$computed", false);
            for ch in original.chars() {
                engine.write(&ch.to_string());
            }
            assert_eq!(engine.finish(), Some(expected.into()), "chunk {chunk}");
        }
        assert_eq!(rewrite("\u{feff}top;", 200), Some("\u{feff}$wrap(top);".into()));
    }
    #[test]
    fn recognizes_regex_after_arrow_and_binary_operators() {
        assert_eq!(rewrite("const f = () => /location/.test('location');", 100),
            Some("const f = () => /location/.test('location');".into()));
        assert_eq!(rewrite("const x = 2 > /top/.test(text);", 100),
            Some("const x = 2 > /top/.test(text);".into()));
    }
    #[test]
    fn division_after_call_and_regex_after_control() {
        assert_eq!(rewrite("const result = calc() / 2;", 100),
            Some("const result = calc() / 2;".into()));
        assert_eq!(rewrite("if (condition) /location/.test(text);", 100),
            Some("if (condition) /location/.test(text);".into()));
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
    fn resolves_multi_declarator_shadowing() {
        assert_eq!(rewrite("var a = 1, location;location;", 200),
            Some("var a = 1, location;location;".into()));
        assert_eq!(rewrite("let x = top, parent = 5; top;", 200),
            Some("let x = $wrap(top), parent = 5; $wrap(top);".into()));
        assert_eq!(rewrite("const x = {foo:top}, parent = 7; parent;", 200),
            Some("const x = {foo:$wrap(top)}, parent = 7; parent;".into()));
    }
    #[test]
    fn function_expression_binding_is_not_a_global() {
        let src = "const f = function top(){return top;}; top;";
        assert_eq!(rewrite(src, src.len()), Some(
            "const f = function top(){return top;}; $wrap(top);".into()
        ));
    }
    #[test]
    fn nested_scope_forward_binding_resolution() {
        let src = "top; { top; const top = 123; top; } top;";
        assert_eq!(rewrite(src, src.len()),
            Some("$wrap(top); { top; const top = 123; top; } $wrap(top);".into()));
        let src = "top; function f(top) { return top + parent; } top;";
        assert_eq!(rewrite(src, src.len()),
            Some("$wrap(top); function f(top) { return top + $wrap(parent); } $wrap(top);".into()));
    }
    #[test]
    fn template_interpolations_and_chunk_boundaries() {
        let source = "const t = `host ${window.location.hostname}: ${top}`;";
        let expected = "const t = `host ${window.$prop$location.hostname}: ${$wrap(top)}`;";
        for n in 1..=source.len() {
            assert_eq!(rewrite(source, n), Some(expected.into()), "chunk {n}");
        }
        let nested = "const t = `a ${`b ${top}`}`;";
        let nested_expected = "const t = `a ${`b ${$wrap(top)}`}`;";
        for n in 1..=nested.len() {
            assert_eq!(rewrite(nested, n), Some(nested_expected.into()), "nested {n}");
        }
    }

    fn rewrite_with_helpers(source: &str) -> Option<String> {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$computed", false)
            .with_helpers("$import", "$rewrite", "$post", "https://a.test/mod.js");
        engine.write(source);
        engine.finish()
    }
    #[test]
    fn module_eval_and_message_helpers() {
        assert_eq!(rewrite_with_helpers("const m = import('./x.js');"),
            Some("const m = $import(\"https://a.test/mod.js\",'./x.js');".into()));
        assert_eq!(rewrite_with_helpers("eval('window.location');"),
            Some("eval($rewrite('window.location'));".into()));
        assert_eq!(rewrite_with_helpers("window.postMessage('x');"),
            Some("$post(window).postMessage('x');".into()));
        assert_eq!(rewrite_with_helpers("top.postMessage('x');"),
            Some("$post($wrap(top)).postMessage('x');".into()));
        assert!(rewrite_with_helpers("import('./x',{with:{type:'json'}})").is_none());
    }

    #[test]
    fn postmessage_receiver_is_wrapped_once_across_chains() {
        assert_eq!(rewrite_with_helpers("frames[0].postMessage('x');"),
            Some("$post(frames[0]).postMessage('x');".into()));
        assert_eq!(rewrite_with_helpers("foo.bar.postMessage('x');"),
            Some("$post(foo.bar).postMessage('x');".into()));
        assert_eq!(rewrite_with_helpers("foo?.postMessage('x');"),
            Some("$post(foo)?.postMessage('x');".into()));
        assert_eq!(rewrite_with_helpers("getTarget().postMessage('x');"),
            Some("$post(getTarget()).postMessage('x');".into()));
        assert_eq!(rewrite_with_helpers("foo.postMessage = fn;"),
            Some("$post(foo).postMessage = fn;".into()));
        assert_eq!(rewrite_with_helpers("foo.postMessage += fn;"),
            Some("$post(foo).postMessage += fn;".into()));
    }
    #[test]
    fn preserves_direct_eval_with_extra_arguments() {
        assert_eq!(rewrite_with_helpers("eval('x', sideEffect());"),
            Some("eval($rewrite('x'), sideEffect());".into()));
        assert_eq!(rewrite_with_helpers("eval(foo(1,2), bar);"),
            Some("eval($rewrite(foo(1,2)), bar);".into()));
        assert_eq!(rewrite_with_helpers("eval(...args)"), None);
        assert_eq!(rewrite_with_helpers("(eval)('x')"), None);
    }
    #[test]
    fn rejects_member_eval_module_meta_and_method_property_ambiguity() {
        assert!(rewrite("const x = import.meta.url;", 40).is_none());
        assert!(rewrite("const obj = {location(){return 1}};", 50).is_none());
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_helpers("$import", "$rewrite", "$post", "https://example.test/");
        engine.write("window.eval('x')");
        assert_eq!(engine.finish(), Some("window.$prop$eval('x')".into()));
    }
    #[test]
    fn rewrites_module_metadata_without_parser() {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_helpers("$import", "$rewrite", "$post", "https://site.test/app.mjs")
            .with_module_meta("$meta", true);
        engine.write("const url = import.meta.url;");
        assert_eq!(engine.finish(), Some(
            "const url = $meta(import.meta,\"https://site.test/app.mjs\").url;".into()
        ));
    }
    #[test]
    fn location_assignment_is_intercepted_and_rhs_evaluated_once() {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_assignment("$tryset");
        engine.write("location = top;");
        assert_eq!(engine.finish(), Some(
            "((t)=>$tryset(location,\"=\",t)||(location=t))(  $wrap(top));".into()
        ));
    }
    #[test]
    fn supports_exported_declarations_without_ast() {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write("export const home = window.location.href;");
        assert_eq!(engine.finish(), Some(
            "export const home = window.$prop$location.href;".into()
        ));
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write("export {home} from './module.js';");
        assert!(engine.finish().is_none());
    }
    #[test]
    fn removes_debugger_without_changing_script_semantics() {
        assert_eq!(rewrite("debugger; top;", 200), Some("; $wrap(top);".into()));
        assert_eq!(rewrite("function f(){debugger;return 7;}", 200),
            Some("function f(){;return 7;}".into()));
        assert_eq!(rewrite("obj.debugger;", 200), Some("obj.debugger;".into()));
    }
    #[test]
    fn rewrites_unsafe_object_shorthand_without_changing_key_name() {
        assert_eq!(rewrite("const obj = {top};", 100),
            Some("const obj = {top:$wrap(top)};".into()));
        assert_eq!(rewrite("const obj = {key: 1, top, location};", 100),
            Some("const obj = {key: 1, top:$wrap(top), location:$wrap(location)};".into()));
        assert_eq!(rewrite("({parent})", 100),
            Some("({parent:$wrap(parent)})".into()));
        assert_eq!(rewrite("{top}", 100), None);
    }
    #[test]
    fn object_literal_keys_are_not_protected_member_references() {
        let src = "const obj = {location: 1, top: 2, parent: 3, eval: 4, safe: 5};";
        assert_eq!(rewrite(src, src.len()), Some(src.into()));
        let src = "const obj = {location: top, parent: location};";
        assert_eq!(
            rewrite(src, src.len()),
            Some("const obj = {location: $wrap(top), parent: $wrap(location)};".into())
        );
        let src = "const obj = {location: 1}; obj.location;";
        assert_eq!(
            rewrite(src, src.len()),
            Some("const obj = {location: 1}; obj.$prop$location;".into())
        );
    }
    #[test]
    fn contextual_keywords_can_be_methods_or_properties() {
        assert_eq!(rewrite("array.with(0, top);", 100),
            Some("array.with(0, $wrap(top));".into()));
        assert_eq!(rewrite("obj.import('./x');", 100),
            Some("obj.import('./x');".into()));
    }
    #[test]
    fn local_exports_are_not_expression_references() {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write("const x = top; export {x};");
        assert_eq!(engine.finish(), Some("const x = $wrap(top); export {x};".into()));
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write("export {top};");
        assert!(engine.finish().is_none());
    }
    #[test]
    fn static_module_specifiers_use_host_url_resolver() {
        let script = "import x from './mod.js'; export {x} from '../a.mjs';";
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write(script);
        let result = engine.finish_with_url_rewriter(
            |specifier| Some(format!("/proxy/{specifier}"))
        );
        assert_eq!(result, Some(
            "import x from \"/proxy/./mod.js\"; export {x} from \"/proxy/../a.mjs\";".into()
        ));
    }
    #[test]
    fn rejects_unresolved_relative_module_urls() {
        let mut engine = SemanticProgram::new("$wrap", "$prop$", "$compute", false)
            .with_module_meta("$meta", true);
        engine.write("import './mod.js';");
        assert!(engine.finish().is_none());
    }
    #[test]
    fn supports_simple_classes_without_parser() {
        let src = "class Test {constructor(value){this.value=value;}} const x=new Test(top);";
        assert_eq!(rewrite(src, src.len()), Some(
            "class Test {constructor(value){this.value=value;}} const x=new Test($wrap(top));".into()));
        assert_eq!(rewrite("class X { location(){return top;} }", 100), None);
        assert_eq!(rewrite("class top { constructor(){} }", 100), None);
    }
    #[test]
    fn preserves_destructuring_declarations_without_unsafe_binding_names() {
        for source in [
            "const {a,b} = obj;",
            "const {a: renamed, ...rest} = obj;",
            "const {a = 'default'} = obj;",
            "let [a, b = 5, ...rest] = values;",
            "const { nested: { value }} = obj;",
            "const {[key]: value} = obj;",
        ] {
            assert_eq!(rewrite(source, source.len()), Some(source.into()), "{source:?}");
        }
        assert_eq!(rewrite("({a}=obj);", 100), None);
        assert_eq!(rewrite("const {top} = obj;", 100), None);
    }
    #[test]
    fn object_grammar_and_spread_identifiers() {
        assert!(rewrite("({location:x}=obj);", 50).is_none());
        assert!(rewrite("const o={get location(){return 1}};", 60).is_none());
        assert_eq!(rewrite("f(...location);", 60), Some("f(...$wrap(location));".into()));
        assert_eq!(rewrite("const o = {...top};", 60),
            Some("const o = {...$wrap(top)};".into()));
    }
    #[test]
    fn fails_closed() {
        for s in [
            "const {location} = window;", "function f({location}){return location}",
            "location = x;", "eval('x')", "import('./x.js')",
            "with(x){location}", "obj.postMessage('x')", "const {location} = x",
        ] { assert!(rewrite(s,s.len()).is_none(),"{s:?}"); }
    }
}
