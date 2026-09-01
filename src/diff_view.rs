use similar::{ChangeTag, TextDiff};
use std::fs;
use std::io::Read;
use std::path::Path;

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

/// Text columns that must remain after the full gutter; otherwise hide the
/// line number and separator but keep the `+` / `-` / `…` marker.
pub const MIN_DIFF_TEXT_COLUMNS: usize = 8;

/// Fixed per-pane gutter: right-aligned source line number, marker, separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffGutter {
    pub show_numbers: bool,
    pub number_width: usize,
    /// Columns occupied by the gutter, including trailing space before text.
    pub width: usize,
}

/// Change marker drawn in the gutter. Independent of colour (Issue #241).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMarker {
    Blank,
    Delete,
    Insert,
    Gap,
}

impl DiffMarker {
    pub fn as_char(self) -> char {
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
pub fn diff_gutter(line_count: usize, pane_inner_width: usize) -> DiffGutter {
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

/// Shared text width both panes wrap and scroll against: pane inner width
/// minus the wider of the two per-side gutters, so geometry, wrapping, and
/// rendering agree (Issue #241).
pub fn diff_text_width(
    pane_inner_width: usize,
    left_line_count: usize,
    right_line_count: usize,
) -> usize {
    let left = diff_gutter(left_line_count, pane_inner_width);
    let right = diff_gutter(right_line_count, pane_inner_width);
    pane_inner_width.saturating_sub(left.width.max(right.width))
}

pub fn diff_marker_for_side(row: &DiffRow, left_side: bool) -> DiffMarker {
    if row.omitted {
        return DiffMarker::Gap;
    }
    let line = if left_side { &row.left } else { &row.right };
    match line.as_ref().map(|l| l.tag) {
        Some(ChangeTag::Delete) => DiffMarker::Delete,
        Some(ChangeTag::Insert) => DiffMarker::Insert,
        _ => DiffMarker::Blank,
    }
}

/// Render the gutter prefix for one physical row. `source_line` is 0-based.
pub fn format_diff_gutter(
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

/// Highest 1-based source line on `left_side`, or the count of visible lines
/// when rows were built without source metadata (test fixtures).
pub fn diff_side_line_count(diff_rows: &[DiffRow], left_side: bool) -> usize {
    let max_source = diff_rows
        .iter()
        .filter_map(|row| {
            if left_side {
                row.left_source
            } else {
                row.right_source
            }
        })
        .max();
    if let Some(idx) = max_source {
        return idx + 1;
    }
    diff_rows
        .iter()
        .filter(|row| {
            !row.omitted
                && if left_side {
                    row.left.is_some()
                } else {
                    row.right.is_some()
                }
        })
        .count()
}

/// A file's text as lines plus the byte-level shape needed to write it back
/// unchanged: which line ending it uses and whether it ended with one.
///
/// Staged hunk edits work on this rather than on disk, so the diff a user sees
/// is computed from exactly the bytes a save would write (Issue #235).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBuffer {
    pub lines: Vec<String>,
    /// `"\n"`, `"\r\n"`, or `"\r"`. Defaults to `"\n"` when the text has no
    /// line break to learn from.
    pub line_ending: String,
    /// Whether the text ended with a line break. An empty file has none.
    pub trailing_newline: bool,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            line_ending: "\n".to_string(),
            trailing_newline: false,
        }
    }
}

impl TextBuffer {
    /// Split `text` into lines, remembering its line ending and final-newline
    /// state. The first ending found wins for a mixed-ending file.
    pub fn from_text(text: &str) -> Self {
        let line_ending = if text.contains("\r\n") {
            "\r\n"
        } else if text.contains('\n') {
            "\n"
        } else if text.contains('\r') {
            "\r"
        } else {
            "\n"
        };
        if text.is_empty() {
            return Self {
                lines: Vec::new(),
                line_ending: line_ending.to_string(),
                trailing_newline: false,
            };
        }
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let trailing_newline = normalized.ends_with('\n');
        let mut lines: Vec<String> = normalized.split('\n').map(str::to_string).collect();
        if trailing_newline {
            lines.pop();
        }
        Self {
            lines,
            line_ending: line_ending.to_string(),
            trailing_newline,
        }
    }

