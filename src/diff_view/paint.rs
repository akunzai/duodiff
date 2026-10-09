//! Painting helpers for the File Diff panes: gutters, change markers, and
//! intraline highlighting.

use super::{DiffLine, DiffRow};
use crate::side::Side;
use similar::{ChangeTag, TextDiff};

/// Text columns that must remain after the full gutter; otherwise hide the
/// line number and separator but keep the `+` / `-` / `…` marker.
const MIN_DIFF_TEXT_COLUMNS: usize = 8;

/// Fixed per-pane gutter: right-aligned source line number, marker, separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffGutter {
    pub(crate) show_numbers: bool,
    pub(crate) number_width: usize,
    /// Columns occupied by the gutter, including trailing space before text.
    pub(crate) width: usize,
}

/// Change marker drawn in the gutter. Independent of colour (Issue #241).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffMarker {
    Blank,
    Delete,
    Insert,
    Gap,
}

impl DiffMarker {
    pub(crate) fn as_char(self) -> char {
        match self {
            Self::Blank => ' ',
            Self::Delete => '-',
            Self::Insert => '+',
            Self::Gap => '…',
        }
    }
}

/// Gutter for a pane whose file has `line_count` lines and `pane_inner_width`
/// columns inside the border.
pub(crate) fn diff_gutter(line_count: usize, pane_inner_width: usize) -> DiffGutter {
    let number_width = line_count.max(1).to_string().len();
    let full_width = number_width + 5;
    if pane_inner_width.saturating_sub(full_width) < MIN_DIFF_TEXT_COLUMNS {
        DiffGutter {
            show_numbers: false,
            number_width: 0,
            width: 2,
        }
    } else {
        DiffGutter {
            show_numbers: true,
            number_width,
            width: full_width,
        }
    }
}

pub(crate) fn diff_marker_for_side(row: &DiffRow, side: Side) -> DiffMarker {
    if row.omitted {
        return DiffMarker::Gap;
    }
    let line = match side {
        Side::Left => &row.left,
        Side::Right => &row.right,
    };
    match line.as_ref().map(|l| l.tag) {
        Some(ChangeTag::Delete) => DiffMarker::Delete,
        Some(ChangeTag::Insert) => DiffMarker::Insert,
        _ => DiffMarker::Blank,
    }
}

/// Render the gutter prefix for one physical row. `source_line` is 0-based.
pub(crate) fn format_diff_gutter(
    gutter: DiffGutter,
    source_line: Option<usize>,
    marker: DiffMarker,
    continuation: bool,
) -> String {
    if !gutter.show_numbers {
        if continuation {
            return "  ".to_string();
        }
        return format!("{} ", marker.as_char());
    }
    let number = match (continuation, source_line) {
        (true, _) | (_, None) => " ".repeat(gutter.number_width),
        (_, Some(idx)) => format!("{:>width$}", idx + 1, width = gutter.number_width),
    };
    let mark = if continuation { ' ' } else { marker.as_char() };
    format!("{number} {mark} │ ")
}

/// True when a row is a side-by-side replacement (delete on left, insert on right).
pub(crate) fn is_replacement_pair(
    left_line: &Option<DiffLine>,
    right_line: &Option<DiffLine>,
) -> bool {
    matches!(
        (
            left_line.as_ref().map(|l| l.tag),
            right_line.as_ref().map(|r| r.tag)
        ),
        (Some(ChangeTag::Delete), Some(ChangeTag::Insert))
    )
}

