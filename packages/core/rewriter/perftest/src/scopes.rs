//! Lightweight lexical scope index, not an AST.
//!
//! All scopes are contiguous token intervals, discovered from the single
//! forward token stream. Bindings can be declared after an earlier reference;
//! no output is released until those bindings are known.
//!
//! Unsupported/ambiguous binding patterns return None (fail closed).

const NAMES: [&str; 4] = ["parent", "top", "location", "eval"];
fn bit(name: &str) -> u8 {
    NAMES.iter().position(|x| *x == name).map_or(0, |i| 1 << i)
}
fn control(name: &str) -> bool {
    matches!(name, "if" | "for" | "while" | "switch" | "catch" |
        "with" | "try" | "do" | "else")
}
#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    function: bool,
    bindings: u8,
}
pub struct ScopeIndex {
    scopes: Vec<Scope>,
    at: Vec<usize>,
    declarations: Vec<bool>,
    parameter_masks: Vec<u8>,
}
impl ScopeIndex {
    pub fn new(words: &[&str], is_word: &[bool], pairs: &[Option<usize>]) -> Option<Self> {
        if words.len() != is_word.len() || words.len() != pairs.len() { return None; }
        let n = words.len();
        let mut result = Self {
            scopes: vec![Scope::default()],
            at: vec![0; n],
            declarations: vec![false; n],
            parameter_masks: vec![0; n],
        };
        let mut stack = vec![0usize];
        // Build a lightweight hierarchy of curly-brace regions.
        let mut brace_scope = vec![None; n];
        for i in 0..n {
            if words[i] == "{" {
                let parent = *stack.last()?;
                let new = result.scopes.len();
                result.scopes.push(Scope { parent: Some(parent),
                    function: Self::function_body(words, pairs, i),
                    bindings: 0 });
                brace_scope[i] = Some(new);
                stack.push(new);
            }
            result.at[i] = *stack.last()?;
            if words[i] == "}" {
                if stack.len() <= 1 { return None; }
                stack.pop();
            }
        }
        if stack.len() != 1 { return None; }

        // Identify parameter bindings for function declarations, function
        // expressions, catch handlers, class/object methods and block arrows.
        for i in 0..n {
            if words[i] != "{" { continue; }
            let scope = brace_scope[i]?;
            if i > 0 && words[i-1] == ")" {
                let open = pairs[i-1]?;
                if Self::function_body(words, pairs, i) {
                    result.add_params(words, is_word, pairs, scope, open+1, i-1)?;
                }
            } else if i > 1 && words[i-1] == ">" && words[i-2] == "=" {
                if i < 3 { return None; }
                if words[i-3] == ")" {
                    let open = pairs[i-3]?;
                    result.add_params(words, is_word, pairs, scope, open+1, i-3)?;
                } else if is_word[i-3] {
                    result.bind(scope, words[i-3], i-3);
                } else { return None; }
            }
            if i > 1 && words[i-1] == ")" {
                let open = pairs[i-1]?;
                if open > 0 && words[open-1] == "catch" {
                    result.add_params(words, is_word, pairs, scope, open+1, i-1)?;
                }
            }
        }
        // Function declarations bind in their enclosing scope. Named
        // function EXPRESSIONS bind only inside their own function scope.
        // An expression name must not shadow globals outside that body.
        for i in 0..n {
            if words[i] != "function" { continue; }
            let mut j = i+1;
            if j < n && words[j] == "*" { j += 1; }
            if j >= n || !is_word[j] || words.get(j+1) != Some(&"(") {
                continue;
            }
            let prev = if i > 0 && words[i-1] == "async" {
                words.get(i.wrapping_sub(2)).copied()
            } else {
                i.checked_sub(1).and_then(|index| words.get(index).copied())
            };
            let expression = matches!(prev,
                Some("=" | "(" | "[" | "," | ":" | "return" | "=>" | "yield"));
            if expression {
                let params_close = pairs[j+1]?;
                let open_brace = params_close+1;
                if words.get(open_brace) != Some(&"{") { return None; }
                let scope = brace_scope[open_brace]?;
                result.bind(scope, words[j], j);
            } else {
                result.bind(result.at[i], words[j], j);
            }
        }
        // The lexical scope of a for(let/const ...) binding includes its
        // loop body but not the surrounding block. Until loop-region scopes
        // are indexed, do not mark that name in an outer scope.
        for i in 0..n {
            if words[i] != "for" || words.get(i+1) != Some(&"(") { continue; }
            let end = pairs[i+1]?;
            for j in i+2..end {
                if matches!(words[j], "let" | "const") &&
                    j+1 < end && bit(words[j+1]) != 0 {
                    return None;
                }
            }
        }
        // Scan declarations with source-order independent resolution.
        for i in 0..n {
            if !matches!(words[i], "let" | "const" | "var") { continue; }
            // Distinguish property references such as obj.const and const
            // on the right side of an object literal.
            if i > 0 && words[i-1] == "." { continue; }
            let mut scope = result.at[i];
            if words[i] == "var" {
                while scope != 0 && !result.scopes[scope].function {
                    scope = result.scopes[scope].parent?;
                }
            }
            let mut j = i+1;
            let mut binding = true;
            // A declaration cannot extend past a semicolon at this nesting level.
            // Skip nested initializer structures without inspecting them as names.
            while j < n && words[j] != ";" && words[j] != ")" && words[j] != "}" {
                if words[j] == "," {
                    binding = true;
                    j += 1;
                    continue;
                }
                if binding {
                    if words[j] == "[" || words[j] == "{" {
                        let end = pairs[j]?;
                        // Destructuring of unsafe names requires property
                        // and TDZ semantics not yet covered by this engine.
                        if (j+1..end).any(|k| bit(words[k]) != 0) { return None; }
                        j = end+1; binding = false; continue;
                    }
                    if !is_word[j] { return None; }
                    result.bind(scope, words[j], j);
                    binding = false;
                    j += 1;
                    continue;
                }
                // '=' introduces an initializer that may contain nested
                // blocks; skip those as a unit.
                if matches!(words[j], "(" | "[" | "{") {
                    j = pairs[j]? + 1;
                } else { j += 1; }
            }
        }

        // Expression-bodied arrows with protected parameter names still
        // require an interval scope; reject until that case is implemented.
        for i in 1..n.saturating_sub(1) {
            if words[i] == "=" && words[i+1] == ">" && words.get(i+2) != Some(&"{") {
                if i >= 1 && is_word[i-1] && bit(words[i-1]) != 0 { return None; }
                if words[i-1] == ")" {
                    let start = pairs[i-1]?;
                    if (start+1..i-1).any(|j| bit(words[j]) != 0) { return None; }
                }
            }
        }
        Some(result)
    }