    /// Re-render the exact bytes this buffer stands for.
    pub fn to_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut out = self.lines.join(&self.line_ending);
        if self.trailing_newline {
            out.push_str(&self.line_ending);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub fn detect_file_line_ending(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = [0u8; 8192];
    let bytes_read = file.read(&mut buffer).ok()?;
    if bytes_read == 0 {
        return None;
    }
    let chunk = &buffer[..bytes_read];
    let has_lf = chunk.contains(&b'\n');
    let has_cr = chunk.contains(&b'\r');

    if has_cr && has_lf {
        let mut has_crlf = false;
        for i in 0..bytes_read.saturating_sub(1) {
            if chunk[i] == b'\r' && chunk[i + 1] == b'\n' {
                has_crlf = true;
                break;
            }
        }
        if has_crlf {
            Some("CRLF".to_string())
        } else {
            Some("LF".to_string())
        }
    } else if has_lf {
        Some("LF".to_string())
    } else if has_cr {
        Some("CR".to_string())
    } else {
        None
    }
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

/// Maximum size of a single side accepted by the built-in text diff viewer.
/// Larger files should be opened with an external tool.
pub const MAX_DIFF_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

/// Load one side of a file pair for the built-in diff.
///
/// Missing paths and non-files are treated as empty content (one-sided rows).
/// Truncate `path` from the left to at most `max_len` characters, keeping the
/// filename and trailing directories intact and prefixing with `…/` when truncated.
pub fn truncate_path_left(path: &Path, max_len: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.len() <= max_len {
        return path_str.into_owned();
    }
    if max_len <= 1 {
        return "…".to_string();
    }

    let sep = std::path::MAIN_SEPARATOR.to_string();
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|s| !s.is_empty() && s != &sep)
        .collect();

    if components.is_empty() {
        let tail: String = path_str
            .chars()
            .rev()
            .take(max_len.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return format!("…{tail}");
    }

    let last = &components[components.len() - 1];
    let prefix = format!("…{sep}");
    if last.len() + prefix.len() > max_len {
        if max_len <= prefix.len() {
            return "…".chars().take(max_len).collect();
        }
        let avail = max_len - prefix.len();
        let tail: String = last
            .chars()
            .rev()
            .take(avail)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return format!("{prefix}{tail}");
    }

    let mut right_part = last.to_string();
    for comp in components[..components.len() - 1].iter().rev() {
        let candidate = format!("{}{}{}", comp, sep, right_part);
        if candidate.len() + prefix.len() <= max_len {
            right_part = candidate;
        } else {
            break;
        }
    }
    format!("{prefix}{right_part}")
}

/// Existing files that are too large, binary (NUL), or non-UTF-8 return an error
/// so callers can show a status toast instead of a false empty/identical view.
pub fn load_text_for_diff(path: &Path) -> Result<String, std::io::Error> {
    if !path.is_file() {
        return Ok(String::new());
    }

    let meta = fs::metadata(path)?;
    if meta.len() > MAX_DIFF_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "file too large ({} bytes > {} limit): {} (press D for external diff)",
                meta.len(),
                MAX_DIFF_FILE_BYTES,
                truncate_path_left(path, 32)
            ),
        ));
    }

    let mut file = fs::File::open(path)?;
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut buf)?;

    // NUL in the sample strongly indicates binary content.
    let sample_len = buf.len().min(8192);
    if buf[..sample_len].contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "binary file not supported: {} (press D for external diff)",
                truncate_path_left(path, 32)
            ),
        ));
    }

    let text = String::from_utf8(buf).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "non-UTF-8 file not supported: {} (press D for external diff)",
                truncate_path_left(path, 32)
            ),
        )
    })?;
    Ok(text.replace("\r\n", "\n"))
}

