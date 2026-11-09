//! Single-pass deferred-output coordinator for the experimental lexer.
//!
//! Correctness policy: never emit a potentially rewritable region before
//! all declarations relevant to its binding resolution are known.
//! This initial coordinator conservatively holds the entire program until EOF.
//! A future grammar/scope engine can release smaller regions when proven safe.
//! No JavaScript transformations are performed here.
use crate::{Scanner, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredOutput {
    /// Source bytes for the region that cannot yet be safely emitted.
    pub pending_bytes: usize,
    /// Token count waiting for semantic decisions.
    pub pending_tokens: usize,
    /// Bytes emitted after semantic validation (zero until finish in v0).
    pub emitted_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferredError {
    Lexical(String),
    Unsupported(Vec<usize>),
}

/// Accepts arbitrarily chunked valid UTF-8 source. The finalizer refuses
/// ambiguous syntax rather than returning incorrectly rewritten JavaScript.
pub struct DeferredScanner {
    lexer: Scanner,
    pending: Vec<Span>,
    source: String,
    emitted: usize,
}

impl Default for DeferredScanner {
    fn default() -> Self {
        Self { lexer: Scanner::new(), pending: Vec::new(),
            source: String::new(), emitted: 0 }
    }
}

impl DeferredScanner {
    pub fn new() -> Self { Self::default() }

    /// Scans once, retains tokens and source for later binding resolution.
    /// No output is released while a future declaration could change meaning.
    pub fn write(&mut self, chunk: &str) -> DeferredOutput {
        self.source.push_str(chunk);
        self.pending.extend(self.lexer.write(chunk));
        self.status()
    }

    pub fn status(&self) -> DeferredOutput {
        DeferredOutput {
            pending_bytes: self.source.len() - self.emitted,
            pending_tokens: self.pending.len(),
            emitted_bytes: self.emitted,
        }
    }

    /// Return original source only for lexically supported inputs.
    /// A later semantic pass will consume the pending spans and produce edits.
    pub fn finish(mut self) -> Result<String, DeferredError> {
        self.pending.extend(self.lexer.finish().map_err(DeferredError::Lexical)?);
        if !self.lexer.unsafe_syntax_offsets().is_empty() {
            return Err(DeferredError::Unsupported(
                self.lexer.unsafe_syntax_offsets().to_vec()
            ));
        }
        let reconstructed: String = self.pending.iter()
            .map(|span| span.source.as_str()).collect();
        if reconstructed != self.source {
            return Err(DeferredError::Lexical("source span reconstruction mismatch".into()));
        }
        Ok(self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forward_shadowing_never_emits_early() {
        let mut engine = DeferredScanner::new();
        let first = "console.log(window.location);";
        let second = " let window = customWindow;";
        let state = engine.write(first);
        assert_eq!(state.emitted_bytes, 0);
        assert_eq!(state.pending_bytes, first.len());
        engine.write(second);
        assert_eq!(engine.finish().unwrap(), format!("{first}{second}"));
    }

    #[test]
    fn no_loss_at_chunk_boundaries() {
        let input = "function f(window) { return window.location; }";
        for step in 1..=input.len() {
            let mut engine = DeferredScanner::new();
            for part in input.as_bytes().chunks(step) {
                engine.write(std::str::from_utf8(part).unwrap());
            }
            assert_eq!(engine.finish().unwrap(), input);
        }
    }

    #[test]
    fn refuses_ambiguous_regexp() {
        let mut engine = DeferredScanner::new();
        engine.write("const pattern = /abc/;");
        assert!(matches!(engine.finish(), Err(DeferredError::Unsupported(_))));
    }
}
