use std::ops::Range;
use thiserror::Error;

pub trait Backend: Sync {
    fn id(&self) -> &'static str;
    fn plan(&self, source: &[u8]) -> BackendResult;
}

#[derive(Debug)]
pub enum BackendResult {
    Ready(FormatPlan),
    Diagnostics(Vec<Diagnostic>),
}

#[derive(Debug)]
pub struct FormatPlan {
    pub boundaries: Vec<Boundary>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct Boundary {
    pub range: Range<usize>,
    pub required: bool,
    pub indentation: Vec<u8>,
    pub line_ending: LineEnding,
    pub barrier: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    #[must_use]
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub range: Option<Range<usize>>,
    pub message: String,
    pub backend: &'static str,
}

#[derive(Debug, Error)]
pub enum RewriteError {
    #[error("editable whitespace range is outside the source")]
    RangeOutOfBounds,

    #[error("editable whitespace range contains non-whitespace bytes")]
    UnsafeRange,

    #[error("formatting patches overlap or conflict")]
    PatchConflict,
}

/// Rewrites only the backend-selected boundary whitespace ranges.
///
/// # Errors
///
/// Returns an error when a range is outside the source, contains
/// non-whitespace bytes, or conflicts with another range.
pub fn rewrite(source: &[u8], boundaries: &[Boundary]) -> Result<Vec<u8>, RewriteError> {
    let mut patches = Vec::new();

    for boundary in boundaries {
        if boundary.barrier {
            continue;
        }

        let Some(gap) = source.get(boundary.range.clone()) else {
            return Err(RewriteError::RangeOutOfBounds);
        };

        if !gap.iter().all(|byte| is_editable_whitespace(*byte))
            || !boundary
                .indentation
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t'))
        {
            return Err(RewriteError::UnsafeRange);
        }

        if !boundary.required && !has_line_break(gap) {
            continue;
        }

        let mut replacement = Vec::with_capacity(
            boundary.line_ending.bytes().len() * if boundary.required { 2 } else { 1 }
                + boundary.indentation.len(),
        );

        replacement.extend_from_slice(boundary.line_ending.bytes());

        if boundary.required {
            replacement.extend_from_slice(boundary.line_ending.bytes());
        }

        replacement.extend_from_slice(&boundary.indentation);

        patches.push(Patch {
            range: boundary.range.clone(),
            replacement,
        });
    }

    validate_patches(source, &mut patches)?;
    let mut output = source.to_vec();

    for patch in patches.into_iter().rev() {
        output.splice(patch.range, patch.replacement);
    }

    Ok(output)
}

struct Patch {
    range: Range<usize>,
    replacement: Vec<u8>,
}

fn validate_patches(source: &[u8], patches: &mut Vec<Patch>) -> Result<(), RewriteError> {
    patches.sort_by_key(|patch| (patch.range.start, patch.range.end));
    let mut validated: Vec<Patch> = Vec::with_capacity(patches.len());

    for patch in patches.drain(..) {
        let Some(range) = source.get(patch.range.clone()) else {
            return Err(RewriteError::RangeOutOfBounds);
        };

        if !range.iter().all(|byte| is_editable_whitespace(*byte)) {
            return Err(RewriteError::UnsafeRange);
        }

        if let Some(previous) = validated.last() {
            if previous.range == patch.range {
                if previous.replacement == patch.replacement {
                    continue;
                }

                return Err(RewriteError::PatchConflict);
            }

            if previous.range.end > patch.range.start {
                return Err(RewriteError::PatchConflict);
            }
        }

        validated.push(patch);
    }

    *patches = validated;
    Ok(())
}

fn has_line_break(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n'))
}

const fn is_editable_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary(start: usize, end: usize, replacement: LineEnding) -> Boundary {
        Boundary {
            range: start..end,
            required: false,
            indentation: Vec::new(),
            line_ending: replacement,
            barrier: false,
        }
    }

    #[test]
    fn identical_patches_are_deduplicated() {
        let mut first = boundary(0, 1, LineEnding::Lf);
        first.required = true;
        let mut second = boundary(0, 1, LineEnding::Lf);
        second.required = true;
        assert_eq!(rewrite(b" ", &[first, second]).expect("safe"), b"\n\n");
    }

    #[test]
    fn overlapping_patches_are_rejected() {
        let mut first = boundary(0, 2, LineEnding::Lf);
        first.required = true;
        let mut second = boundary(1, 2, LineEnding::Lf);
        second.required = true;

        assert!(matches!(
            rewrite(b"  ", &[first, second]),
            Err(RewriteError::PatchConflict)
        ));
    }

    #[test]
    fn conflicting_insertions_are_rejected() {
        let mut first = boundary(1, 1, LineEnding::Lf);
        first.required = true;
        first.indentation = b" ".to_vec();
        let mut second = boundary(1, 1, LineEnding::Lf);
        second.required = true;
        second.indentation = b"  ".to_vec();

        assert!(matches!(
            rewrite(b" ", &[first, second]),
            Err(RewriteError::PatchConflict)
        ));
    }

    #[test]
    fn barriers_are_not_edited() {
        let mut barrier = boundary(0, 1, LineEnding::Lf);
        barrier.barrier = true;
        assert_eq!(rewrite(b" ", &[barrier]).expect("safe"), b" ");
    }

    #[test]
    fn optional_same_line_whitespace_is_preserved() {
        let boundary = boundary(0, 1, LineEnding::Lf);
        assert_eq!(rewrite(b" ", &[boundary]).expect("safe"), b" ");
    }

    #[test]
    fn required_crlf_boundaries_keep_crlf() {
        let mut boundary = boundary(0, 2, LineEnding::CrLf);
        boundary.required = true;
        assert_eq!(rewrite(b"  ", &[boundary]).expect("safe"), b"\r\n\r\n");
    }
}
