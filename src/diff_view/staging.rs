//! Staging one change hunk from one working buffer into the other.

use super::{diff_hunk_row_ranges, DiffRow};
use crate::side::Side;
use crate::text::TextBuffer;

/// Direction for copying a single change hunk between file sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkCopyDirection {
    LeftToRight,
    RightToLeft,
}

/// Per-row 0-based line indices in the left and right files (`None` when that side is empty).
fn diff_row_file_line_indices(diff_rows: &[DiffRow]) -> Vec<(Option<usize>, Option<usize>)> {
    diff_rows
        .iter()
        .map(|row| (row.left_source, row.right_source))
        .collect()
}

fn hunk_side_line_range(
    indices: &[(Option<usize>, Option<usize>)],
    row_range: std::ops::Range<usize>,
    side: Side,
) -> Option<std::ops::Range<usize>> {
    let line_nos: Vec<usize> = row_range
        .filter_map(|i| match side {
            Side::Left => indices[i].0,
            Side::Right => indices[i].1,
        })
        .collect();
    if line_nos.is_empty() {
        None
    } else {
        Some(*line_nos.first().unwrap()..line_nos.last().unwrap() + 1)
    }
}

fn extract_hunk_lines(
    diff_rows: &[DiffRow],
    row_range: std::ops::Range<usize>,
    side: Side,
) -> Vec<String> {
    diff_rows[row_range]
        .iter()
        .filter_map(|row| {
            let line = match side {
                Side::Left => &row.left,
                Side::Right => &row.right,
            };
            line.as_ref()
                .map(|line| line.text.trim_end_matches(['\r', '\n']).to_string())
        })
        .collect()
}

/// Whether a hunk's only difference is the presence of a trailing newline at
/// EOF — every row's left and right text is identical once line breaks are
/// trimmed, but not identical raw (that raw difference is why `similar`
/// flagged the row as a change at all), and the hunk reaches the last row of
/// the diff (line-ending shape can't otherwise make two interior lines read
/// as different once normalized).
///
/// A hunk like this can never be resolved by copying line *text* — both
/// sides already agree on that — so [`stage_hunk_copy`] additionally copies
/// the source's trailing-newline state for exactly this case (Issue #315).
/// An ordinary content edit that happens to touch the last line does not
/// qualify (its trimmed text differs) and keeps deferring to the
/// destination's own trailing newline, per [`splice_buffer`]'s contract.
fn hunk_is_pure_newline_diff(diff_rows: &[DiffRow], row_range: &std::ops::Range<usize>) -> bool {
    row_range.end == diff_rows.len()
        && !row_range.is_empty()
        && diff_rows[row_range.clone()]
            .iter()
            .all(|row| match (&row.left, &row.right) {
                (Some(l), Some(r)) => {
                    l.text != r.text
                        && l.text.trim_end_matches(['\r', '\n'])
                            == r.text.trim_end_matches(['\r', '\n'])
                }
                _ => false,
            })
}

