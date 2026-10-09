//! The rows of the built-in side-by-side diff: how two texts align, where
//! each row lands once wrapped, and where the change hunks are.

pub(crate) mod paint;
pub(crate) mod staging;

use crate::side::Side;
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub text: String,
}

/// One aligned pair of the built-in side-by-side diff.
///
/// `left_source` / `right_source` are 0-based indices into the old/new files
/// (`None` when that side is empty or the row is an omitted-range gap).
/// Rendering, hunk navigation, and hunk staging all read these fields rather
/// than counting visible rows (Issue #241).
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
    pub left_source: Option<usize>,
    pub right_source: Option<usize>,
    /// Collapsed-view placeholder for an omitted equal range. Shown as `…`
    /// with no line numbers; never a change hunk.
    pub omitted: bool,
}

impl From<(Option<DiffLine>, Option<DiffLine>)> for DiffRow {
    fn from((left, right): (Option<DiffLine>, Option<DiffLine>)) -> Self {
        Self {
            left,
            right,
            left_source: None,
            right_source: None,
            omitted: false,
        }
    }
}

impl DiffRow {
    pub(crate) fn content(
        left: Option<DiffLine>,
        right: Option<DiffLine>,
        left_source: Option<usize>,
        right_source: Option<usize>,
    ) -> Self {
        Self {
            left,
            right,
            left_source,
            right_source,
            omitted: false,
        }
    }

    pub(crate) fn omitted_gap() -> Self {
        Self {
            left: None,
            right: None,
            left_source: None,
            right_source: None,
            omitted: true,
        }
    }
}

/// Shared text width both panes wrap and scroll against: pane inner width
/// minus the wider of the two per-side gutters, so geometry, wrapping, and
/// rendering agree (Issue #241).
pub fn diff_text_width(
    pane_inner_width: usize,
    left_line_count: usize,
    right_line_count: usize,
) -> usize {
    let left = paint::diff_gutter(left_line_count, pane_inner_width);
    let right = paint::diff_gutter(right_line_count, pane_inner_width);
    pane_inner_width.saturating_sub(left.width.max(right.width))
}

/// Highest 1-based source line on `side`, or the count of visible lines
/// when rows were built without source metadata (test fixtures).
pub fn diff_side_line_count(diff_rows: &[DiffRow], side: Side) -> usize {
    let max_source = diff_rows
        .iter()
        .filter_map(|row| match side {
            Side::Left => row.left_source,
            Side::Right => row.right_source,
        })
        .max();
    if let Some(idx) = max_source {
        return idx + 1;
    }
    diff_rows
        .iter()
        .filter(|row| {
            !row.omitted
                && match side {
                    Side::Left => row.left.is_some(),
                    Side::Right => row.right.is_some(),
                }
        })
        .count()
}

fn process_op(
    diff: &similar::TextDiff<'_, '_, str>,
    op: &similar::DiffOp,
    rows: &mut Vec<DiffRow>,
) {
    let changes: Vec<_> = diff.iter_changes(op).collect();
    let deletes: Vec<_> = changes
        .iter()
        .filter(|c| c.tag() == ChangeTag::Delete)
        .collect();
    let inserts: Vec<_> = changes
        .iter()
        .filter(|c| c.tag() == ChangeTag::Insert)
        .collect();

    if deletes.is_empty() && inserts.is_empty() {
        // Equal changes only
        for change in changes {
            let line_content = change.value().to_string();
            rows.push(DiffRow::content(
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: line_content.clone(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: line_content,
                }),
                change.old_index(),
                change.new_index(),
            ));
        }
    } else if !deletes.is_empty() && inserts.is_empty() {
        // Delete changes only
        for change in deletes {
            rows.push(DiffRow::content(
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: change.value().to_string(),
                }),
                None,
                change.old_index(),
                None,
            ));
        }
    } else if deletes.is_empty() && !inserts.is_empty() {
        // Insert changes only
        for change in inserts {
            rows.push(DiffRow::content(
                None,
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: change.value().to_string(),
                }),
                None,
                change.new_index(),
            ));
        }
    } else {
        // Replace: both deletes and inserts exist -> align side-by-side!
        let max_len = std::cmp::max(deletes.len(), inserts.len());
        for i in 0..max_len {
            let left = if i < deletes.len() {
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: deletes[i].value().to_string(),
                })
            } else {
                None
            };
            let right = if i < inserts.len() {
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: inserts[i].value().to_string(),
                })
            } else {
                None
            };
            rows.push(DiffRow::content(
                left,
                right,
                deletes.get(i).and_then(|c| c.old_index()),
                inserts.get(i).and_then(|c| c.new_index()),
            ));
        }
    }
}