    fn function_body(words: &[&str], pairs: &[Option<usize>], open_brace: usize) -> bool {
        if open_brace == 0 { return false; }
        let prev = words[open_brace-1];
        if prev == ">" && open_brace >= 2 && words[open_brace-2] == "=" {
            return true;
        }
        if prev != ")" { return false; }
        let Some(open) = pairs[open_brace-1] else { return false };
        if open == 0 { return false; }
        if control(words[open-1]) { return false; }
        // Function declarations and method definitions.
        if words[open-1] == "function" || words[open-1] == "*" {
            return true;
        }
        if open >= 2 && words[open-2] == "function" { return true; }
        if open >= 3 && words[open-3] == "function" { return true; }
        // Bare method names are allowed only in brace/object contexts.
        // The grammar gate independently rejects unsupported class syntax.
        open >= 2 && matches!(words[open-2], "{" | "," | ";" | "}")
    }

    fn bind(&mut self, scope: usize, name: &str, position: usize) {
        self.declarations[position] = true;
        self.scopes[scope].bindings |= bit(name);
    }

    fn add_params(&mut self, words: &[&str], is_word: &[bool], pairs: &[Option<usize>],
        scope: usize, start: usize, end: usize) -> Option<()> {
        let mut next_binding = true;
        let mut i = start;
        while i < end {
            if words[i] == "," { next_binding = true; i += 1; continue; }
            if next_binding {
                if matches!(words[i], "{" | "[") {
                    let close = pairs[i]?;
                    if (i+1..close).any(|j| bit(words[j]) != 0) {
                        // Track protected names in simple destructured
                        // parameters, without accepting aliases, defaults, or
                        // nested patterns whose bindings require grammar.
                        // E.g. function f({location, top}) { ... } is safe:
                        // those names are bound locals throughout this body.
                        let mut j = i+1;
                        while j < close {
                            if words[j] == "," { j += 1; continue; }
                            if !is_word[j] || (j+1 < close && words[j+1] != ",") {
                                return None;
                            }
                            self.bind(scope, words[j], j);
                            j += 1;
                        }
                    }
                    i = close+1; next_binding = false; continue;
                }
                if words[i] == "." { i += 1; continue; } // rest parameter
                if !is_word[i] { return None; }
                self.bind(scope, words[i], i);
                next_binding = false;
                i += 1;
                continue;
            }
            if matches!(words[i], "(" | "[" | "{") {
                i = pairs[i]? + 1;
            } else { i += 1; }
        }
        // Parameter default expressions use the same parameter environment.
        // Even a later parameter shadows an earlier default (TDZ).
        let mask = self.scopes[scope].bindings;
        for index in start..end {
            self.parameter_masks[index] |= mask;
        }
        Some(())
    }