/// Splice a single hunk from one working buffer into the other, in memory.
///
/// Nothing is written: `[` / `]` stage an edit that only an explicit save
/// commits, so both sides can be dirty at once and each further hunk operation
/// reads the latest buffers (Issue #235).
///
/// Returns whether the destination buffer actually changed. It can come back
/// `false` when nothing distinguishes the copied text from what the
/// destination already held — callers use this to avoid reporting a stage
/// that did nothing as if it succeeded.
pub fn stage_hunk_copy(
    left: &mut TextBuffer,
    right: &mut TextBuffer,
    diff_rows: &[DiffRow],
    hunk_index: usize,
    direction: HunkCopyDirection,
) -> Result<bool, std::io::Error> {
    let hunks = diff_hunk_row_ranges(diff_rows);
    let row_range = hunks
        .get(hunk_index)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid hunk index"))?
        .clone();
    let pure_newline_diff = hunk_is_pure_newline_diff(diff_rows, &row_range);
    let indices = diff_row_file_line_indices(diff_rows);
    let left_range = hunk_side_line_range(&indices, row_range.clone(), Side::Left);
    let right_range = hunk_side_line_range(&indices, row_range.clone(), Side::Right);
    let last_row = &diff_rows[row_range.end - 1];

    let changed = match direction {
        HunkCopyDirection::LeftToRight => {
            let source = extract_hunk_lines(diff_rows, row_range, Side::Left);
            let dest = right_range.unwrap_or_else(|| {
                let pos = left_range.as_ref().map(|r| r.start).unwrap_or(0);
                pos..pos
            });
            let before = right.clone();
            splice_buffer(right, dest, source);
            if pure_newline_diff {
                if let Some(line) = &last_row.left {
                    right.trailing_newline = line.text.ends_with(['\n', '\r']);
                }
            }
            *right != before
        }
        HunkCopyDirection::RightToLeft => {
            let source = extract_hunk_lines(diff_rows, row_range, Side::Right);
            let dest = left_range.unwrap_or_else(|| {
                let pos = right_range.as_ref().map(|r| r.start).unwrap_or(0);
                pos..pos
            });
            let before = left.clone();
            splice_buffer(left, dest, source);
            if pure_newline_diff {
                if let Some(line) = &last_row.right {
                    left.trailing_newline = line.text.ends_with(['\n', '\r']);
                }
            }
            *left != before
        }
    };
    Ok(changed)
}