/// Diff two already-loaded texts, so the File Diff view can re-diff staged
/// working buffers without touching the filesystem (Issue #235).
pub fn compare_texts(
    left_text: &str,
    right_text: &str,
    full_context: bool,
    context: usize,
) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(left_text, right_text);
    let mut rows = Vec::new();

    if full_context {
        for op in diff.ops() {
            process_op(&diff, op, &mut rows);
        }
    } else {
        let mut last_old = 0usize;
        let mut last_new = 0usize;
        let groups = diff.grouped_ops(context);
        for group in &groups {
            let Some(first) = group.first() else {
                continue;
            };
            if first.old_range().start > last_old || first.new_range().start > last_new {
                rows.push(DiffRow::omitted_gap());
            }
            for op in group {
                process_op(&diff, op, &mut rows);
                last_old = op.old_range().end;
                last_new = op.new_range().end;
            }
        }
        if !groups.is_empty() && (last_old < diff.old_len() || last_new < diff.new_len()) {
            rows.push(DiffRow::omitted_gap());
        }
    }
    rows
}

/// True when either side of a diff row is a delete or insert (not equal-only).
pub fn diff_row_is_change(row: &DiffRow) -> bool {
    if row.omitted {
        return false;
    }
    row.left.as_ref().map(|l| l.tag) == Some(ChangeTag::Delete)
        || row.right.as_ref().map(|r| r.tag) == Some(ChangeTag::Insert)
}

/// Rows `row` takes once each side wraps at `content_width`: the taller
/// side's count, and 1 when not wrapping.
fn row_physical_count(row: &DiffRow, content_width: usize, wrap: bool) -> usize {
    if !wrap {
        return 1;
    }
    let count = |line: &Option<DiffLine>| {
        line.as_ref().map_or(1, |line| {
            crate::wrap::line_count(line.text.trim_end(), content_width)
        })
    };
    count(&row.left).max(count(&row.right))
}

/// Where each row of a File Diff starts once wrapped, with the change hunks
/// and the longest line: what scrolling, the jumps between changes, the
/// active hunk, and painting all read, so they agree on one set of numbers.
///
/// Built when the rows change or when they wrap differently; reading it
/// never walks the rows again.
#[derive(Clone, Debug)]
pub struct RowIndex {
    /// The physical row each logical row starts at, then the total.
    starts: Vec<usize>,
    hunks: Vec<std::ops::Range<usize>>,
    max_line_width: usize,
    content_width: usize,
    wrap: bool,
}

impl Default for RowIndex {
    /// The index of no rows.
    fn default() -> Self {
        Self::new(&[], 0, false)
    }
}

impl RowIndex {
    pub fn new(rows: &[DiffRow], content_width: usize, wrap: bool) -> Self {
        let mut starts = Vec::with_capacity(rows.len() + 1);
        let mut physical = 0usize;
        let mut max_line_width = 0usize;
        for row in rows {
            starts.push(physical);
            physical += row_physical_count(row, content_width, wrap);
            for line in [&row.left, &row.right].into_iter().flatten() {
                max_line_width =
                    max_line_width.max(crate::wrap::display_width(line.text.trim_end()));
            }
        }
        starts.push(physical);
        Self {
            starts,
            hunks: diff_hunk_row_ranges(rows),
            max_line_width,
            content_width,
            wrap,
        }
    }