    pub fn shadowed(&self, token_index: usize, name: &str) -> bool {
        let flag = bit(name);
        if flag == 0 { return false; }
        if (self.parameter_masks[token_index] & flag) != 0 { return true; }
        let mut scope = self.at[token_index];
        loop {
            if (self.scopes[scope].bindings & flag) != 0 { return true; }
            match self.scopes[scope].parent {
                Some(parent) => scope = parent,
                None => return false,
            }
        }
    }
    pub fn declaration(&self, token_index: usize) -> bool {
        self.declarations[token_index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn analyze(items: &[&str]) -> ScopeIndex {
        let mut stack: Vec<usize> = vec![];
        let mut pairs = vec![None; items.len()];
        for (i, tok) in items.iter().enumerate() {
            if matches!(*tok, "(" | "{" | "[") { stack.push(i); }
            else if matches!(*tok, ")" | "}" | "]") {
                let start = stack.pop().unwrap();
                pairs[i] = Some(start);
                pairs[start] = Some(i);
            }
        }
        ScopeIndex::new(items, &items.iter().map(|x| {
            x.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        }).collect::<Vec<_>>(), &pairs).unwrap()
    }
    #[test]
    fn lexical_binding_deferred_resolution() {
        let words = ["top", ";", "{", "let", "top", "=", "1", ";", "top", ";", "}", "top"];
        let scopes = analyze(&words);
        assert!(!scopes.shadowed(0, "top"));
        assert!(scopes.shadowed(8, "top"));
        assert!(!scopes.shadowed(11, "top"));
        assert!(scopes.declaration(4));
    }
    #[test]
    fn does_not_hoist_lexical_for_loop_bindings_to_outer_scope() {
        let words = ["top", ";", "for", "(", "let", "top", "of", "[", "]",
            ")", "{", "top", ";", "}", "top"];
        let mut stack = vec![];
        let mut pairs = vec![None; words.len()];
        for (i, word) in words.iter().enumerate() {
            if matches!(*word, "(" | "{" | "[") { stack.push(i); }
            else if matches!(*word, ")" | "}" | "]") {
                let open = stack.pop().unwrap();
                pairs[i] = Some(open); pairs[open] = Some(i);
            }
        }
        let lexical = words.iter().map(|w| w.chars().next()
            .is_some_and(|ch| ch.is_alphabetic())).collect::<Vec<_>>();
        assert!(ScopeIndex::new(&words, &lexical, &pairs).is_none());
    }
    #[test]
    fn var_binding_hoisted_to_root() {
        let words = ["top", ";", "{", "var", "top", "=", "1", ";", "}", "top"];
        let scopes = analyze(&words);
        assert!(scopes.shadowed(0, "top"));
        assert!(scopes.shadowed(9, "top"));
    }
    #[test]
    fn single_parameter_arrow_binding() {
        let words = ["top", "=", ">", "{", "top", ";", "}", "top"];
        let scopes = analyze(&words);
        assert!(scopes.shadowed(4, "top"));
        assert!(!scopes.shadowed(7, "top"));
    }
    #[test]
    fn named_function_expression_name_does_not_escape_its_scope() {
        let words = ["const", "f", "=", "function", "top", "(", ")",
            "{", "top", ";", "}", ";", "top", ";"];
        let scopes = analyze(&words);
        assert!(scopes.shadowed(8, "top"));
        assert!(!scopes.shadowed(12, "top"));
        assert!(scopes.declaration(4));
    }
    #[test]
    fn later_parameter_shadowing_applies_to_default_expressions() {
        let words = [
            "function", "f", "(", "x", "=", "top", ",", "top", "=", "1",
            ")", "{", "top", ";", "}"
        ];
        let scopes = analyze(&words);
        assert!(scopes.shadowed(5, "top"));
        assert!(scopes.shadowed(12, "top"));
        assert!(scopes.declaration(7));
    }
    #[test]
    fn function_parameter_shadowing() {
        let words = ["top", ";", "function", "f", "(", "top", ")", "{", "top", ";", "}", "top"];
        let scopes = analyze(&words);
        assert!(!scopes.shadowed(0, "top"));
        assert!(scopes.shadowed(8, "top"));
        assert!(!scopes.shadowed(11, "top"));
    }
}