/// Splice `replacement` over `range` in `buffer`, keeping its line ending and
/// final-newline state. A buffer that gains its first lines adopts a trailing
/// newline; one emptied out loses it, so empty-file semantics round-trip.
fn splice_buffer(buffer: &mut TextBuffer, range: std::ops::Range<usize>, replacement: Vec<String>) {
    let was_empty = buffer.lines.is_empty();
    let start = range.start.min(buffer.lines.len());
    let end = range.end.min(buffer.lines.len());
    buffer.lines.splice(start..end, replacement);
    if buffer.lines.is_empty() {
        buffer.trailing_newline = false;
    } else if was_empty {
        buffer.trailing_newline = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_view::{compare_texts, DiffLine};
    use similar::ChangeTag;

    #[test]
    fn test_stage_hunk_copy_left_to_right_replaces_one_block() {
        let mut left = TextBuffer::from_text("alpha\nleft-only\ngamma\n");
        let mut right = TextBuffer::from_text("alpha\nright-only\ngamma\n");
        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        assert_eq!(diff_hunk_row_ranges(&rows).len(), 1);

        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();

        assert_eq!(right.to_text(), "alpha\nleft-only\ngamma\n");
        assert_eq!(
            left.to_text(),
            "alpha\nleft-only\ngamma\n",
            "the source side is untouched"
        );
    }

    #[test]
    fn test_stage_hunk_copy_right_to_left_inserts_missing_block() {
        let mut left = TextBuffer::from_text("keep\n");
        let mut right = TextBuffer::from_text("keep\nfrom-right\n");
        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);

        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::RightToLeft,
        )
        .unwrap();

        assert_eq!(left.to_text(), "keep\nfrom-right\n");
    }

    /// Issue #235: staging must preserve the destination's byte-level shape, so
    /// the preview matches what a save writes.
    #[test]
    fn test_stage_hunk_copy_preserves_line_endings_and_final_newline() {
        // CRLF destination without a trailing newline.
        let mut left = TextBuffer::from_text("keep\nnew-line\n");
        let mut right = TextBuffer::from_text("keep\r\nold-line");
        assert_eq!(right.line_ending, "\r\n");
        assert!(!right.trailing_newline);

        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();

        assert_eq!(right.line_ending, "\r\n");
        assert!(!right.trailing_newline);
        assert_eq!(right.to_text(), "keep\r\nnew-line");
    }

    /// Issue #315: two files whose only difference is a missing trailing
    /// newline at EOF show a change block that line-text copying alone can
    /// never resolve, since both sides already agree on the text. Staging it
    /// must additionally adopt the source's trailing-newline state.
    #[test]
    fn test_stage_hunk_copy_resolves_a_pure_trailing_newline_difference() {
        let mut left = TextBuffer::from_text("keep\n```");
        let mut right = TextBuffer::from_text("keep\n```\n");
        assert!(!left.trailing_newline);
        assert!(right.trailing_newline);

        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        let changed = stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::RightToLeft,
        )
        .unwrap();

        assert!(changed, "adopting the trailing newline is a real change");
        assert!(left.trailing_newline);
        assert_eq!(left.to_text(), right.to_text());
    }

    /// An ordinary content edit that happens to touch the last line must keep
    /// deferring to the destination's own trailing newline (unlike the
    /// pure-newline case above) — copying real text stays governed by
    /// `test_stage_hunk_copy_preserves_line_endings_and_final_newline`.
    #[test]
    fn test_stage_hunk_copy_pure_newline_fix_does_not_apply_to_real_edits() {
        let mut left = TextBuffer::from_text("keep\nnew-line\n");
        let mut right = TextBuffer::from_text("keep\r\nold-line");

        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();

        assert!(
            !right.trailing_newline,
            "a real text edit must not also flip EOF shape"
        );
    }

    /// [`stage_hunk_copy`] reports no change when the splice produces bytes
    /// identical to what the destination already held, so a caller never
    /// claims to have staged something it did not (Issue #315).
    #[test]
    fn test_stage_hunk_copy_reports_no_change_for_a_true_no_op() {
        let mut left = TextBuffer::from_text("same\n");
        let mut right = TextBuffer::from_text("same\n");
        // A synthetic row the diff engine would never actually produce (both
        // sides hold the exact same text), used to exercise the general
        // change-detection safety net directly.
        let rows = vec![DiffRow::content(
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "same\n".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "same\n".to_string(),
            }),
            Some(0),
            Some(0),
        )];

        let changed = stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();

        assert!(!changed);
    }

    /// Issue #235: an emptied buffer loses its trailing newline, and a buffer
    /// that gains its first lines takes one on.
    #[test]
    fn test_stage_hunk_copy_handles_empty_file_semantics() {
        let mut left = TextBuffer::from_text("");
        let mut right = TextBuffer::from_text("only\n");
        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::RightToLeft,
        )
        .unwrap();
        assert_eq!(left.to_text(), "only\n");

        let mut left = TextBuffer::from_text("only\n");
        let mut right = TextBuffer::from_text("");
        let rows = compare_texts(&left.to_text(), &right.to_text(), true, 3);
        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            0,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();
        assert_eq!(right.to_text(), "only\n");
    }

    /// Issue #241: staging the later hunk in a collapsed view splices the
    /// absolute source range, not a count of visible rows.
    #[test]
    fn test_stage_hunk_copy_collapsed_targets_absolute_source_range() {
        let mut left_lines = Vec::new();
        let mut right_lines = Vec::new();
        for i in 0..30 {
            if i == 5 {
                left_lines.push("left-a");
                right_lines.push("right-a");
            } else if i == 24 {
                left_lines.push("left-b");
                right_lines.push("right-b");
            } else {
                left_lines.push("same");
                right_lines.push("same");
            }
        }
        let mut left = TextBuffer::from_text(&(left_lines.join("\n") + "\n"));
        let mut right = TextBuffer::from_text(&(right_lines.join("\n") + "\n"));
        let rows = compare_texts(&left.to_text(), &right.to_text(), false, 1);
        assert_eq!(diff_hunk_row_ranges(&rows).len(), 2);

        stage_hunk_copy(
            &mut left,
            &mut right,
            &rows,
            1,
            HunkCopyDirection::LeftToRight,
        )
        .unwrap();

        assert!(
            right.lines[24] == "left-b",
            "second hunk must land on source line 25, got {:?}",
            right.lines
        );
        assert_eq!(
            right.lines[5], "right-a",
            "the first hunk must stay untouched"
        );
    }
}
