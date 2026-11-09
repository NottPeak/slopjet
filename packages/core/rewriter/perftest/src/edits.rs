//! Source-preserving edit application for future semantic rewrite passes.
//! Edits are half-open UTF-8 byte ranges. No AST regeneration is required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum EditError { OutOfBounds, InvalidUtf8Boundary, Overlap }

/// Edits must be ordered by source offset; insertions at the same location
/// are deliberately rejected to avoid ambiguous ordering.
pub fn apply_edits(source: &str, edits: &[Edit]) -> Result<String, EditError> {
    let mut cursor = 0;
    let mut output = String::with_capacity(source.len());
    let mut previous: Option<&Edit> = None;
    for edit in edits {
        if edit.start > edit.end || edit.end > source.len() {
            return Err(EditError::OutOfBounds);
        }
        if !source.is_char_boundary(edit.start) || !source.is_char_boundary(edit.end) {
            return Err(EditError::InvalidUtf8Boundary);
        }
        if edit.start < cursor || previous.is_some_and(|p|
            p.start == p.end && p.start == edit.start
        ) {
            return Err(EditError::Overlap);
        }
        output.push_str(&source[cursor..edit.start]);
        output.push_str(&edit.replacement);
        cursor = edit.end;
        previous = Some(edit);
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserve_unmodified_source() {
        let src = "/* before */ location.href; // after";
        let start = src.find("location").unwrap();
        let result = apply_edits(src, &[Edit {
            start, end: start + "location".len(),
            replacement: "$wrap(location)".into()
        }]).unwrap();
        assert_eq!(result, "/* before */ $wrap(location).href; // after");
    }
    #[test]
    fn reject_overlap_and_invalid_boundaries() {
        assert_eq!(apply_edits("abcdef", &[
            Edit { start: 1, end: 4, replacement: "".into() },
            Edit { start: 3, end: 5, replacement: "".into() },
        ]), Err(EditError::Overlap));
        assert_eq!(apply_edits("é", &[Edit {
            start: 1, end: 2, replacement: "x".into()
        }]), Err(EditError::InvalidUtf8Boundary));
    }
    #[test]
    fn insertions_and_deletions() {
        let src = "abcd";
        assert_eq!(apply_edits(src, &[
            Edit { start: 0, end: 0, replacement: "!".into() },
            Edit { start: 1, end: 3, replacement: "".into() },
        ]).unwrap(), "!ad");
    }
}