pub fn compare_files(
    left: &Path,
    right: &Path,
    full_context: bool,
    context: usize,
) -> Result<Vec<DiffRow>, std::io::Error> {
    let left_text = load_text_for_diff(left)?;
    let right_text = load_text_for_diff(right)?;
    Ok(compare_texts(
        &left_text,
        &right_text,
        full_context,
        context,
    ))
}

/// Diff two already-loaded texts. Split out of [`compare_files`] so the File Diff
/// view can re-diff staged working buffers without touching the filesystem
/// (Issue #235).
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

/// True when a row is a side-by-side replacement (delete on left, insert on right).
pub fn is_replacement_pair(left_line: &Option<DiffLine>, right_line: &Option<DiffLine>) -> bool {
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
pub fn intraline_change_mask(text: &str, other: &str, is_left: bool) -> Vec<bool> {
    let diff = TextDiff::from_chars(text, other);
    let mut mask = Vec::new();
    for change in diff.iter_all_changes() {
        match (is_left, change.tag()) {
            (true, ChangeTag::Insert) | (false, ChangeTag::Delete) => continue,
            (true, ChangeTag::Delete) | (false, ChangeTag::Insert) => {
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

fn logical_row_physical_count(
    left_line: &Option<DiffLine>,
    right_line: &Option<DiffLine>,
    content_width: usize,
    wrap: bool,
) -> usize {
    if !wrap {
        return 1;
    }
    let left_wrapped = left_line
        .as_ref()
        .map(|l| crate::wrap::lines(l.text.trim_end(), content_width))
        .unwrap_or_else(|| vec![String::new()]);
    let right_wrapped = right_line
        .as_ref()
        .map(|r| crate::wrap::lines(r.text.trim_end(), content_width))
        .unwrap_or_else(|| vec![String::new()]);
    std::cmp::max(left_wrapped.len(), right_wrapped.len()).max(1)
}

/// Total physical (post-wrap) rows `diff_rows` occupies at `content_width`.
///
/// Matches what the diff renderer emits, so it can be used to clamp scrolling
/// without running a render pass.
pub fn diff_total_physical_rows(diff_rows: &[DiffRow], content_width: usize, wrap: bool) -> usize {
    diff_rows
        .iter()
        .map(|row| logical_row_physical_count(&row.left, &row.right, content_width, wrap))
        .sum()
}

/// Longest line (in characters) across both sides of `diff_rows`.
pub fn diff_max_line_width(diff_rows: &[DiffRow]) -> usize {
    diff_rows
        .iter()
        .map(|row| {
            let left_width = row
                .left
                .as_ref()
                .map(|l| crate::wrap::display_width(l.text.trim_end()))
                .unwrap_or(0);
            let right_width = row
                .right
                .as_ref()
                .map(|r| r.text.trim_end())
                .map(crate::wrap::display_width)
                .unwrap_or(0);
            left_width.max(right_width)
        })
        .max()
        .unwrap_or(0)
}

/// Physical scroll offsets for the start of each logical `diff_rows` entry.
pub fn diff_row_physical_offsets(
    diff_rows: &[DiffRow],
    content_width: usize,
    wrap: bool,
) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(diff_rows.len());
    let mut physical = 0usize;
    for row in diff_rows {
        offsets.push(physical);
        physical += logical_row_physical_count(&row.left, &row.right, content_width, wrap);
    }
    offsets
}

/// Direction for copying a single change hunk between file sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkCopyDirection {
    LeftToRight,
    RightToLeft,
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

/// Per-row 0-based line indices in the left and right files (`None` when that side is empty).
pub fn diff_row_file_line_indices(diff_rows: &[DiffRow]) -> Vec<(Option<usize>, Option<usize>)> {
    diff_rows
        .iter()
        .map(|row| (row.left_source, row.right_source))
        .collect()
}

fn hunk_side_line_range(
    indices: &[(Option<usize>, Option<usize>)],
    row_range: std::ops::Range<usize>,
    left_side: bool,
) -> Option<std::ops::Range<usize>> {
    let line_nos: Vec<usize> = row_range
        .filter_map(|i| {
            if left_side {
                indices[i].0
            } else {
                indices[i].1
            }
        })
        .collect();
    if line_nos.is_empty() {
        None
    } else {
        Some(*line_nos.first().unwrap()..line_nos.last().unwrap() + 1)
    }
}

fn nearest_change_row(diff_rows: &[DiffRow], logical_row: usize) -> Option<usize> {
    if logical_row < diff_rows.len() && diff_row_is_change(&diff_rows[logical_row]) {
        return Some(logical_row);
    }
    (logical_row..diff_rows.len())
        .find(|&i| diff_row_is_change(&diff_rows[i]))
        .or_else(|| {
            (0..logical_row)
                .rev()
                .find(|&i| diff_row_is_change(&diff_rows[i]))
        })
}

/// Map the current physical scroll offset to a hunk index (nearest change when on context).
pub fn hunk_index_at_scroll(
    diff_rows: &[DiffRow],
    scroll: usize,
    content_width: usize,
    wrap: bool,
) -> Option<usize> {
    if diff_rows.is_empty() {
        return None;
    }
    let offsets = diff_row_physical_offsets(diff_rows, content_width, wrap);
    let logical_row = offsets
        .iter()
        .rposition(|&offset| offset <= scroll)
        .unwrap_or(0);
    let change_row = nearest_change_row(diff_rows, logical_row)?;
    let hunks = diff_hunk_row_ranges(diff_rows);
    hunks.iter().position(|range| range.contains(&change_row))
}

fn extract_hunk_lines(
    diff_rows: &[DiffRow],
    row_range: std::ops::Range<usize>,
    from_left: bool,
) -> Vec<String> {
    diff_rows[row_range]
        .iter()
        .filter_map(|row| {
            let line = if from_left { &row.left } else { &row.right };
            line.as_ref()
                .map(|line| line.text.trim_end_matches(['\r', '\n']).to_string())
        })
        .collect()
}

/// Splice a single hunk from one working buffer into the other, in memory.
///
/// Nothing is written: `[` / `]` stage an edit that only an explicit save
/// commits, so both sides can be dirty at once and each further hunk operation
/// reads the latest buffers (Issue #235).
pub fn stage_hunk_copy(
    left: &mut TextBuffer,
    right: &mut TextBuffer,
    diff_rows: &[DiffRow],
    hunk_index: usize,
    direction: HunkCopyDirection,
) -> Result<(), std::io::Error> {
    let hunks = diff_hunk_row_ranges(diff_rows);
    let row_range = hunks
        .get(hunk_index)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid hunk index"))?
        .clone();
    let indices = diff_row_file_line_indices(diff_rows);
    let left_range = hunk_side_line_range(&indices, row_range.clone(), true);
    let right_range = hunk_side_line_range(&indices, row_range.clone(), false);

    match direction {
        HunkCopyDirection::LeftToRight => {
            let source = extract_hunk_lines(diff_rows, row_range, true);
            let dest = right_range.unwrap_or_else(|| {
                let pos = left_range.as_ref().map(|r| r.start).unwrap_or(0);
                pos..pos
            });
            splice_buffer(right, dest, source);
        }
        HunkCopyDirection::RightToLeft => {
            let source = extract_hunk_lines(diff_rows, row_range, false);
            let dest = left_range.unwrap_or_else(|| {
                let pos = right_range.as_ref().map(|r| r.start).unwrap_or(0);
                pos..pos
            });
            splice_buffer(left, dest, source);
        }
    }
    Ok(())
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

/// Jump `diff_scroll` to the next or previous change block, optionally wrapping around.
pub fn jump_to_change_scroll(
    diff_rows: &[DiffRow],
    current_scroll: usize,
    content_width: usize,
    wrap: bool,
    forward: bool,
) -> Option<usize> {
    let offsets = diff_row_physical_offsets(diff_rows, content_width, wrap);
    let change_offsets: Vec<usize> = diff_rows
        .iter()
        .enumerate()
        .filter(|(i, row)| diff_row_is_change(row) && offsets.get(*i).is_some())
        .map(|(i, _)| offsets[i])
        .collect();

    if change_offsets.is_empty() {
        return None;
    }

    if forward {
        change_offsets
            .iter()
            .find(|&&offset| offset > current_scroll)
            .copied()
            .or_else(|| change_offsets.first().copied())
    } else {
        change_offsets
            .iter()
            .rfind(|&&offset| offset < current_scroll)
            .copied()
            .or_else(|| change_offsets.last().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn line(tag: ChangeTag, text: &str) -> DiffLine {
        DiffLine {
            tag,
            text: text.to_string(),
        }
    }

    fn pair(left: Option<DiffLine>, right: Option<DiffLine>) -> DiffRow {
        DiffRow::from((left, right))
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
    fn test_truncate_path_left() {
        let sample = Path::new("a").join("b").join("c").join("file.txt");
        assert_eq!(truncate_path_left(&sample, 100), sample.to_string_lossy());
        let long_path = Path::new("Users")
            .join("someone")
            .join("deep")
            .join("nested")
            .join("directory")
            .join("structure")
            .join("image.png");
        let truncated = truncate_path_left(&long_path, 25);
        let expected_prefix = format!("…{}", std::path::MAIN_SEPARATOR);
        assert!(
            truncated.starts_with(&expected_prefix),
            "Truncated path should start with '{expected_prefix}': {truncated}"
        );
        assert!(
            truncated.ends_with("image.png"),
            "Truncated path should retain filename: {truncated}"
        );
        assert!(
            truncated.len() <= 25,
            "Truncated path length {} should be <= 25: {truncated}",
            truncated.len()
        );
    }

    #[test]
    fn test_load_text_for_diff_missing_is_empty() {
        let text = load_text_for_diff(Path::new("/nonexistent/duodiff-missing.txt")).unwrap();
        assert!(text.is_empty());
    }

    #[test]
    fn test_load_text_for_diff_rejects_binary() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello\0world").unwrap();
        let err = load_text_for_diff(file.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("binary"),
            "Should mention binary: {err_msg}"
        );
        assert!(
            err_msg.contains("press D for external diff"),
            "Should include actionable hint: {err_msg}"
        );
    }

    #[test]
    fn test_load_text_for_diff_rejects_non_utf8() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0xC3, 0x28]).unwrap(); // invalid UTF-8
        let err = load_text_for_diff(file.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("non-UTF-8"),
            "Should mention non-UTF-8: {err_msg}"
        );
        assert!(
            err_msg.contains("press D for external diff"),
            "Should include actionable hint: {err_msg}"
        );
    }

    #[test]
    fn test_load_text_for_diff_rejects_oversize() {
        let file = NamedTempFile::new().unwrap();
        // Don't actually write 10MiB+; set_len is enough for metadata.len().
        file.as_file().set_len(MAX_DIFF_FILE_BYTES + 1).unwrap();
        let err = load_text_for_diff(file.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("too large"),
            "Should mention too large: {err_msg}"
        );
        assert!(
            err_msg.contains("press D for external diff"),
            "Should include actionable hint: {err_msg}"
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
        let left_mask = intraline_change_mask(left, right, true);
        let right_mask = intraline_change_mask(right, left, false);

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
    fn test_jump_to_change_scroll_skips_equal_regions() {
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

        assert_eq!(jump_to_change_scroll(&rows, 0, 40, false, true), Some(1));
        assert_eq!(jump_to_change_scroll(&rows, 1, 40, false, true), Some(3));
        assert_eq!(jump_to_change_scroll(&rows, 2, 40, false, true), Some(3));
        assert_eq!(jump_to_change_scroll(&rows, 3, 40, false, true), Some(1));

        assert_eq!(jump_to_change_scroll(&rows, 3, 40, false, false), Some(1));
        assert_eq!(jump_to_change_scroll(&rows, 2, 40, false, false), Some(1));
        assert_eq!(jump_to_change_scroll(&rows, 1, 40, false, false), Some(3));
        assert_eq!(jump_to_change_scroll(&rows, 0, 40, false, false), Some(3));
    }

    #[test]
    fn test_jump_to_change_scroll_respects_wrap_physical_rows() {
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
        assert_eq!(jump_to_change_scroll(&rows, 0, 8, true, true), Some(1));
        assert_eq!(jump_to_change_scroll(&rows, 2, 8, true, false), Some(1));
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

    #[test]
    fn test_text_buffer_round_trips_every_shape() {
        for text in ["", "a", "a\n", "a\nb", "a\r\nb\r\n", "a\rb\r", "\n"] {
            assert_eq!(TextBuffer::from_text(text).to_text(), text, "{text:?}");
        }
        assert!(TextBuffer::from_text("").is_empty());
        assert_eq!(TextBuffer::from_text("a\nb").lines, vec!["a", "b"]);
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

    #[test]
    fn test_hunk_index_at_scroll_finds_nearest_change() {
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

        assert_eq!(hunk_index_at_scroll(&rows, 0, 40, false), Some(0));
    }

    #[test]
    fn test_hunk_index_at_scroll_after_omitted_range_finds_later_hunk() {
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
        let offsets = diff_row_physical_offsets(&rows, 40, false);
        assert_eq!(
            hunk_index_at_scroll(&rows, offsets[second], 40, false),
            Some(1)
        );
    }

    #[test]
    fn test_detect_file_line_ending() {
        let mut lf_file = NamedTempFile::new().unwrap();
        let mut crlf_file = NamedTempFile::new().unwrap();

        write!(lf_file, "hello\nworld").unwrap();
        write!(crlf_file, "hello\r\nworld").unwrap();

        assert_eq!(
            detect_file_line_ending(lf_file.path()),
            Some("LF".to_string())
        );
        assert_eq!(
            detect_file_line_ending(crlf_file.path()),
            Some("CRLF".to_string())
        );
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
            .filter(|row| is_replacement_pair(&row.left, &row.right) || diff_row_is_change(row))
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
    fn test_diff_total_physical_rows_wraps_cjk_by_display_width() {
        let rows = vec![pair(
            Some(line(ChangeTag::Equal, "中中中中")),
            Some(line(ChangeTag::Equal, "中中中中")),
        )];
        assert_eq!(diff_total_physical_rows(&rows, 4, true), 2);
        assert_eq!(diff_total_physical_rows(&rows, 8, true), 1);
        assert_eq!(diff_max_line_width(&rows), 8);
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
        assert_eq!(diff_marker_for_side(&equal, true), DiffMarker::Blank);
        assert_eq!(diff_marker_for_side(&equal, false), DiffMarker::Blank);

        let replace = pair(
            Some(line(ChangeTag::Delete, "old")),
            Some(line(ChangeTag::Insert, "new")),
        );
        assert_eq!(diff_marker_for_side(&replace, true), DiffMarker::Delete);
        assert_eq!(diff_marker_for_side(&replace, false), DiffMarker::Insert);

        let delete = pair(Some(line(ChangeTag::Delete, "gone")), None);
        assert_eq!(diff_marker_for_side(&delete, true), DiffMarker::Delete);
        assert_eq!(diff_marker_for_side(&delete, false), DiffMarker::Blank);

        let insert = pair(None, Some(line(ChangeTag::Insert, "added")));
        assert_eq!(diff_marker_for_side(&insert, true), DiffMarker::Blank);
        assert_eq!(diff_marker_for_side(&insert, false), DiffMarker::Insert);

        assert_eq!(
            diff_marker_for_side(&DiffRow::omitted_gap(), true),
            DiffMarker::Gap
        );
        assert_eq!(
            diff_marker_for_side(&DiffRow::omitted_gap(), false),
            DiffMarker::Gap
        );
    }

    #[test]
    fn test_diff_max_line_width_uses_unicode_display_width() {
        let rows = vec![pair(Some(line(ChangeTag::Equal, "中中")), None)];
        assert_eq!(diff_max_line_width(&rows), 4);
    }
}