/// Per-character mask for intraline highlighting on a replacement line.
/// `true` marks characters that differ from the paired side.
pub(crate) fn intraline_change_mask(text: &str, other: &str, side: Side) -> Vec<bool> {
    let diff = TextDiff::from_chars(text, other);
    let mut mask = Vec::new();
    for change in diff.iter_all_changes() {
        match (side, change.tag()) {
            (Side::Left, ChangeTag::Insert) | (Side::Right, ChangeTag::Delete) => continue,
            (Side::Left, ChangeTag::Delete) | (Side::Right, ChangeTag::Insert) => {
                mask.extend(std::iter::repeat_n(true, change.value().chars().count()));
            }
            (_, ChangeTag::Equal) => {
                mask.extend(std::iter::repeat_n(false, change.value().chars().count()));
            }
        }
    }

    let char_count = text.chars().count();
    mask.truncate(char_count);
    mask.resize(char_count, false);
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_view::diff_text_width;
    use crate::diff_view::tests::{line, pair};

    #[test]
    fn test_is_replacement_pair() {
        let delete = Some(DiffLine {
            tag: ChangeTag::Delete,
            text: "old".to_string(),
        });
        let insert = Some(DiffLine {
            tag: ChangeTag::Insert,
            text: "new".to_string(),
        });
        let equal = Some(DiffLine {
            tag: ChangeTag::Equal,
            text: "same".to_string(),
        });

        assert!(is_replacement_pair(&delete, &insert));
        assert!(!is_replacement_pair(&delete, &None));
        assert!(!is_replacement_pair(&equal, &insert));
    }

    #[test]
    fn test_intraline_change_mask_highlights_only_changed_chars() {
        let left = "let foo = 1;";
        let right = "let bar = 1;";
        let left_mask = intraline_change_mask(left, right, Side::Left);
        let right_mask = intraline_change_mask(right, left, Side::Right);

        assert_eq!(left_mask.len(), left.chars().count());
        assert_eq!(right_mask.len(), right.chars().count());

        let left_chars: Vec<char> = left.chars().collect();
        let highlighted_left: String = left_chars
            .iter()
            .zip(left_mask.iter())
            .filter_map(|(ch, hi)| if *hi { Some(*ch) } else { None })
            .collect();
        assert_eq!(highlighted_left, "foo");

        let right_chars: Vec<char> = right.chars().collect();
        let highlighted_right: String = right_chars
            .iter()
            .zip(right_mask.iter())
            .filter_map(|(ch, hi)| if *hi { Some(*ch) } else { None })
            .collect();
        assert_eq!(highlighted_right, "bar");
    }

    #[test]
    fn test_diff_gutter_width_follows_line_count() {
        let g = diff_gutter(9, 40);
        assert!(g.show_numbers);
        assert_eq!(g.number_width, 1);
        assert_eq!(g.width, 6);
        let g = diff_gutter(100, 40);
        assert_eq!(g.number_width, 3);
        assert_eq!(g.width, 8);
    }

    #[test]
    fn test_diff_gutter_width_is_independent_per_side() {
        let left = diff_gutter(9, 40);
        let right = diff_gutter(1000, 40);
        assert_eq!(left.number_width, 1);
        assert_eq!(right.number_width, 4);
        assert_eq!(
            format_diff_gutter(left, Some(8), DiffMarker::Blank, false),
            "9   │ "
        );
        assert_eq!(
            format_diff_gutter(right, Some(999), DiffMarker::Blank, false),
            "1000   │ "
        );
        assert_eq!(diff_text_width(40, 9, 1000), 31);
    }

    #[test]
    fn test_diff_gutter_hides_numbers_when_fewer_than_eight_text_columns() {
        let g = diff_gutter(1000, 16);
        assert!(!g.show_numbers);
        assert_eq!(g.width, 2);
        let g = diff_gutter(1000, 17);
        assert!(g.show_numbers);
        assert_eq!(g.width, 9);
    }

    #[test]
    fn test_format_diff_gutter_markers_and_blank_empty_side() {
        let g = diff_gutter(42, 40);
        assert_eq!(
            format_diff_gutter(g, Some(41), DiffMarker::Delete, false),
            "42 - │ "
        );
        assert_eq!(
            format_diff_gutter(g, Some(42), DiffMarker::Blank, false),
            "43   │ "
        );
        assert_eq!(
            format_diff_gutter(g, None, DiffMarker::Insert, false),
            "   + │ "
        );
    }

    #[test]
    fn test_format_diff_gutter_gap_and_wrapped_continuation() {
        let g = diff_gutter(42, 40);
        assert_eq!(
            format_diff_gutter(g, None, DiffMarker::Gap, false),
            "   … │ "
        );
        assert_eq!(
            format_diff_gutter(g, Some(41), DiffMarker::Delete, true),
            "     │ "
        );
    }

    #[test]
    fn test_format_diff_gutter_narrow_keeps_marker() {
        let g = diff_gutter(1000, 16);
        assert!(!g.show_numbers);
        assert_eq!(
            format_diff_gutter(g, Some(0), DiffMarker::Delete, false),
            "- "
        );
        assert_eq!(
            format_diff_gutter(g, Some(0), DiffMarker::Delete, true),
            "  "
        );
        assert_eq!(format_diff_gutter(g, None, DiffMarker::Gap, false), "… ");
    }

    #[test]
    fn test_diff_marker_for_side_matches_gutter_semantics() {
        let equal = pair(
            Some(line(ChangeTag::Equal, "ctx")),
            Some(line(ChangeTag::Equal, "ctx")),
        );
        assert_eq!(diff_marker_for_side(&equal, Side::Left), DiffMarker::Blank);
        assert_eq!(diff_marker_for_side(&equal, Side::Right), DiffMarker::Blank);

        let replace = pair(
            Some(line(ChangeTag::Delete, "old")),
            Some(line(ChangeTag::Insert, "new")),
        );
        assert_eq!(
            diff_marker_for_side(&replace, Side::Left),
            DiffMarker::Delete
        );
        assert_eq!(
            diff_marker_for_side(&replace, Side::Right),
            DiffMarker::Insert
        );

        let delete = pair(Some(line(ChangeTag::Delete, "gone")), None);
        assert_eq!(
            diff_marker_for_side(&delete, Side::Left),
            DiffMarker::Delete
        );
        assert_eq!(
            diff_marker_for_side(&delete, Side::Right),
            DiffMarker::Blank
        );

        let insert = pair(None, Some(line(ChangeTag::Insert, "added")));
        assert_eq!(diff_marker_for_side(&insert, Side::Left), DiffMarker::Blank);
        assert_eq!(
            diff_marker_for_side(&insert, Side::Right),
            DiffMarker::Insert
        );

        assert_eq!(
            diff_marker_for_side(&DiffRow::omitted_gap(), Side::Left),
            DiffMarker::Gap
        );
        assert_eq!(
            diff_marker_for_side(&DiffRow::omitted_gap(), Side::Right),
            DiffMarker::Gap
        );
    }
}