    /// Whether this index was built for rows wrapped this way.
    pub fn is_for(&self, content_width: usize, wrap: bool) -> bool {
        self.wrap == wrap && (!wrap || self.content_width == content_width)
    }

    /// Physical rows all the rows take.
    pub fn physical_rows(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// Longest line, in display columns, on either side.
    pub fn max_line_width(&self) -> usize {
        self.max_line_width
    }

    /// The change hunks, as ranges of logical rows.
    pub fn hunks(&self) -> &[std::ops::Range<usize>] {
        &self.hunks
    }

    /// The physical row logical row `row` starts at.
    pub fn start(&self, row: usize) -> usize {
        self.starts[row.min(self.starts.len() - 1)]
    }

    /// The logical row painted on physical row `physical`; past the end, the
    /// last row.
    pub fn row_at(&self, physical: usize) -> usize {
        let rows = self.starts.len() - 1;
        self.starts[..rows]
            .partition_point(|&start| start <= physical)
            .saturating_sub(1)
    }

    /// The logical rows `height` physical rows from `scroll` show, and how
    /// many of the first one's physical rows are scrolled off the top.
    pub fn window(&self, scroll: usize, height: usize) -> (std::ops::Range<usize>, usize) {
        let rows = self.starts.len() - 1;
        if rows == 0 || height == 0 {
            return (0..0, 0);
        }
        let first = self.row_at(scroll);
        let last = self.row_at(scroll + height - 1);
        (first..last + 1, scroll.saturating_sub(self.starts[first]))
    }

    /// The hunk under physical row `physical`: the one it is in, else the
    /// next one, else the last.
    pub fn hunk_at(&self, physical: usize) -> Option<usize> {
        if self.hunks.is_empty() {
            return None;
        }
        let row = self.row_at(physical);
        let next = self.hunks.partition_point(|hunk| hunk.end <= row);
        Some(next.min(self.hunks.len() - 1))
    }

    /// [`RowIndex::hunk_at`] as the logical rows that hunk covers.
    pub fn hunk_rows_at(&self, physical: usize) -> Option<std::ops::Range<usize>> {
        self.hunk_at(physical).map(|hunk| self.hunks[hunk].clone())
    }

    /// The physical row of the next (`forward`) or previous changed row from
    /// `current`, wrapping around at either end.
    pub fn jump(&self, current: usize, forward: bool) -> Option<usize> {
        let mut changes = self
            .hunks
            .iter()
            .flat_map(|hunk| hunk.clone())
            .map(|row| self.starts[row]);
        if forward {
            let first = changes.next()?;
            if first > current {
                return Some(first);
            }
            Some(changes.find(|&start| start > current).unwrap_or(first))
        } else {
            let all: Vec<usize> = changes.collect();
            all.iter()
                .rfind(|&&start| start < current)
                .or(all.last())
                .copied()
        }
    }
}

/// Contiguous row-index ranges in `diff_rows` that form change hunks.
pub fn diff_hunk_row_ranges(diff_rows: &[DiffRow]) -> Vec<std::ops::Range<usize>> {
    let mut hunks = Vec::new();
    let mut i = 0;
    while i < diff_rows.len() {
        if !diff_row_is_change(&diff_rows[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < diff_rows.len() && diff_row_is_change(&diff_rows[i]) {
            i += 1;
        }
        hunks.push(start..i);
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{LoadedText, TextRejection};
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    pub(super) fn line(tag: ChangeTag, text: &str) -> DiffLine {
        DiffLine {
            tag,
            text: text.to_string(),
        }
    }

    pub(super) fn pair(left: Option<DiffLine>, right: Option<DiffLine>) -> DiffRow {
        DiffRow::from((left, right))
    }

    /// Decode two files as File Diff does and diff them.
    fn compare_files(
        left: &Path,
        right: &Path,
        full_context: bool,
        context: usize,
    ) -> Result<Vec<DiffRow>, TextRejection> {
        let left = LoadedText::from_bytes(fs::read(left).unwrap())?.text;
        let right = LoadedText::from_bytes(fs::read(right).unwrap())?.text;
        Ok(compare_texts(&left, &right, full_context, context))
    }

    #[test]
    fn test_compare_files_basic() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();

        writeln!(left_file, "hello\nworld\nfoo").unwrap();
        writeln!(right_file, "hello\nbar\nfoo").unwrap();

        let rows = compare_files(left_file.path(), right_file.path(), false, 3).unwrap();

        // Let's assert we have the changes
        assert!(!rows.is_empty());

        // Verify that the replacement of "world" with "bar" is aligned side-by-side
        let has_aligned_replace = rows.iter().any(|row| {
            row.left
                .as_ref()
                .is_some_and(|l| l.tag == ChangeTag::Delete && l.text.contains("world"))
                && row
                    .right
                    .as_ref()
                    .is_some_and(|r| r.tag == ChangeTag::Insert && r.text.contains("bar"))
        });

        assert!(
            has_aligned_replace,
            "Should contain aligned replace of 'world' with 'bar'"
        );
    }

    #[test]
    fn test_compare_files_ignore_crlf() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();

        writeln!(left_file, "hello\r\nworld\r\nfoo").unwrap();
        writeln!(right_file, "hello\nworld\nfoo").unwrap();

        let rows = compare_files(left_file.path(), right_file.path(), false, 3).unwrap();

        // Since files are identical after CRLF normalization, rows should be empty
        assert!(
            rows.is_empty(),
            "Should be empty when files are identical after CRLF normalization"
        );
    }

    #[test]
    fn test_compare_files_full_context() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();

        writeln!(left_file, "hello\nworld\nfoo").unwrap();
        writeln!(right_file, "hello\nbar\nfoo").unwrap();

        let rows = compare_files(left_file.path(), right_file.path(), true, 3).unwrap();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_compare_files_context_radius_controls_collapsed_line_count() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();

        // 10 lines of equal context around a single changed line in the middle.
        let mut left_lines: Vec<String> = (1..=10).map(|n| format!("line{n}")).collect();
        let mut right_lines = left_lines.clone();
        left_lines[5] = "changed-left".to_string();
        right_lines[5] = "changed-right".to_string();
        writeln!(left_file, "{}", left_lines.join("\n")).unwrap();
        writeln!(right_file, "{}", right_lines.join("\n")).unwrap();

        let narrow = compare_files(left_file.path(), right_file.path(), false, 1).unwrap();
        let wide = compare_files(left_file.path(), right_file.path(), false, 4).unwrap();

        assert!(
            wide.len() > narrow.len(),
            "a wider context radius should include more surrounding lines: narrow={}, wide={}",
            narrow.len(),
            wide.len()
        );
    }

    #[test]
    fn test_compare_files_rejects_when_one_side_binary() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();
        writeln!(left_file, "plain text").unwrap();
        right_file.write_all(b"bin\0ary").unwrap();
        let err = compare_files(left_file.path(), right_file.path(), false, 3).unwrap_err();
        assert!(err.to_string().contains("binary"));
    }

    #[test]
    fn test_jump_skips_equal_regions() {
        let rows = vec![
            pair(
                Some(line(ChangeTag::Equal, "same")),
                Some(line(ChangeTag::Equal, "same")),
            ),
            pair(
                Some(line(ChangeTag::Delete, "old")),
                Some(line(ChangeTag::Insert, "new")),
            ),
            pair(
                Some(line(ChangeTag::Equal, "tail")),
                Some(line(ChangeTag::Equal, "tail")),
            ),
            pair(Some(line(ChangeTag::Delete, "end")), None),
        ];

        assert_eq!(RowIndex::new(&rows, 40, false).jump(0, true), Some(1));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(1, true), Some(3));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(2, true), Some(3));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(3, true), Some(1));

        assert_eq!(RowIndex::new(&rows, 40, false).jump(3, false), Some(1));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(2, false), Some(1));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(1, false), Some(3));
        assert_eq!(RowIndex::new(&rows, 40, false).jump(0, false), Some(3));
    }

    #[test]
    fn test_jump_respects_wrap_physical_rows() {
        let long = "a".repeat(20);
        let rows = vec![
            pair(
                Some(line(ChangeTag::Equal, "ctx")),
                Some(line(ChangeTag::Equal, "ctx")),
            ),
            pair(
                Some(line(ChangeTag::Delete, &long)),
                Some(line(ChangeTag::Insert, &long)),
            ),
        ];

        // width 8 -> 20 chars wrap into 3 physical lines; change starts at offset 1
        assert_eq!(RowIndex::new(&rows, 8, true).jump(0, true), Some(1));
        assert_eq!(RowIndex::new(&rows, 8, true).jump(2, false), Some(1));
    }

    #[test]
    fn test_diff_hunk_row_ranges_groups_contiguous_changes() {
        let rows = vec![
            pair(
                Some(line(ChangeTag::Equal, "ctx")),
                Some(line(ChangeTag::Equal, "ctx")),
            ),
            pair(
                Some(line(ChangeTag::Delete, "old")),
                Some(line(ChangeTag::Insert, "new")),
            ),
            pair(
                Some(line(ChangeTag::Equal, "mid")),
                Some(line(ChangeTag::Equal, "mid")),
            ),
            pair(None, Some(line(ChangeTag::Insert, "added"))),
        ];

        assert_eq!(diff_hunk_row_ranges(&rows), vec![1..2, 3..4]);
    }

    #[test]
    fn test_hunk_at_finds_nearest_change() {
        let rows = vec![
            pair(
                Some(line(ChangeTag::Equal, "ctx")),
                Some(line(ChangeTag::Equal, "ctx")),
            ),
            pair(
                Some(line(ChangeTag::Delete, "old")),
                Some(line(ChangeTag::Insert, "new")),
            ),
        ];

        assert_eq!(RowIndex::new(&rows, 40, false).hunk_at(0), Some(0));
    }

    #[test]
    fn test_hunk_at_after_omitted_range_finds_later_hunk() {
        let mut left = Vec::new();
        let mut right = Vec::new();
        for i in 0..30 {
            if i == 5 {
                left.push("left-a");
                right.push("right-a");
            } else if i == 24 {
                left.push("left-b");
                right.push("right-b");
            } else {
                left.push("same");
                right.push("same");
            }
        }
        let rows = compare_texts(
            &(left.join("\n") + "\n"),
            &(right.join("\n") + "\n"),
            false,
            1,
        );
        let second = rows
            .iter()
            .position(|row| row.left.as_ref().is_some_and(|l| l.text.contains("left-b")))
            .unwrap();
        let index = RowIndex::new(&rows, 40, false);
        assert_eq!(index.hunk_at(index.start(second)), Some(1));
    }

    /// Issue #241: full-file mode keeps the true 0-based source index of every
    /// line, including equal context.
    #[test]
    fn test_compare_texts_full_context_keeps_absolute_source_indices() {
        let rows = compare_texts("hello\nworld\nfoo\n", "hello\nbar\nfoo\n", true, 3);
        let indices: Vec<_> = rows
            .iter()
            .map(|row| (row.left_source, row.right_source))
            .collect();
        assert_eq!(
            indices,
            vec![(Some(0), Some(0)), (Some(1), Some(1)), (Some(2), Some(2))]
        );
        assert!(!rows.iter().any(|row| row.omitted));
    }

    /// Issue #241: collapsed mode keeps absolute indices and inserts a gap row
    /// for the omitted equal range between hunks.
    #[test]
    fn test_compare_texts_collapsed_preserves_absolute_indices_and_gap_rows() {
        let mut left = Vec::new();
        let mut right = Vec::new();
        for i in 0..30 {
            if i == 5 {
                left.push("left-a");
                right.push("right-a");
            } else if i == 24 {
                left.push("left-b");
                right.push("right-b");
            } else {
                left.push("same");
                right.push("same");
            }
        }
        let left_text = left.join("\n") + "\n";
        let right_text = right.join("\n") + "\n";
        let rows = compare_texts(&left_text, &right_text, false, 1);

        let gaps: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.omitted)
            .map(|(i, _)| i)
            .collect();
        assert!(
            gaps.len() >= 3,
            "leading, between-hunk, and trailing omitted ranges each get a gap: {rows:?}"
        );
        let first_change_idx = rows
            .iter()
            .position(|row| row.left.as_ref().is_some_and(|l| l.text.contains("left-a")))
            .unwrap();
        let second_change_idx = rows
            .iter()
            .position(|row| row.left.as_ref().is_some_and(|l| l.text.contains("left-b")))
            .unwrap();
        assert!(
            rows[first_change_idx..second_change_idx]
                .iter()
                .any(|row| row.omitted),
            "a gap must sit between the two hunks"
        );

        let first_change = rows
            .iter()
            .find(|row| row.left.as_ref().is_some_and(|l| l.text.contains("left-a")))
            .expect("first change");
        assert_eq!(first_change.left_source, Some(5));
        assert_eq!(first_change.right_source, Some(5));

        let second_change = rows
            .iter()
            .find(|row| row.left.as_ref().is_some_and(|l| l.text.contains("left-b")))
            .expect("second change");
        assert_eq!(second_change.left_source, Some(24));
        assert_eq!(second_change.right_source, Some(24));
    }

    /// Issue #241: empty sides of insert/delete rows have no source index.
    #[test]
    fn test_compare_texts_insert_and_delete_leave_empty_side_unnumbered() {
        let deleted = compare_texts("keep\ngone\n", "keep\n", true, 3);
        let delete = deleted
            .iter()
            .find(|row| row.left.as_ref().is_some_and(|l| l.text.contains("gone")))
            .expect("delete");
        assert_eq!(delete.left_source, Some(1));
        assert_eq!(delete.right_source, None);
        assert!(delete.right.is_none());

        let inserted = compare_texts("keep\n", "keep\nadded\n", true, 3);
        let insert = inserted
            .iter()
            .find(|row| row.right.as_ref().is_some_and(|r| r.text.contains("added")))
            .expect("insert");
        assert_eq!(insert.left_source, None);
        assert_eq!(insert.right_source, Some(1));
        assert!(insert.left.is_none());
    }

    /// Issue #241: unequal replacement pairs keep each side's own source index.
    #[test]
    fn test_compare_texts_unequal_replacement_aligns_independent_indices() {
        let rows = compare_texts("a\nold1\nold2\nz\n", "a\nnew1\nz\n", true, 3);
        let replacements: Vec<_> = rows
            .iter()
            .filter(|row| {
                paint::is_replacement_pair(&row.left, &row.right) || diff_row_is_change(row)
            })
            .collect();
        assert!(
            replacements.len() >= 2,
            "two deleted lines against one insert: {rows:?}"
        );
        assert_eq!(replacements[0].left_source, Some(1));
        assert_eq!(replacements[0].right_source, Some(1));
        assert_eq!(replacements[1].left_source, Some(2));
        assert_eq!(replacements[1].right_source, None);
    }

    #[test]
    fn test_physical_rows_wrap_cjk_by_display_width() {
        let rows = vec![pair(
            Some(line(ChangeTag::Equal, "中中中中")),
            Some(line(ChangeTag::Equal, "中中中中")),
        )];
        assert_eq!(RowIndex::new(&rows, 4, true).physical_rows(), 2);
        assert_eq!(RowIndex::new(&rows, 8, true).physical_rows(), 1);
        assert_eq!(RowIndex::new(&rows, 0, false).max_line_width(), 8);
    }

    #[test]
    fn test_diff_max_line_width_uses_unicode_display_width() {
        let rows = vec![pair(Some(line(ChangeTag::Equal, "中中")), None)];
        assert_eq!(RowIndex::new(&rows, 0, false).max_line_width(), 4);
    }
}
