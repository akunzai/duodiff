use crate::app::{App, FlatRow, HelpTopic, ViewMode};
use crate::diff::{DiffState, TreeSummary};
use crate::theme::Theme;
use ratatui::{prelude::*, widgets::*};
use std::path::Path;
use std::time::SystemTime;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Format a `SystemTime` as a UTC datetime string (`YYYY-MM-DD HH:MM:SS UTC`).
/// Uses UTC everywhere so we do not need platform-specific localtime (no `libc`).
fn format_system_time(t: &SystemTime) -> String {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let total_secs = dur.as_secs() as i64;
            let s = total_secs.rem_euclid(60);
            let m = (total_secs / 60).rem_euclid(60);
            let h = (total_secs / 3600).rem_euclid(24);
            let days = total_secs.div_euclid(86400);
            let (y, mo, d) = days_to_date(days);
            format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Gregorian civil date for `days_since_epoch` days after 1970-01-01 (UTC).
fn days_to_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let mut y = 1970;
    let mut remaining = days_since_epoch;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            mo = i as i64 + 1;
            break;
        }
        remaining -= md;
    }
    (y, mo, remaining + 1)
}

/// Build a detail info string for the selected row showing modification times
/// and sizes when both sides exist and differ.
///
/// `pub(crate)`: also called from [`App::tree_layout_inputs`] (has_detail), not just
/// [`draw_tree_footer`] — widened rather than re-deriving the same `DiffState` match twice.
/// `(left_tag, right_tag)` marking whichever side is newer, or two empty strings
/// when the timestamps match.
///
/// Derived from the timestamps rather than the `DiffState`, because the
/// `≈` [`DiffState::Unverified`] state deliberately carries no newer-side
/// variant — and `DifferentNewerLeft`/`Right` come from these same two
/// timestamps anyway, so one source of truth serves both.
fn newer_tag(
    left: std::time::SystemTime,
    right: std::time::SystemTime,
) -> (&'static str, &'static str) {
    match left.cmp(&right) {
        std::cmp::Ordering::Greater => (" (newer)", ""),
        std::cmp::Ordering::Less => ("", " (newer)"),
        std::cmp::Ordering::Equal => ("", ""),
    }
}

pub(crate) fn selected_row_detail(row: Option<&FlatRow>) -> Option<(String, String)> {
    let row = row?;
    if row.is_ambiguous_case_collision {
        let left_str = if let Some(left) = &row.left {
            let left_time = format_system_time(&left.modified);
            let time_tag = format!("  {left_time}");
            if left.is_dir {
                format!("Ambiguous case collision{time_tag}")
            } else {
                format!(
                    "Ambiguous case collision  {}{time_tag}",
                    format_size(left.size)
                )
            }
        } else {
            String::new()
        };
        let right_str = if let Some(right) = &row.right {
            let right_time = format_system_time(&right.modified);
            let time_tag = format!("  {right_time}");
            if right.is_dir {
                format!("Ambiguous case collision{time_tag}")
            } else {
                format!(
                    "Ambiguous case collision  {}{time_tag}",
                    format_size(right.size)
                )
            }
        } else {
            String::new()
        };
        return Some((left_str, right_str));
    }

    let left = row.left.as_ref()?;
    let right = row.right.as_ref()?;

    let unverified_reason = match row.state {
        // `≈` rows carry the same size/time detail, plus why the contents were
        // never compared (Issue #232).
        DiffState::Unverified(reason) => Some(reason),
        _ => None,
    };

    let left_time = format_system_time(&left.modified);
    let right_time = format_system_time(&right.modified);

    let (left_tag, right_tag) = newer_tag(left.modified, right.modified);
    let reason = unverified_reason
        .map(|r| format!("  ·  {}", r.detail()))
        .unwrap_or_default();

    let show_names = row.left_name() != row.right_name();

    let left_name_prefix = if show_names {
        format!("{}  ", row.left_name())
    } else {
        String::new()
    };
    let right_name_prefix = if show_names {
        format!("{}  ", row.right_name())
    } else {
        String::new()
    };

    let left_str = if left.is_dir {
        format!("{}{}{}", left_name_prefix, left_time, left_tag)
    } else {
        format!(
            "{}{} {}{}",
            left_name_prefix,
            format_size(left.size),
            left_time,
            left_tag
        )
    };
    let right_str = if right.is_dir {
        format!("{}{}{}{}", right_name_prefix, right_time, right_tag, reason)
    } else {
        format!(
            "{}{} {}{}{}",
            right_name_prefix,
            format_size(right.size),
            right_time,
            right_tag,
            reason
        )
    };
    Some((left_str, right_str))
}

/// Format byte size in a human-friendly form.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Render `input`'s text with a reverse-video block cursor at its real (char, not byte)
/// position, so multi-byte CJK text and mid-string editing show the cursor correctly.
/// `cursor_style` is the caller's accent colour; the cursor block always reverses video
/// regardless, so it stays visible over any text colour.
fn text_input_spans(
    input: &crate::text_input::TextInput,
    cursor_style: Style,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = input.chars().collect();
    let cursor = input.cursor().min(chars.len());
    let rev = cursor_style.add_modifier(Modifier::REVERSED);
    let mut spans = Vec::new();
    let before: String = chars[..cursor].iter().collect();
    if !before.is_empty() {
        spans.push(Span::raw(before));
    }
    if cursor < chars.len() {
        spans.push(Span::styled(chars[cursor].to_string(), rev));
        let after: String = chars[cursor + 1..].iter().collect();
        if !after.is_empty() {
            spans.push(Span::raw(after));
        }
    } else {
        spans.push(Span::styled(" ".to_string(), rev));
    }
    spans
}

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner_char(frame: usize) -> &'static str {
    SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]
}

/// Pure title-bar state for the shared top chrome (Config / Help shortcuts).
#[derive(Clone, Copy, Debug)]
pub struct TopBarView {
    pub view_mode: ViewMode,
    pub precise_mode: bool,
    pub diff_show_full: bool,
    pub diff_wrap: bool,
    pub scan_in_progress: bool,
    pub scan_progress_count: usize,
    pub spinner_frame: usize,
    pub theme: Theme,
}

/// Render the shared top bar from an [`App`] (projects via [`App::top_bar_view`]).
pub fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    draw_top_bar_content(f, &app.top_bar_view(), area);
}

// Text spans `draw_top_bar_content`'s right-aligned column renders, named so the
// painter and `top_bar_links`'s hit-test geometry read from the same source and
// cannot drift apart.
const TOPBAR_LEAD: &str = " (";
const TOPBAR_CONFIG_KEY: &str = "C";
const TOPBAR_CONFIG_LABEL: &str = ")onfig";
const TOPBAR_GAP: &str = "  ";
const TOPBAR_HELP_LEAD: &str = "(";
const TOPBAR_HELP_KEY: &str = "?";
const TOPBAR_HELP_LABEL: &str = ")Help";
const TOPBAR_TRAIL: &str = " ";

/// The top bar's `[left title, right Config/Help column]` split. Shared by
/// `draw_top_bar_content` (render) and `top_bar_links` (hit-test) so the column
/// boundary itself — not just the text within it — cannot drift between them.
fn top_bar_columns(area: Rect) -> (Rect, Rect) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(22)])
        .split(area);
    (layout[0], layout[1])
}

/// Paint the top bar from a hand-built [`TopBarView`] (no full `App`).
pub fn draw_top_bar_content(f: &mut Frame, view: &TopBarView, area: Rect) {
    let theme = view.theme;
    let (left_col, right_col) = top_bar_columns(area);

    let scan_indicator = if view.scan_in_progress {
        let spinner = spinner_char(view.spinner_frame);
        if view.scan_progress_count > 0 {
            format!(
                " {} Scanning ({} items)...",
                spinner, view.scan_progress_count
            )
        } else {
            format!(" {} Scanning...", spinner)
        }
    } else {
        String::new()
    };

    let left_text = match view.view_mode {
        ViewMode::DirectoryTree => {
            if view.precise_mode {
                format!(" duodiff - Directory Tree [Precise]{} ", scan_indicator)
            } else {
                format!(" duodiff - Directory Tree [Fast]{} ", scan_indicator)
            }
        }
        ViewMode::FileDiff => {
            let context_label = if view.diff_show_full {
                "Full"
            } else {
                "Diff Only"
            };
            let wrap_label = if view.diff_wrap { "Wrap" } else { "No Wrap" };
            format!(
                " duodiff - File Diff [{}] [{}]{} ",
                context_label, wrap_label, scan_indicator
            )
        }
        ViewMode::ConfigMenu => format!(" duodiff - Config{} ", scan_indicator),
        ViewMode::Help => format!(" duodiff - Help{} ", scan_indicator),
    };

    let left_p = Paragraph::new(Line::from(vec![Span::styled(
        left_text,
        Style::default().fg(theme.emphasis).bold(),
    )]));
    f.render_widget(left_p, left_col);

    let right_p = Paragraph::new(Line::from(vec![
        Span::styled(TOPBAR_LEAD, Style::default().fg(theme.muted)),
        Span::styled(TOPBAR_CONFIG_KEY, Style::default().fg(theme.accent).bold()),
        Span::styled(TOPBAR_CONFIG_LABEL, Style::default().fg(theme.muted)),
        Span::raw(TOPBAR_GAP),
        Span::styled(TOPBAR_HELP_LEAD, Style::default().fg(theme.muted)),
        Span::styled(TOPBAR_HELP_KEY, Style::default().fg(theme.accent).bold()),
        Span::styled(TOPBAR_HELP_LABEL, Style::default().fg(theme.muted)),
        Span::raw(TOPBAR_TRAIL),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(right_p, right_col);
}

/// The clickable Rects for the top bar's "(C)onfig"/"(?)Help" links, derived from
/// the same span-width constants `draw_top_bar_content` renders from — so the two
/// cannot drift apart. `area` is the top-bar's Rect (row 0, full width) — same
/// `Constraint::Length(22)` right column `draw_top_bar_content` splits out. Each
/// link's Rect covers its key + label text (e.g. "(C)onfig"), not the surrounding
/// lead space / gap / trailing space.
pub struct TopBarLinks {
    pub config: Rect,
    pub help: Rect,
}

pub fn top_bar_links(area: Rect) -> TopBarLinks {
    let (_, col) = top_bar_columns(area);

    let total_width = (TOPBAR_LEAD.len()
        + TOPBAR_CONFIG_KEY.len()
        + TOPBAR_CONFIG_LABEL.len()
        + TOPBAR_GAP.len()
        + TOPBAR_HELP_LEAD.len()
        + TOPBAR_HELP_KEY.len()
        + TOPBAR_HELP_LABEL.len()
        + TOPBAR_TRAIL.len()) as u16;
    let text_start = col.x + col.width.saturating_sub(total_width);

    let config_x = text_start + TOPBAR_LEAD.len() as u16 - 1; // include TOPBAR_LEAD's '('
    let config_width = 1 + TOPBAR_CONFIG_KEY.len() as u16 + TOPBAR_CONFIG_LABEL.len() as u16;

    let help_x = config_x + config_width + TOPBAR_GAP.len() as u16;
    let help_width = TOPBAR_HELP_LEAD.len() as u16
        + TOPBAR_HELP_KEY.len() as u16
        + TOPBAR_HELP_LABEL.len() as u16;

    TopBarLinks {
        config: Rect {
            x: config_x,
            y: col.y,
            width: config_width,
            height: 1,
        },
        help: Rect {
            x: help_x,
            y: col.y,
            width: help_width,
            height: 1,
        },
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    // Paint the full canvas so every unfilled cell uses the theme background (no-op for
    // dark theme where bg=Reset; effective for light theme which sets a white canvas).
    f.render_widget(Block::default().style(app.theme().base_style()), f.area());

    match app.view_mode() {
        ViewMode::DirectoryTree => {
            draw_tree(f, app);
            if app.confirm_modal().is_some() {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::FileDiff => {
            draw_diff(f, app);
            if app.confirm_modal().is_some() {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::ConfigMenu => {
            draw_config(f, app);
            if app.exclusion_editor_open() {
                let item_count = app
                    .exclusion_editor_view()
                    .map(|view| view.draft.len())
                    .unwrap_or(0);
                let layout = exclusion_editor_layout(item_count, f.area());
                app.sync_exclusion_editor_viewport(layout.visible_rows());
                if let Some(view) = app.exclusion_editor_view() {
                    draw_exclusion_editor(f, &view, &layout);
                }
            }
        }
        ViewMode::Help => draw_help(f, app),
    }

    if app.palette_visible() {
        draw_palette(f, app);
    }
}

fn get_display_path(path: &std::path::Path, max_len: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.len() <= max_len {
        return path_str.into_owned();
    }

    let sep = std::path::MAIN_SEPARATOR.to_string();
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|s| !s.is_empty() && s != &sep)
        .collect();

    if components.is_empty() {
        return path_str.into_owned();
    }

    let last = &components[components.len() - 1];
    let mut right_part = last.to_string();
    let mut idx = components.len().saturating_sub(2);
    while idx > 0 {
        let next_part = format!("{}{}{}", components[idx], sep, right_part);
        if next_part.len() + 4 <= max_len {
            right_part = next_part;
            idx -= 1;
        } else {
            break;
        }
    }

    let prefix = format!("...{sep}");
    if prefix.len() + right_part.len() <= max_len {
        return format!("{prefix}{right_part}");
    }

    if max_len <= prefix.len() {
        return "...".chars().take(max_len).collect();
    }

    let truncated_last = truncate_filename_middle(last, max_len - prefix.len());
    format!("{prefix}{truncated_last}")
}

/// Borrowed render state for the directory-tree **content** region (dual panes + indicator).
///
/// Built by [`App::tree_view`] in production, or hand-assembled in ui tests without
/// a full [`App`]. Top bar and footer stay on the [`draw_tree`] shell.
#[derive(Clone, Copy, Debug)]
pub struct TreeView<'a> {
    /// Filtered tree rows (full list; view applies scroll/selection).
    pub rows: &'a [FlatRow],
    pub scroll_offset: usize,
    pub selected_idx: usize,
    pub visible_height: usize,
    pub left_root: &'a std::path::Path,
    pub right_root: &'a std::path::Path,
    pub active_side_left: bool,
    pub theme: Theme,
    pub is_filter_active: bool,
}

/// Borrowed render state for the directory-tree **footer** region (status toast, detail
/// line, filter bar, keybindings/scan banner, update hint).
///
/// Built by [`App::tree_footer_view`] in production, or hand-assembled in ui tests without
/// a full [`App`]. Separate from [`TreeView`] (content-only) because the footer needs
/// several more fields than the content pane ever reads — folding them into `TreeView`
/// would make [`draw_tree_content`] receive data it never uses.
#[derive(Clone, Copy, Debug)]
pub struct TreeFooterView<'a> {
    /// Selected tree row (for the width-dependent left/right detail line).
    pub row: Option<&'a FlatRow>,
    pub status_toast: Option<(&'a str, bool)>,
    pub filter_active: bool,
    pub filter_input: &'a crate::text_input::TextInput,
    pub filter_pattern: &'a str,
    pub filter_diffs_only: bool,
    pub scan_in_progress: bool,
    pub scan_progress_count: usize,
    pub spinner_frame: usize,
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    pub theme: Theme,
    /// Whole-tree inventory for the summary line (Issue #252). `None` hides it.
    pub summary: Option<TreeSummary>,
}

/// Pure geometry-decision inputs for [`tree_layout`], shared with [`App::sync_viewport`]
/// (via [`App::tree_layout_inputs`]) so the sizing decision and the frame render read the
/// same booleans without either side borrowing `&App`. Same shape as [`DiffLayoutInputs`].
#[derive(Clone, Copy, Debug)]
pub struct TreeLayoutInputs {
    pub has_detail: bool,
    pub has_status: bool,
    pub has_filter: bool,
    pub has_update: bool,
    pub has_summary: bool,
}

/// Regions of the directory-tree screen.
pub struct TreeLayout {
    pub top_bar: Rect,
    /// Left file pane, borders included.
    pub left: Rect,
    /// Narrow column of `=` / `≈` / `≠` / `<` / `>` / `!` marks between the panes.
    pub indicator: Rect,
    /// Right file pane, borders included.
    pub right: Rect,
    pub footer: Rect,
}

/// Selected row as `n/N` among currently visible (filtered) rows, 1-based.
/// `None` when the tree is empty so the pane border stays clean.
pub(crate) fn tree_scroll_label(selected_idx: usize, total: usize) -> Option<String> {
    if total == 0 {
        None
    } else {
        let n = selected_idx.saturating_add(1).min(total);
        Some(format!("{n}/{total}"))
    }
}

/// Format a whole-tree inventory for the footer, dropping trailing units
/// that would not fit `width` display columns.
pub(crate) fn format_tree_summary(summary: TreeSummary, width: usize) -> String {
    let mut units = vec![
        format!("{} differ", summary.differ),
        format!("{} left-only", summary.left_only),
        format!("{} right-only", summary.right_only),
    ];
    if summary.unverified > 0 {
        units.push(format!("{} unverified", summary.unverified));
    }
    units.push(format!("{} identical", summary.identical));
    while !units.is_empty() {
        let line = units.join(" · ");
        if str_column_width(&line) <= width {
            return line;
        }
        units.pop();
    }
    String::new()
}

/// Split `area` into the directory-tree screen's regions.
///
/// Shared by [`draw_tree`] (via [`App::tree_layout_inputs`]) and [`App::sync_viewport`],
/// so the rects the renderer draws into and the geometry scrolling is clamped against
/// cannot drift apart.
pub fn tree_layout(inputs: &TreeLayoutInputs, area: Rect) -> TreeLayout {
    let TreeLayoutInputs {
        has_detail,
        has_status,
        has_filter,
        has_update,
        has_summary,
    } = *inputs;
    let footer_height = 1u16
        + u16::from(has_detail)
        + u16::from(has_status)
        + u16::from(has_filter)
        + u16::from(has_update)
        + u16::from(has_summary);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // Top Bar (1 line)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(area);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(30),   // Left
            Constraint::Length(4), // Indicator (no borders, symbols only)
            Constraint::Min(30),   // Right
        ])
        .split(chunks[1]);

    TreeLayout {
        top_bar: chunks[0],
        left: body_chunks[0],
        indicator: body_chunks[1],
        right: body_chunks[2],
        footer: chunks[2],
    }
}

/// Render the directory-tree screen.
///
/// Shell: layout + top bar (still need [`App`]). Content and footer paint through
/// [`draw_tree_content`]/[`draw_tree_footer`] with their own [`TreeView`]/[`TreeFooterView`]
/// so ui tests can exercise either region without a full app fixture.
pub fn draw_tree(f: &mut Frame, app: &App) {
    let inputs = app.tree_layout_inputs();
    let layout = tree_layout(&inputs, f.area());

    draw_top_bar(f, app, layout.top_bar);

    let view = app.tree_view();
    draw_tree_content(f, &view, &layout);

    let footer_view = app.tree_footer_view();
    draw_tree_footer(f, &footer_view, &layout);
}

/// Paint the directory-tree footer (status toast, detail line, filter bar,
/// keybindings/scan banner, update hint).
///
/// Same split as [`draw_tree_content`]: no `&App`, just `view` + `layout`. The
/// width-dependent detail-line padding needs `layout.footer.width`, so it computes
/// here rather than earlier — it can't be decided before the `Layout::split` that
/// produces the Rect.
pub fn draw_tree_footer(f: &mut Frame, view: &TreeFooterView<'_>, layout: &TreeLayout) {
    let theme = view.theme;

    let footer_txt = if view.scan_in_progress {
        let spinner = spinner_char(view.spinner_frame);
        let progress_str = if view.scan_progress_count > 0 {
            format!(
                " {} Scanning in progress ({} items)... Please wait.",
                spinner, view.scan_progress_count
            )
        } else {
            format!(" {} Scanning in progress... Please wait.", spinner)
        };
        Line::from(vec![Span::styled(
            progress_str,
            Style::default().fg(theme.warn).bold(),
        )])
    } else {
        Line::from(vec![
            Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
            Span::raw("or"),
            Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
            Span::raw("Command Palette  ·  right-click anywhere"),
        ])
    };

    // Build footer lines (top → bottom: status, detail, filter input, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error)) = view.status_toast {
        let status_style = if is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if is_error { "✗ " } else { "✓ " };
        let full_msg = format!("{}{}", icon, msg);
        let max_width = layout.footer.width as usize;
        let display_msg = truncate_to_width(&full_msg, max_width);
        footer_lines.push(Line::from(Span::styled(display_msg, status_style)));
    }

    if let Some((left_detail, right_detail)) = selected_row_detail(view.row) {
        let min_gutter = 2usize;
        let total_width = layout.footer.width as usize;
        let left_width = str_column_width(&left_detail);
        let right_width = str_column_width(&right_detail);

        let (left_out, right_out, space) = if total_width == 0 {
            (String::new(), String::new(), String::new())
        } else if left_width + right_width + min_gutter <= total_width {
            let padding = total_width - (left_width + right_width);
            (left_detail, right_detail, " ".repeat(padding))
        } else {
            let available = total_width.saturating_sub(min_gutter);
            let half = available / 2;
            let (left_alloc, right_alloc) = if left_width <= half {
                (left_width, available.saturating_sub(left_width))
            } else if right_width <= half {
                (available.saturating_sub(right_width), right_width)
            } else {
                (half, available - half)
            };
            let left_fit = truncate_to_width(&left_detail, left_alloc);
            let right_fit = truncate_to_width(&right_detail, right_alloc);
            let used = str_column_width(&left_fit) + str_column_width(&right_fit);
            let padding = total_width
                .saturating_sub(used)
                .max(min_gutter.min(total_width));
            (left_fit, right_fit, " ".repeat(padding))
        };

        if !left_out.is_empty() || !right_out.is_empty() {
            footer_lines.push(Line::from(vec![
                Span::styled(left_out, Style::default().fg(theme.accent)),
                Span::raw(space),
                Span::styled(right_out, Style::default().fg(theme.accent)),
            ]));
        }
    }

    // Filter input bar (shown when filter is active or a pattern is committed)
    if view.filter_active {
        let mut filter_spans = vec![Span::styled(
            " Filter: ",
            Style::default().fg(theme.warn).bold(),
        )];
        filter_spans.extend(text_input_spans(
            view.filter_input,
            Style::default().fg(theme.warn),
        ));
        if view.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }

        // Add filter hints with complete-unit truncation based on available width
        let total_w = layout.footer.width as usize;
        let prefix_w = 9; // " Filter: "
        let input_w = str_column_width(&view.filter_input.to_string()) + 1; // +1 for cursor
        let badge_w = if view.filter_diffs_only { 14 } else { 0 }; // "  [diffs only]"
        let base_w = prefix_w + input_w + badge_w;

        if total_w > base_w {
            let avail = total_w - base_w;
            let hint_str = if avail >= 46 {
                Some("  Enter:apply · Esc:cancel · Ctrl+f:diffs only")
            } else if avail >= 26 {
                Some("  Enter:apply · Esc:cancel")
            } else if avail >= 13 {
                Some("  Enter:apply")
            } else {
                None
            };
            if let Some(hints) = hint_str {
                filter_spans.push(Span::styled(hints, Style::default().fg(theme.dim)));
            }
        }

        footer_lines.push(Line::from(filter_spans));
    } else if !view.filter_pattern.is_empty() || view.filter_diffs_only {
        let mut filter_spans = vec![
            Span::styled(" Filter: ", Style::default().fg(theme.warn).bold()),
            Span::raw(view.filter_pattern),
            Span::styled(
                "  (/:edit, Esc/Backspace:clear)",
                Style::default().fg(theme.dim),
            ),
        ];
        if view.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    }

    if let Some(summary) = view.summary {
        let line = format_tree_summary(summary, layout.footer.width as usize);
        if !line.is_empty() {
            footer_lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(theme.muted),
            )));
        }
    }

    footer_lines.push(footer_txt);

    if let Some(version) = view.update_available {
        let hint = crate::upgrade::update_hint(version, view.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
}

/// Paint the directory-tree content region (left / indicator / right panes).
///
/// Does not touch top bar or footer — those stay on the [`draw_tree`] shell.
pub fn draw_tree_content(f: &mut Frame, view: &TreeView<'_>, layout: &TreeLayout) {
    let theme = view.theme;

    let mut left_items = Vec::new();
    let mut indicator_items = Vec::new();
    let mut right_items = Vec::new();

    // Pad the indicator column with a blank top line so symbols align
    // vertically with items in the bordered left/right panes (which have
    // a top border row).
    indicator_items.push(ListItem::new(""));
    let left_inner = layout.left.width.saturating_sub(2) as usize;
    let right_inner = layout.right.width.saturating_sub(2) as usize;

    if view.rows.is_empty() {
        left_items.push(
            ListItem::new(format_tree_cell(
                "  ",
                "",
                TREE_NO_VISIBLE_ENTRIES,
                left_inner,
            ))
            .style(Style::default().fg(theme.dim)),
        );
        right_items.push(
            ListItem::new(format_tree_cell(
                "  ",
                "",
                TREE_NO_VISIBLE_ENTRIES,
                right_inner,
            ))
            .style(Style::default().fg(theme.dim)),
        );
    } else {
        for (i, row) in view
            .rows
            .iter()
            .enumerate()
            .skip(view.scroll_offset)
            .take(view.visible_height)
        {
            let is_selected = i == view.selected_idx;
            let style = if is_selected {
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg)
            } else {
                match row.state {
                    DiffState::Identical => Style::default().fg(theme.muted),
                    // Both are yellow warnings, but an established difference is
                    // bold so `≠` outweighs the merely-unverified `≈` (Issue #232).
                    DiffState::Unverified(_) => Style::default().fg(theme.warn),
                    DiffState::DifferentNewerLeft
                    | DiffState::DifferentNewerRight
                    | DiffState::DifferentSameTime => Style::default().fg(theme.warn).bold(),
                    DiffState::LeftOnly => Style::default().fg(theme.success),
                    DiffState::RightOnly => Style::default().fg(theme.info),
                    DiffState::TypeConflict => Style::default().fg(theme.error).bold(),
                }
            };

            let is_filter_active = view.is_filter_active;
            let indent = if is_filter_active {
                String::new()
            } else {
                "  ".repeat(row.depth)
            };

            // Left item
            if let Some(ref left_info) = row.left {
                // The filtered list is flat, so no row is collapsible there; the
                // trailing separator alone carries directory-ness.
                let icon =
                    disclosure_marker(!is_filter_active && left_info.is_dir, row.is_expanded);
                let name = entry_display_name(row.left_name(), left_info.is_dir);
                let cell_text = if is_filter_active {
                    let rel_path = row.left_relative_path();
                    let prefix_width = str_column_width(icon);
                    let inner_avail = left_inner.saturating_sub(prefix_width);
                    let parent = rel_path.parent().unwrap_or(Path::new(""));
                    let formatted = format_breadcrumb(parent, &name, inner_avail);
                    format!("{icon}{formatted}")
                } else {
                    format_tree_cell(&indent, icon, &name, left_inner)
                };
                left_items.push(ListItem::new(cell_text).style(style));
            } else {
                left_items.push(ListItem::new("").style(style));
            }

            // Indicator
            let base_symbol = match row.state {
                DiffState::Identical => "=",
                DiffState::Unverified(_) => "≈",
                DiffState::DifferentNewerLeft
                | DiffState::DifferentNewerRight
                | DiffState::DifferentSameTime => "≠",
                DiffState::LeftOnly => "<",
                DiffState::RightOnly => ">",
                DiffState::TypeConflict => "!",
            };
            let has_case_badge = row.has_case_conflict
                || row.contains_case_conflict
                || row.is_ambiguous_case_collision;
            let symbol_str = if has_case_badge {
                format!(" {base_symbol}Aa")
            } else {
                format!(" {base_symbol}")
            };
            indicator_items.push(ListItem::new(symbol_str).style(style));

            // Right item
            if let Some(ref right_info) = row.right {
                // The filtered list is flat, so no row is collapsible there; the
                // trailing separator alone carries directory-ness.
                let icon =
                    disclosure_marker(!is_filter_active && right_info.is_dir, row.is_expanded);
                let name = entry_display_name(row.right_name(), right_info.is_dir);
                let cell_text = if is_filter_active {
                    let rel_path = row.right_relative_path();
                    let prefix_width = str_column_width(icon);
                    let inner_avail = right_inner.saturating_sub(prefix_width);
                    let parent = rel_path.parent().unwrap_or(Path::new(""));
                    let formatted = format_breadcrumb(parent, &name, inner_avail);
                    format!("{icon}{formatted}")
                } else {
                    format_tree_cell(&indent, icon, &name, right_inner)
                };
                right_items.push(ListItem::new(cell_text).style(style));
            } else {
                right_items.push(ListItem::new("").style(style));
            }
        }
    }

    let left_title = Line::from(vec![
        Span::raw(" "),
        Span::styled("[1] ", Style::default().fg(theme.accent).bold()),
        Span::styled(
            get_display_path(view.left_root, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);
    let right_title = Line::from(vec![
        Span::raw(" "),
        Span::styled("[2] ", Style::default().fg(theme.accent).bold()),
        Span::styled(
            get_display_path(view.right_root, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);

    let left_border_style = if view.active_side_left {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.dim)
    };

    let right_border_style = if !view.active_side_left {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.dim)
    };

    let scroll_title = tree_scroll_label(view.selected_idx, view.rows.len()).map(|label| {
        Line::from(Span::styled(
            format!(" {label} "),
            Style::default().fg(theme.muted),
        ))
        .right_aligned()
    });

    let mut left_block = Block::default()
        .title(left_title)
        .border_style(left_border_style)
        .borders(Borders::ALL);
    let mut right_block = Block::default()
        .title(right_title)
        .border_style(right_border_style)
        .borders(Borders::ALL);
    if let Some(scroll_title) = scroll_title {
        left_block = left_block.title_bottom(scroll_title.clone());
        right_block = right_block.title_bottom(scroll_title);
    }

    let left_list = List::new(left_items).block(left_block);

    let indicator_list = List::new(indicator_items);

    let right_list = List::new(right_items).block(right_block);

    f.render_widget(left_list, layout.left);
    f.render_widget(indicator_list, layout.indicator);
    f.render_widget(right_list, layout.right);
}

/// Format a `SystemTime` as a relative time string (e.g. "3d ago", "1y ago").
fn format_relative_time(t: &SystemTime) -> String {
    let now = SystemTime::now();
    match now.duration_since(*t) {
        Ok(dur) => {
            let secs = dur.as_secs();
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else if secs < 2_592_000 {
                format!("{}d ago", secs / 86400)
            } else if secs < 31_536_000 {
                format!("{}mo ago", secs / 2_592_000)
            } else {
                format!("{}y ago", secs / 31_536_000)
            }
        }
        Err(_) => format_system_time(t),
    }
}

/// Wrap a single line of text into chunks that fit within `width` display columns.
/// Preserves empty input as a single empty chunk so alignment is maintained.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_text_with_mask(text, &[], width)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

/// Extract the visible portion of `text` starting at `h_scroll` display columns.
fn scrolled_text(text: &str, h_scroll: usize, width: usize) -> String {
    scrolled_text_with_mask(text, &[], h_scroll, width).0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLineHighlight {
    None,
    /// Any row that belongs to a differing (mergeable) hunk.
    ChangeHunk,
    /// The hunk under the current scroll position (target for `[` / `]`).
    ActiveHunk,
    /// The physical row at `diff_scroll`.
    Cursor,
}

fn diff_line_highlight(
    in_change_hunk: bool,
    in_active_hunk: bool,
    is_cursor: bool,
) -> DiffLineHighlight {
    if is_cursor {
        DiffLineHighlight::Cursor
    } else if in_active_hunk {
        DiffLineHighlight::ActiveHunk
    } else if in_change_hunk {
        DiffLineHighlight::ChangeHunk
    } else {
        DiffLineHighlight::None
    }
}

fn apply_diff_line_highlight(style: Style, highlight: DiffLineHighlight, theme: Theme) -> Style {
    match highlight {
        DiffLineHighlight::None => style,
        DiffLineHighlight::ChangeHunk => style.bg(theme.hunk_bg),
        DiffLineHighlight::ActiveHunk => style.bg(theme.active_hunk_bg),
        DiffLineHighlight::Cursor => style.bg(theme.cursor_bg).bold(),
    }
}

#[derive(Clone)]
struct DiffDisplayCell {
    gutter: String,
    text: String,
    tag: Option<similar::ChangeTag>,
    intraline_mask: Option<Vec<bool>>,
    highlight: DiffLineHighlight,
}

fn diff_tag_base_style(tag: Option<similar::ChangeTag>, theme: Theme) -> Style {
    match tag {
        Some(similar::ChangeTag::Delete) => Style::default().fg(theme.error),
        Some(similar::ChangeTag::Insert) => Style::default().fg(theme.success),
        Some(similar::ChangeTag::Equal) => Style::default().fg(theme.muted),
        None => Style::default(),
    }
}

fn line_from_diff_cell(cell: &DiffDisplayCell, theme: Theme) -> Line<'static> {
    let base =
        apply_diff_line_highlight(diff_tag_base_style(cell.tag, theme), cell.highlight, theme);
    let mut spans = Vec::new();
    if !cell.gutter.is_empty() {
        spans.push(Span::styled(cell.gutter.clone(), base));
    }
    let Some(mask) = &cell.intraline_mask else {
        if cell.text.is_empty()
            && cell.highlight != DiffLineHighlight::None
            && cell.gutter.is_empty()
        {
            spans.push(Span::styled(" ", base));
            return Line::from(spans);
        }
        if !cell.text.is_empty() {
            spans.push(Span::styled(cell.text.clone(), base));
        } else if spans.is_empty() {
            return Line::from(Span::raw(""));
        }
        return Line::from(spans);
    };

    let chars: Vec<char> = cell.text.chars().collect();
    if chars.is_empty() {
        if cell.highlight != DiffLineHighlight::None && cell.gutter.is_empty() {
            spans.push(Span::styled(" ", base));
        }
        return Line::from(spans);
    }

    let mut aligned_mask = mask.clone();
    aligned_mask.truncate(chars.len());
    aligned_mask.resize(chars.len(), false);

    let mut run_start = 0usize;
    let mut run_highlight = aligned_mask[0];

    for i in 1..=chars.len() {
        if i == chars.len() || aligned_mask[i] != run_highlight {
            let run: String = chars[run_start..i].iter().collect();
            let style = if run_highlight {
                base.bold().underlined()
            } else {
                base
            };
            spans.push(Span::styled(run, style));
            if i < chars.len() {
                run_start = i;
                run_highlight = aligned_mask[i];
            }
        }
    }

    Line::from(spans)
}

fn wrap_text_with_mask(text: &str, mask: &[bool], width: usize) -> Vec<(String, Vec<bool>)> {
    if width == 0 {
        return vec![(text.to_string(), mask.to_vec())];
    }

    let chars: Vec<char> = text.chars().collect();
    let mut aligned_mask = mask.to_vec();
    aligned_mask.truncate(chars.len());
    aligned_mask.resize(chars.len(), false);

    let mut lines = Vec::new();
    let mut line_chars = Vec::new();
    let mut line_mask = Vec::new();
    let mut line_width = 0usize;

    for (ch, highlighted) in chars.iter().zip(aligned_mask.iter()) {
        let ch_width = crate::diff_view::char_display_width(*ch);
        if line_width + ch_width > width && !line_chars.is_empty() {
            lines.push((line_chars.iter().collect(), std::mem::take(&mut line_mask)));
            line_chars.clear();
            line_width = 0;
        }
        line_chars.push(*ch);
        line_mask.push(*highlighted);
        line_width += ch_width;
    }

    if !line_chars.is_empty() || lines.is_empty() {
        lines.push((line_chars.into_iter().collect(), line_mask));
    }

    lines
}

fn scrolled_text_with_mask(
    text: &str,
    mask: &[bool],
    h_scroll: usize,
    width: usize,
) -> (String, Vec<bool>) {
    if width == 0 {
        return (String::new(), Vec::new());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut aligned_mask = mask.to_vec();
    aligned_mask.truncate(chars.len());
    aligned_mask.resize(chars.len(), false);

    let mut skipped = 0usize;
    let mut out = String::new();
    let mut out_mask = Vec::new();
    let mut out_width = 0usize;
    for (ch, highlighted) in chars.into_iter().zip(aligned_mask) {
        let ch_width = crate::diff_view::char_display_width(ch);
        if skipped < h_scroll {
            skipped += ch_width;
            continue;
        }
        if out_width + ch_width > width {
            break;
        }
        out.push(ch);
        out_mask.push(highlighted);
        out_width += ch_width;
    }
    (out, out_mask)
}

fn push_diff_display_cells(
    cells: &mut Vec<DiffDisplayCell>,
    text: Option<&str>,
    tag: Option<similar::ChangeTag>,
    intraline_mask: Option<Vec<bool>>,
    wrap: bool,
    content_width: usize,
    h_scroll: usize,
) {
    let Some(text) = text else {
        cells.push(DiffDisplayCell {
            gutter: String::new(),
            text: String::new(),
            tag: None,
            intraline_mask: None,
            highlight: DiffLineHighlight::None,
        });
        return;
    };

    if wrap {
        if let Some(mask) = intraline_mask.as_deref() {
            for (chunk, chunk_mask) in wrap_text_with_mask(text, mask, content_width) {
                cells.push(DiffDisplayCell {
                    gutter: String::new(),
                    text: chunk,
                    tag,
                    intraline_mask: Some(chunk_mask),
                    highlight: DiffLineHighlight::None,
                });
            }
        } else {
            for chunk in wrap_text(text, content_width) {
                cells.push(DiffDisplayCell {
                    gutter: String::new(),
                    text: chunk,
                    tag,
                    intraline_mask: None,
                    highlight: DiffLineHighlight::None,
                });
            }
        }
    } else {
        let (visible, visible_mask) = if let Some(mask) = intraline_mask.as_ref() {
            scrolled_text_with_mask(text, mask, h_scroll, content_width)
        } else {
            (scrolled_text(text, h_scroll, content_width), Vec::new())
        };
        cells.push(DiffDisplayCell {
            gutter: String::new(),
            text: visible,
            tag,
            intraline_mask: if intraline_mask.is_some() {
                Some(visible_mask)
            } else {
                None
            },
            highlight: DiffLineHighlight::None,
        });
    }
}

/// Borrowed render state for the file-diff **content** region (info bar + panes).
///
/// Built by [`App::diff_view`] in production, or hand-assembled in ui tests without
/// standing up a full [`App`]. Top bar and footer stay on the `draw_diff` shell.
#[derive(Clone, Copy, Debug)]
pub struct DiffView<'a> {
    pub rows: &'a [crate::diff_view::DiffRow],
    pub wrap: bool,
    pub scroll: usize,
    pub h_scroll: usize,
    /// Content rows visible in each pane (from [`crate::app::Viewport`]).
    pub visible_height: usize,
    /// Content columns inside one pane (borders excluded, gutter subtracted).
    pub content_width: usize,
    pub left_line_count: usize,
    pub right_line_count: usize,
    pub left_root: &'a std::path::Path,
    pub right_root: &'a std::path::Path,
    /// Selected tree row that was opened into the diff (for titles / info bar).
    pub row: Option<&'a FlatRow>,
    pub left_hash: Option<&'a str>,
    pub right_hash: Option<&'a str>,
    pub left_line_ending: Option<&'a str>,
    pub right_line_ending: Option<&'a str>,
    pub theme: Theme,
    /// Active footer toast, if any: `(message, is_error)` (footer content).
    pub status_toast: Option<(&'a str, bool)>,
    /// Whether the two sides have any differing lines (keybinding-hint trimming, footer content).
    pub has_changes: bool,
    /// Latest update version when available (update hint, footer content).
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    /// Whether each side has staged, unwritten edits. A dirty pane's title is
    /// marked with `*` and the footer offers `s save · u undo` (Issue #235).
    pub left_dirty: bool,
    pub right_dirty: bool,
    /// Whether a staged hunk operation can still be undone.
    pub can_undo: bool,
}

/// Pure geometry-decision inputs for [`diff_layout`], shared with [`App::sync_viewport`]
/// (via [`App::diff_layout_inputs`]) so the sizing decision and the frame render read the
/// same booleans without either side borrowing `&App`.
#[derive(Clone, Copy, Debug)]
pub struct DiffLayoutInputs {
    pub has_changes: bool,
    /// Selected row has content on either side (used with `!has_changes` to show the
    /// "files are identical" notice).
    pub row_has_content: bool,
    pub has_status: bool,
    pub has_update: bool,
}

/// Regions of the file-diff screen.
pub struct DiffLayout {
    pub top_bar: Rect,
    /// Row below the top bar carrying the "files are identical" notice; empty
    /// unless [`DiffLayout::show_identical`].
    pub notice: Rect,
    /// Left half of the info bar (size + SHA-256 + line ending).
    pub info_left: Rect,
    /// Right half of the info bar.
    pub info_right: Rect,
    /// Left diff pane, borders included.
    pub left: Rect,
    /// Right diff pane, borders included.
    pub right: Rect,
    pub footer: Rect,
    /// True when the two sides have no differing lines.
    pub show_identical: bool,
}

/// Split `area` into the file-diff screen's regions.
///
/// Shared by [`draw_diff`] (via [`App::diff_layout_inputs`]) and [`App::sync_viewport`],
/// so the rects the renderer draws into and the geometry scrolling is clamped against
/// cannot drift apart.
pub fn diff_layout(inputs: &DiffLayoutInputs, area: Rect) -> DiffLayout {
    let show_identical = !inputs.has_changes && inputs.row_has_content;

    let header_height = if show_identical { 2 } else { 1 };
    let footer_height =
        if inputs.has_status { 2 } else { 1 } + if inputs.has_update { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Header (Top Bar + optional Identical Msg)
            Constraint::Length(1),             // Info bar (size + SHA-256)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(area);

    let header_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(chunks[0]);

    let info_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    DiffLayout {
        top_bar: header_layout[0],
        notice: header_layout[1],
        info_left: info_chunks[0],
        info_right: info_chunks[1],
        left: body_chunks[0],
        right: body_chunks[1],
        footer: chunks[3],
        show_identical,
    }
}

/// Render the file-diff screen.
///
/// Shell: layout + top bar (still need [`App`]). Content and footer paint through
/// [`draw_diff_content`]/[`draw_diff_footer`] with one shared [`DiffView`] so ui tests
/// can exercise either region without a full app fixture.
pub fn draw_diff(f: &mut Frame, app: &App) {
    let inputs = app.diff_layout_inputs();
    let layout = diff_layout(&inputs, f.area());

    draw_top_bar(f, app, layout.top_bar);

    let view = app.diff_view();
    draw_diff_content(f, &view, &layout);
    draw_diff_footer(f, &view, &layout);
}

/// Paint the file-diff footer (status toast, keybindings, update hint).
///
/// Same split as [`draw_diff_content`]: no `&App`, just `view` + `layout`.
pub fn draw_diff_footer(f: &mut Frame, view: &DiffView<'_>, layout: &DiffLayout) {
    let theme = view.theme;

    // Build footer lines (top → bottom: status, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error)) = view.status_toast {
        let status_style = if is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if is_error { "✗ " } else { "✓ " };
        let full_msg = format!("{}{}", icon, msg);
        let max_width = layout.footer.width as usize;
        let display_msg = truncate_to_width(&full_msg, max_width);
        footer_lines.push(Line::from(Span::styled(display_msg, status_style)));
    }

    let mut footer_spans = vec![
        Span::styled(" N ", Style::default().fg(theme.accent).bold()),
        Span::raw("Next  ·  "),
        Span::styled(" P ", Style::default().fg(theme.accent).bold()),
        Span::raw("Prev  ·  "),
        Span::styled(" [ ", Style::default().fg(theme.accent).bold()),
        Span::raw("Hunk←  ·  "),
        Span::styled(" ] ", Style::default().fg(theme.accent).bold()),
        Span::raw("Hunk→  ·  "),
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ];
    // Staged, unwritten edits get their own hint line so the way out is obvious.
    if view.left_dirty || view.right_dirty {
        let mut staged = vec![
            Span::styled(" s ", Style::default().fg(theme.warn).bold()),
            Span::raw("save  ·  "),
        ];
        if view.can_undo {
            staged.push(Span::styled(" u ", Style::default().fg(theme.warn).bold()));
            staged.push(Span::raw("undo  ·  "));
        }
        staged.push(Span::styled(
            " Esc ",
            Style::default().fg(theme.warn).bold(),
        ));
        staged.push(Span::raw("back"));
        footer_lines.push(Line::from(staged));
    }
    if !view.has_changes {
        footer_spans.drain(0..10);
    }
    footer_lines.push(Line::from(footer_spans));

    if let Some(version) = view.update_available {
        let hint = crate::upgrade::update_hint(version, view.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
}

/// Paint the file-diff content region (identical notice, info bar, dual panes).
///
/// Does not touch top bar or footer — those stay on the [`draw_diff`] shell.
/// Geometry comes from `layout` (shell/`diff_layout`); line data from `view`.
pub fn draw_diff_content(f: &mut Frame, view: &DiffView<'_>, layout: &DiffLayout) {
    let theme = view.theme;
    let show_identical = layout.show_identical;

    if show_identical {
        let msg = Paragraph::new(Line::from(Span::styled(
            " ✓ Both files are identical — no differences found.",
            Style::default().fg(theme.success).bold(),
        )));
        f.render_widget(msg, layout.notice);
    }

    // Info bar: size + SHA-256 hash for each side, above the pane borders
    let left_info =
        build_diff_info_spans(view.row, true, view.left_hash, view.left_line_ending, theme);
    let right_info = build_diff_info_spans(
        view.row,
        false,
        view.right_hash,
        view.right_line_ending,
        theme,
    );
    f.render_widget(Paragraph::new(left_info), layout.info_left);
    f.render_widget(Paragraph::new(right_info), layout.info_right);

    let max_visible = view.visible_height;
    let pane_inner = layout.left.width.saturating_sub(2) as usize;
    let left_gutter = crate::diff_view::diff_gutter(view.left_line_count, pane_inner);
    let right_gutter = crate::diff_view::diff_gutter(view.right_line_count, pane_inner);
    // Same snapshot `App::sync_viewport` wrote — wrap, h-scroll, and hunk
    // mapping must not recompute a second width from `layout` (ADR-0002).
    let content_width = view.content_width;

    let Some(row) = view.row else {
        return;
    };

    let mut left_physical: Vec<DiffDisplayCell> = Vec::new();
    let mut right_physical: Vec<DiffDisplayCell> = Vec::new();

    let hunk_row_ranges = crate::diff_view::diff_hunk_row_ranges(view.rows);
    let active_hunk_rows =
        crate::diff_view::hunk_index_at_scroll(view.rows, view.scroll, content_width, view.wrap)
            .and_then(|idx| hunk_row_ranges.get(idx).cloned());

    let mut physical_row = 0usize;
    for (logical_row, diff_row) in view.rows.iter().enumerate() {
        let left_line = &diff_row.left;
        let right_line = &diff_row.right;
        let in_change_hunk = hunk_row_ranges
            .iter()
            .any(|range| range.contains(&logical_row));
        let in_active_hunk = active_hunk_rows
            .as_ref()
            .is_some_and(|range| range.contains(&logical_row));
        let left_text = left_line.as_ref().map(|l| l.text.trim_end());
        let right_text = right_line.as_ref().map(|r| r.text.trim_end());
        let left_tag = left_line.as_ref().map(|l| l.tag);
        let right_tag = right_line.as_ref().map(|r| r.tag);
        let left_marker = crate::diff_view::diff_marker_for_side(diff_row, true);
        let right_marker = crate::diff_view::diff_marker_for_side(diff_row, false);

        let replacement = crate::diff_view::is_replacement_pair(left_line, right_line);
        let left_mask = replacement
            .then(|| {
                left_text
                    .zip(right_text)
                    .map(|(left, right)| crate::diff_view::intraline_change_mask(left, right, true))
            })
            .flatten();
        let right_mask = replacement
            .then(|| {
                left_text.zip(right_text).map(|(left, right)| {
                    crate::diff_view::intraline_change_mask(right, left, false)
                })
            })
            .flatten();

        let mut left_chunk = Vec::new();
        let mut right_chunk = Vec::new();
        push_diff_display_cells(
            &mut left_chunk,
            left_text,
            left_tag,
            left_mask,
            view.wrap,
            content_width,
            view.h_scroll,
        );
        push_diff_display_cells(
            &mut right_chunk,
            right_text,
            right_tag,
            right_mask,
            view.wrap,
            content_width,
            view.h_scroll,
        );

        let max_lines = std::cmp::max(left_chunk.len(), right_chunk.len());
        for i in 0..max_lines {
            let highlight = diff_line_highlight(
                in_change_hunk,
                in_active_hunk,
                physical_row + i == view.scroll,
            );
            let continuation = i > 0;
            let mut left_cell = left_chunk.get(i).cloned().unwrap_or(DiffDisplayCell {
                gutter: String::new(),
                text: String::new(),
                tag: left_tag,
                intraline_mask: None,
                highlight,
            });
            left_cell.gutter = crate::diff_view::format_diff_gutter(
                left_gutter,
                diff_row.left_source,
                left_marker,
                continuation,
            );
            left_cell.highlight = highlight;
            left_physical.push(left_cell);

            let mut right_cell = right_chunk.get(i).cloned().unwrap_or(DiffDisplayCell {
                gutter: String::new(),
                text: String::new(),
                tag: right_tag,
                intraline_mask: None,
                highlight,
            });
            right_cell.gutter = crate::diff_view::format_diff_gutter(
                right_gutter,
                diff_row.right_source,
                right_marker,
                continuation,
            );
            right_cell.highlight = highlight;
            right_physical.push(right_cell);
        }
        physical_row += max_lines;
    }

    let left_lines: Vec<Line> = left_physical
        .into_iter()
        .skip(view.scroll)
        .take(max_visible)
        .map(|cell| line_from_diff_cell(&cell, theme))
        .collect();

    let right_lines: Vec<Line> = right_physical
        .into_iter()
        .skip(view.scroll)
        .take(max_visible)
        .map(|cell| line_from_diff_cell(&cell, theme))
        .collect();

    // Build pane titles: " [1] /truncated/path/file.txt (3d ago) "
    let left_title = build_diff_pane_title(
        "[1] ",
        &view.left_root.join(&row.relative_path),
        row.left.as_ref().map(|f| &f.modified),
        view.left_dirty,
        false,
        layout.left.width as usize,
        theme,
    );
    let right_title = build_diff_pane_title(
        "[2] ",
        &view.right_root.join(&row.relative_path),
        row.right.as_ref().map(|f| &f.modified),
        view.right_dirty,
        true,
        layout.right.width as usize,
        theme,
    );

    let left_p =
        Paragraph::new(left_lines).block(Block::default().title(left_title).borders(Borders::ALL));
    let right_p = Paragraph::new(right_lines)
        .block(Block::default().title(right_title).borders(Borders::ALL));

    f.render_widget(left_p, layout.left);
    f.render_widget(right_p, layout.right);
    draw_close_button(f, layout.right);
}

/// Build info spans (size + line ending style + SHA-256 hash) for the diff view info bar.
fn build_diff_info_spans<'a>(
    row: Option<&'a FlatRow>,
    is_left: bool,
    hash: Option<&'a str>,
    line_ending: Option<&'a str>,
    theme: Theme,
) -> Line<'a> {
    let info = row.and_then(|r| {
        if is_left {
            r.left.as_ref()
        } else {
            r.right.as_ref()
        }
    });

    let mut spans = vec![Span::raw(" ")];

    if let Some(fi) = info {
        if !fi.is_dir {
            spans.push(Span::styled(
                format_size(fi.size),
                Style::default().fg(theme.dim),
            ));
            spans.push(Span::raw("  "));
        }
    }

    if let Some(le) = line_ending {
        spans.push(Span::styled(
            format!("[{}]", le),
            Style::default().fg(theme.dim),
        ));
        spans.push(Span::raw("  "));
    }

    if let Some(h) = hash {
        spans.push(Span::styled(
            format!("SHA256: {h}"),
            Style::default().fg(theme.dim),
        ));
    } else {
        spans.push(Span::styled("SHA256: —", Style::default().fg(theme.dim)));
    }

    Line::from(spans)
}

fn build_diff_pane_title<'a>(
    marker: &'static str,
    full_path: &std::path::Path,
    modified: Option<&SystemTime>,
    is_dirty: bool,
    has_close_button: bool,
    pane_width: usize,
    theme: Theme,
) -> Line<'a> {
    let rel_time = modified.map(format_relative_time).unwrap_or_default();
    let suffix_len = if rel_time.is_empty() {
        1
    } else {
        rel_time.len() + 4 // " (rel_time) "
    };
    let prefix_len = 5; // " [1] " or "*[1] "
    let right_margin = if has_close_button && pane_width >= 6 {
        6 // reserve 5 columns for [x] (area.width - 5..area.width - 2) + 1 column margin
    } else {
        2 // borders
    };
    let fixed_len = prefix_len + suffix_len;
    let max_path = pane_width.saturating_sub(fixed_len + right_margin).max(5);
    let display_path = get_display_path(full_path, max_path);

    let prefix_spans = if is_dirty {
        vec![
            Span::styled("*", Style::default().bold().fg(theme.warn)),
            Span::styled(marker, Style::default().bold().fg(theme.warn)),
        ]
    } else {
        vec![
            Span::raw(" "),
            Span::styled(marker, Style::default().bold().fg(theme.accent)),
        ]
    };

    let text_style = if is_dirty {
        Style::default().bold().fg(theme.warn)
    } else {
        Style::default().bold().fg(theme.fg)
    };

    let mut spans = prefix_spans;
    spans.push(Span::styled(display_path, text_style));
    if !rel_time.is_empty() {
        spans.push(Span::styled(format!(" ({}) ", rel_time), text_style));
    } else {
        spans.push(Span::raw(" "));
    }

    Line::from(spans)
}

/// 0-indexed row of the clickable repo-URL line within the `About` topic body (see the
/// `HelpTopic::About` arm of `help_topic_body`) — kept in sync with `handle_mouse`'s click
/// detection in `input.rs`. Stable regardless of update-check state since the URL line always
/// comes before the optional update-hint line.
pub(crate) const ABOUT_REPO_LINE: u16 = 2;

/// Borrowed render state for the Help **body** region (topic list or scrolled body).
///
/// Built by [`App::help_view`]; top bar and footer stay on the [`draw_help`] shell.
#[derive(Clone, Copy, Debug)]
pub struct HelpView<'a> {
    pub topic: HelpTopic,
    pub index_open: bool,
    pub index_sel: usize,
    pub scroll: u16,
    pub theme: Theme,
    /// Latest update version when available (About topic footer line).
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
}

fn help_topic_body(
    topic: HelpTopic,
    theme: Theme,
    update_available: Option<&str>,
    install_method: &crate::upgrade::InstallMethod,
) -> Text<'static> {
    match topic {
        HelpTopic::DirectoryTree => Text::from(
            "\
Navigation
  j / Down       move selection down
  k / Up         move selection up
  Ctrl+f         page selection down (about one screen)
  Ctrl+b         page selection up (about one screen)
  h / Left       collapse the selected directory
  l / Right      expand the selected directory
  Space          toggle expand/collapse
  Tab            switch focus between the left and right panes
  1 / 2          jump focus directly to the left / right pane

Row states
  =              no difference found by the active scan mode
  ≈              content unverified — the bytes were not compared
                 (Fast mode: sizes match but timestamps differ;
                  Precise mode: a side could not be read or hashed)
  ≠              a difference the scan established
  < / >          present on the left / right side only
  !              one side is a file, the other a directory
  Aa             case-only path mismatch or collision

Row shape
  ▸ / ▾          a collapsed / expanded directory; a trailing / marks
                 a directory even where the marker does not apply
  (none)         a file, indented in line with its siblings

Scale
  n/N            selected row and visible-row count, on each pane's
                 bottom border (follows the filter, not collapse)
  footer         whole-tree inventory (differ / left-only /
                 right-only / unverified / identical), independent
                 of collapse and filter; trailing units drop on
                 narrow terminals

Actions
  Enter          open the diff view (or toggle expand, for a directory)
  D              compare the selected file pair with the external diff tool
  E              edit the selected file in $EDITOR/$VISUAL
  L              copy the selected item from the right pane to the left (y/n confirm)
  R              copy the selected item from the left pane to the right (y/n confirm)
  C              open the Config screen
  c              switch Fast / Precise scan mode (persists, then re-scans)
  r              force a manual re-scan
  s              swap the left and right directories
  /              open the filter bar; every printable character is typed
                 into the query (Ctrl+f: toggle diffs-only,
                 Enter: apply, Esc: cancel)
  ?              show this help
  Esc            clear the applied filter, or quit when none is applied
  q              quit",
        ),
        HelpTopic::FileDiff => Text::from(
            "  Limits         UTF-8 text only, max 10 MiB per side
                 (binary / non-UTF-8 / oversized → toast; use D)
  j / Down       scroll down one line
  k / Up         scroll up one line
  Ctrl+f         page scroll down (about one screen)
  Ctrl+b         page scroll up (about one screen)
  N / Alt+Down   jump to next change block
  P / Alt+Up     jump to previous change block
  Left / Right   scroll horizontally (only while wrap is off)
  Gutters        1-based source line numbers; - deleted, + inserted,
                 blank for context, … for an omitted collapsed range
  Highlighting   mergeable blocks are tinted; the active block and
                 current line are emphasized for `[` / `]` targets
  [              stage the change block under the cursor to the left
  ]              stage the change block under the cursor to the right
                 (staged only — a `*` marks each dirty pane title)
  s              save every staged side (shows the paths with home as ~,
                 then confirms)
  u              undo the last staged change block
  L              copy the whole right file to the left side (confirm)
  R              copy the whole left file to the right side (confirm)
                 (both are blocked while staged changes are unsaved)
  w              toggle line wrapping
  f              toggle full-file context vs diff-only
  D              compare the same pair with the external diff tool
  E              edit the focused side's file in $EDITOR/$VISUAL
  C              open the Config screen (returns here on Esc/q)
  ?              show this help
  q / Esc        return to the Directory Tree view",
        ),
        HelpTopic::Config => Text::from(
            "  j / k, Down / Up   move the selection (skips unavailable tools)
  Enter / Space      select Auto, Disabled, or an available tool,
                     or toggle Check for updates / Mouse support / Theme
                     / Scan mode / Respect .gitignore; Global exclusions opens
                     a list editor (a add, Enter edit, d delete, r restore
                     defaults, J/K reorder, Ctrl+s apply + one rescan, Esc cancel;
                     the list grows with the terminal and scrolls with the selection)
  T                  toggle light/dark theme from anywhere (persists)
  h / l, Left / Right  adjust the Diff context line count
  ?                  show this help
  q / Esc            return to the screen you opened Config from

  External diff tool choices: Auto (resolves the first launchable tool
  by fixed priority: vim, nvim, code, meld, bcomp, smerge, ksdiff, difft),
  Disabled, or a pinned tool. Unavailable tools are shown as [-] and
  cannot be selected.

  Settings are saved to ~/.config/duodiff/config.toml (honors
  XDG_CONFIG_HOME). See config.example.toml in the repo for every
  field, its default, and what it does.",
        ),
        HelpTopic::Mouse => Text::from(
            "  Left Click     select the clicked row
  Right Click    select a row and open the Command Palette
  Double Click   open diff view for a file, or expand/collapse a directory
  Scroll         scroll the directory tree, diff lines, Config screen, Help
                 topic/index, or the menu/palette list; over the Config
                 screen's Diff context row, scroll adjusts its value

  Mouse is on by default; disable it in Config, in config.toml
  (mouse = false), or for one session with --no-mouse.",
        ),
        HelpTopic::General => Text::from(
            "  ; / Ctrl+p    open the Command Palette (right-click does too);
                 type to search every command for the current screen,
                 Up/Down to select, Enter to run, Esc or Ctrl+p to close
  ?              show this help
  q / Esc        quit (or back, on any sub-screen); in the Directory Tree
                 Esc clears an applied filter before it will quit
  T              toggle light/dark theme (persists across restart)
  Tab            (inside Help) open the topic index list
  1-6            (inside Help) jump straight to a topic",
        ),
        HelpTopic::About => {
            let repo = env!("CARGO_PKG_REPOSITORY")
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let mut lines = vec![
                Line::from(format!("duodiff v{}", env!("CARGO_PKG_VERSION"))),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        repo.to_string(),
                        Style::default()
                            .fg(theme.fg)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ]),
                Line::from(""),
            ];
            if let Some(version) = update_available {
                lines.push(Line::from(crate::upgrade::update_hint(
                    version,
                    install_method,
                )));
            }
            Text::from(lines)
        }
    }
}

/// Render the Help screen.
///
/// Shell: top bar + footer. Body paints through [`draw_help_content`].
pub fn draw_help(f: &mut Frame, app: &App) {
    let theme = app.theme();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    draw_top_bar(f, app, chunks[0]);

    let view = app.help_view();
    draw_help_content(f, &view, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

/// Paint the Help body (topic index list or scrolled topic text + close button).
pub fn draw_help_content(f: &mut Frame, view: &HelpView<'_>, body_area: Rect) {
    let theme = view.theme;
    if view.index_open {
        let items: Vec<ListItem> = HelpTopic::all()
            .iter()
            .enumerate()
            .map(|(i, t)| ListItem::new(format!("  {}  {}", i + 1, t.title())))
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title("Help — pick a topic (1-6 / j/k Enter · Esc back)")
                    .borders(Borders::ALL),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg),
            );
        let mut list_state = ListState::default();
        list_state.select(Some(view.index_sel));
        f.render_stateful_widget(list, body_area, &mut list_state);
    } else {
        let title = format!(
            "Help · {} — Tab topics · j/k scroll · Esc back",
            view.topic.title()
        );
        let paragraph = Paragraph::new(help_topic_body(
            view.topic,
            theme,
            view.update_available,
            view.install_method,
        ))
        .scroll((view.scroll, 0))
        .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(paragraph, body_area);
    }

    draw_close_button(f, body_area);
}

/// Render state for the Config **list** region.
///
/// Built by [`App::config_view`] after `ensure_config_selection` (shell-side).
/// `rows` is owned because [`App::config_rows`] already allocates a fresh list.
#[derive(Clone, Debug)]
pub struct ConfigView<'a> {
    pub rows: Vec<crate::app::ConfigRowKind>,
    pub selected_idx: usize,
    pub detected_diff_tools: &'a [(crate::diff_tool::ExternalDiffTool, bool)],
    pub diff_tool_setting: &'a crate::settings::DiffToolSetting,
    pub resolved_auto_tool: Option<crate::diff_tool::ExternalDiffTool>,
    pub check_updates: bool,
    pub mouse: bool,
    pub theme_choice: crate::theme::ThemeChoice,
    pub diff_context: usize,
    /// Effective scan mode for this session.
    pub scan_mode: crate::settings::ScanMode,
    /// Persisted scan mode. Differs from `scan_mode` only while `--scan-mode`
    /// overrides it, which the row annotates as a session override (Issue #238).
    pub saved_scan_mode: crate::settings::ScanMode,
    pub respect_gitignore: bool,
    pub global_exclusion_count: usize,
    pub cli_exclusion_count: usize,
    pub left_ignore_source: String,
    pub right_ignore_source: String,
    pub theme: Theme,
}

/// Snapshot of the Global exclusions editor for pure rendering.
#[derive(Clone, Debug)]
pub struct ExclusionEditorView {
    pub draft: Vec<String>,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub editing: bool,
    pub input: crate::text_input::TextInput,
    pub theme: Theme,
}

/// Contextual title for the Config block, displaying row-specific control hints.
///
/// On narrow terminals, drops lower-priority hints as whole units while reserving space
/// for the close button.
pub fn config_title(row: Option<crate::app::ConfigRowKind>, available_width: usize) -> String {
    let hints: &[&'static str] = match row {
        Some(
            crate::app::ConfigRowKind::DiffToolAuto
            | crate::app::ConfigRowKind::DiffToolDisabled
            | crate::app::ConfigRowKind::DiffTool {
                available: true, ..
            }
            | crate::app::ConfigRowKind::GlobalExclusions,
        ) => &[
            "j/k move · Enter/Space select · Esc back",
            "Enter/Space select · Esc back",
            "Enter/Space select",
        ],
        Some(
            crate::app::ConfigRowKind::CheckUpdates
            | crate::app::ConfigRowKind::Mouse
            | crate::app::ConfigRowKind::Theme
            | crate::app::ConfigRowKind::ScanMode
            | crate::app::ConfigRowKind::RespectGitignore,
        ) => &[
            "j/k move · Enter/Space toggle · Esc back",
            "Enter/Space toggle · Esc back",
            "Enter/Space toggle",
        ],
        Some(crate::app::ConfigRowKind::DiffContext) => &[
            "j/k move · h/l adjust · Esc back",
            "h/l adjust · Esc back",
            "h/l adjust",
        ],
        Some(
            crate::app::ConfigRowKind::DiffTool {
                available: false, ..
            }
            | crate::app::ConfigRowKind::DiffToolUnknown,
        ) => &["Not Found — install or choose another tool"],
        _ => &[],
    };

    for hint in hints {
        let candidate = format!(" Config — {hint} ");
        if candidate.chars().count() <= available_width {
            return candidate;
        }
    }

    if " Config ".chars().count() <= available_width {
        " Config ".to_string()
    } else if "Config".chars().count() <= available_width {
        "Config".to_string()
    } else {
        String::new()
    }
}

/// Render the Config screen.
///
/// Shell: top bar, `ensure_config_selection`, footer. List paints through
/// [`draw_config_content`].
pub fn draw_config(f: &mut Frame, app: &mut App) {
    let theme = app.theme();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_top_bar(f, app, chunks[0]);
    // Mut side effect stays on the shell so content can be pure-read.
    app.ensure_config_selection();
    let view = app.config_view();
    draw_config_content(f, &view, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

/// Paint the Config list + close button (no top bar / footer).
pub fn draw_config_content(f: &mut Frame, view: &ConfigView<'_>, body_area: Rect) {
    let theme = view.theme;
    let mut items = Vec::new();
    for (row_idx, row) in view.rows.iter().enumerate() {
        let style = if row_idx == view.selected_idx {
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
        } else {
            Style::default()
        };
        match row {
            crate::app::ConfigRowKind::Header(label) => {
                items.push(ListItem::new(Line::from(Span::styled(
                    *label,
                    Style::default().fg(theme.warn).bold(),
                ))));
            }
            crate::app::ConfigRowKind::DiffToolAuto => {
                let is_active = view.diff_tool_setting.is_auto();
                let marker = if is_active { "[x] " } else { "[ ] " };
                let resolved = view
                    .resolved_auto_tool
                    .map(|t| t.as_str())
                    .unwrap_or("none");
                items.push(ListItem::new(format!("  {}Auto ({})", marker, resolved)).style(style));
            }
            crate::app::ConfigRowKind::DiffToolDisabled => {
                let is_active = view.diff_tool_setting.is_disabled();
                let marker = if is_active { "[x] " } else { "[ ] " };
                items.push(ListItem::new(format!("  {}Disabled", marker)).style(style));
            }
            crate::app::ConfigRowKind::DiffTool { idx, available } => {
                let (tool, _) = &view.detected_diff_tools[*idx];
                let is_active = view.diff_tool_setting.pinned() == Some(*tool);
                if *available {
                    let marker = if is_active { "[x] " } else { "[ ] " };
                    items.push(
                        ListItem::new(format!("  {}{:<5} (Available)", marker, tool.as_str()))
                            .style(style),
                    );
                } else {
                    let marker = if is_active { "[x] " } else { "[-] " };
                    let dim_style = if row_idx == view.selected_idx {
                        style
                    } else {
                        Style::default().fg(theme.muted)
                    };
                    items.push(
                        ListItem::new(format!("  {}{:<5} (Not Found)", marker, tool.as_str()))
                            .style(dim_style),
                    );
                }
            }
            crate::app::ConfigRowKind::DiffToolUnknown => {
                let name = view.diff_tool_setting.unknown_name().unwrap_or("unknown");
                let dim_style = if row_idx == view.selected_idx {
                    style
                } else {
                    Style::default().fg(theme.muted)
                };
                items.push(
                    ListItem::new(format!("  [x] Unknown tool: {} (Not Found)", name))
                        .style(dim_style),
                );
            }
            crate::app::ConfigRowKind::CheckUpdates => {
                let marker = if view.check_updates { "[x] " } else { "[ ] " };
                items.push(
                    ListItem::new(format!("  {}Check for updates daily", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::Mouse => {
                let marker = if view.mouse { "[x] " } else { "[ ] " };
                items.push(ListItem::new(format!("  {}Enable mouse support", marker)).style(style));
            }
            crate::app::ConfigRowKind::Theme => {
                let marker = if view.theme_choice == crate::theme::ThemeChoice::Light {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(
                    ListItem::new(format!("  {}Light theme (off = dark)", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::DiffContext => {
                items.push(
                    ListItem::new(format!(
                        "      Diff context: {} lines (h/l to adjust)",
                        view.diff_context
                    ))
                    .style(style),
                );
            }
            crate::app::ConfigRowKind::ScanMode => {
                let mut label = format!(
                    "      Scan mode: {} (Enter to switch)",
                    view.scan_mode.label()
                );
                if view.scan_mode != view.saved_scan_mode {
                    label.push_str(&format!(
                        "  ·  session override; saved default: {}",
                        view.saved_scan_mode.label()
                    ));
                }
                items.push(ListItem::new(label).style(style));
            }
            crate::app::ConfigRowKind::RespectGitignore => {
                let marker = if view.respect_gitignore {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(ListItem::new(format!("  {}Respect .gitignore", marker)).style(style));
            }
            crate::app::ConfigRowKind::GlobalExclusions => {
                items.push(
                    ListItem::new(format!(
                        "      Global exclusions: {} rules (Enter to edit)",
                        view.global_exclusion_count
                    ))
                    .style(style),
                );
            }
            crate::app::ConfigRowKind::IgnoreSources => {
                let inner_width = body_area.width.saturating_sub(2) as usize;
                let muted = Style::default().fg(theme.muted);
                let raw_lines = [
                    "      Sources (read-only)".to_string(),
                    format!("        Left {}", view.left_ignore_source),
                    format!("        Right {}", view.right_ignore_source),
                    format!("        CLI: {} rules", view.cli_exclusion_count),
                ];
                let mut lines = Vec::new();
                for raw in raw_lines {
                    for chunk in wrap_text(&raw, inner_width.max(1)) {
                        lines.push(Line::from(chunk));
                    }
                }
                items.push(ListItem::new(lines).style(muted));
            }
        }
    }

    let available_width = body_area.width.saturating_sub(6) as usize;
    let selected_row = view.rows.get(view.selected_idx).copied();
    let title = config_title(selected_row, available_width);

    let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(list, body_area);
    draw_close_button(f, body_area);
}

const EXCLUSION_EDITOR_MIN_WIDTH: u16 = 32;
const EXCLUSION_EDITOR_MAX_WIDTH: u16 = 96;
/// Two borders plus the compact key legend.
const EXCLUSION_EDITOR_CHROME_HEIGHT: u16 = 3;

/// Popup geometry for the Global exclusions editor.
struct ExclusionEditorLayout {
    popup: Rect,
    hint: Rect,
    list: Rect,
}

impl ExclusionEditorLayout {
    fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

fn exclusion_editor_layout(item_count: usize, area: Rect) -> ExclusionEditorLayout {
    let width = area
        .width
        .saturating_sub(4)
        .clamp(EXCLUSION_EDITOR_MIN_WIDTH, EXCLUSION_EDITOR_MAX_WIDTH)
        .min(area.width);
    let wanted = EXCLUSION_EDITOR_CHROME_HEIGHT.saturating_add(item_count.max(1) as u16);
    let height = wanted
        .min(area.height.saturating_sub(2))
        .max(EXCLUSION_EDITOR_CHROME_HEIGHT.min(area.height));
    let popup = centered_rect(width, height, area);
    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let hint = Rect {
        height: inner.height.min(1),
        ..inner
    };
    let list = Rect {
        y: inner.y.saturating_add(hint.height),
        height: inner.height.saturating_sub(hint.height),
        ..inner
    };
    ExclusionEditorLayout { popup, hint, list }
}

fn exclusion_editor_hint(editing: bool, theme: Theme) -> Line<'static> {
    let key = |label: &'static str| Span::styled(label, Style::default().fg(theme.accent).bold());
    let sep = || Span::raw("  ·  ");
    if editing {
        return Line::from(vec![
            key("⏎"),
            Span::raw("  confirm"),
            sep(),
            key("Esc"),
            Span::raw("  abort"),
        ]);
    }
    Line::from(vec![
        key("a"),
        Span::raw(" +"),
        sep(),
        key("⏎"),
        Span::raw(" ✎"),
        sep(),
        key("d"),
        Span::raw(" −"),
        sep(),
        key("r"),
        Span::raw(" ↺"),
        sep(),
        key("J/K"),
        Span::raw(" ↕"),
        sep(),
        key("^s"),
        Span::raw(" ✓"),
        sep(),
        key("Esc"),
    ])
}

fn draw_exclusion_editor(
    f: &mut Frame,
    editor: &ExclusionEditorView,
    layout: &ExclusionEditorLayout,
) {
    let theme = editor.theme;
    f.render_widget(ClearOverlay, layout.popup);
    let block = Block::default()
        .title(" Global exclusions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent));
    f.render_widget(block, layout.popup);
    f.render_widget(
        Paragraph::new(exclusion_editor_hint(editor.editing, theme)),
        layout.hint,
    );

    let budget = layout.list.width as usize;
    let mut lines = Vec::new();
    if editor.draft.is_empty() {
        lines.push(Line::from(
            "  (empty: built-in defaults are disabled when applied)",
        ));
    } else {
        let visible = layout.visible_rows();
        let start = editor
            .scroll_offset
            .min(editor.draft.len().saturating_sub(1));
        for (idx, pattern) in editor.draft.iter().enumerate().skip(start).take(visible) {
            let selected = idx == editor.selected_idx;
            let prefix = if selected { "> " } else { "  " };
            if editor.editing && selected {
                let mut spans = vec![Span::raw(prefix.to_string())];
                spans.extend(text_input_spans(
                    &editor.input,
                    Style::default().fg(theme.selection_fg),
                ));
                lines.push(
                    Line::from(spans).style(
                        Style::default()
                            .bg(theme.selection_bg)
                            .fg(theme.selection_fg)
                            .bold(),
                    ),
                );
            } else {
                lines.push(Line::from(Span::styled(
                    truncate_to_width(&format!("{prefix}{pattern}"), budget),
                    if selected {
                        Style::default().fg(theme.accent).bold()
                    } else {
                        Style::default()
                    },
                )));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), layout.list);
}

/// The `[x]` close button's rectangle within `area`, or `None` if `area` is too
/// narrow to fit it. Shared by `draw_close_button` (render) and every close-button
/// hit test, so the two cannot drift apart.
pub fn close_button_rect(area: Rect) -> Option<Rect> {
    if area.width < 6 {
        return None;
    }
    Some(Rect {
        x: area.x + area.width.saturating_sub(5),
        y: area.y,
        width: 3,
        height: 1,
    })
}

pub fn draw_close_button(f: &mut Frame, area: Rect) {
    if let Some(button_area) = close_button_rect(area) {
        f.render_widget(Paragraph::new(Span::raw("[x]")), button_area);
    }
}

pub fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((parent.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(parent);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((parent.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(popup_layout[1])[1]
}

/// The Command Palette popup's geometry, clamped to the terminal.
///
/// One source of truth for both painting ([`draw_palette_content`]) and mouse
/// hit-testing ([`crate::input::handle_mouse`]), so a click can never land on a
/// row the renderer put somewhere else (Issue #239).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteLayout {
    /// The whole popup, borders included.
    pub popup: Rect,
    /// The query input line.
    pub query: Rect,
    /// The rule between the query and the list.
    pub separator: Rect,
    /// The action rows. Its height is how many items fit at once.
    pub list: Rect,
}

impl PaletteLayout {
    /// How many action rows are visible at once.
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

/// Popup width bounds. The palette takes four fifths of the terminal within
/// these limits, so a disabled action's reason still fits beside its label.
const PALETTE_MIN_WIDTH: u16 = 40;
const PALETTE_MAX_WIDTH: u16 = 96;
/// Popup chrome that is never an action row: two borders, query, separator.
const PALETTE_CHROME_HEIGHT: u16 = 4;

/// Lay out the palette popup for `item_count` actions inside `area`, clamping
/// both dimensions so it always fits the terminal.
pub fn palette_layout(item_count: usize, area: Rect) -> PaletteLayout {
    let width = (area.width * 4 / 5)
        .clamp(PALETTE_MIN_WIDTH, PALETTE_MAX_WIDTH)
        .min(area.width);
    // Always keep room for one row — the "No matching commands" notice needs it.
    let wanted_rows = (item_count.max(1) as u16).saturating_add(PALETTE_CHROME_HEIGHT);
    let height = wanted_rows
        .min(area.height)
        .max(PALETTE_CHROME_HEIGHT.min(area.height));
    let popup = centered_rect(width, height, area);

    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let query = Rect {
        height: inner.height.min(1),
        ..inner
    };
    let separator = Rect {
        y: inner.y.saturating_add(1),
        height: inner.height.saturating_sub(1).min(1),
        ..inner
    };
    let list = Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(2),
        ..inner
    };
    PaletteLayout {
        popup,
        query,
        separator,
        list,
    }
}

use crate::commands::CommandEntry;

/// Borrowed render state for the Command Palette popup.
///
/// Built by [`App::palette_view`] after the shell has refreshed the item list and
/// synced the viewport.
#[derive(Clone, Copy, Debug)]
pub struct PaletteView<'a> {
    pub items: &'a [CommandEntry],
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub query: &'a str,
    pub theme: Theme,
}

/// Shown in place of the list when the query matches nothing. Non-selectable.
pub const PALETTE_NO_MATCH: &str = "No matching commands";

/// Shown in the directory tree panes when neither side has visible entries. Non-selectable.
pub const TREE_NO_VISIBLE_ENTRIES: &str = "No visible entries";

/// Truncate `text` to `max_width` terminal columns, appending `…` when it does
/// not fit. Measured in display width, so CJK and emoji do not overflow the popup.
fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if str_column_width(text) <= max_width {
        return text.to_string();
    }
    format!(
        "{}…",
        take_prefix_by_width(text, max_width.saturating_sub(1))
    )
}

fn char_column_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

fn str_column_width(text: &str) -> usize {
    text.chars().map(char_column_width).sum()
}

fn take_prefix_by_width(text: &str, max_width: usize) -> &str {
    let mut used = 0usize;
    let mut end = 0usize;
    for (i, c) in text.char_indices() {
        let w = char_column_width(c);
        if used + w > max_width {
            break;
        }
        used += w;
        end = i + c.len_utf8();
    }
    &text[..end]
}

fn take_suffix_by_width(text: &str, max_width: usize) -> &str {
    let mut used = 0usize;
    let mut start = text.len();
    for (i, c) in text.char_indices().rev() {
        let w = char_column_width(c);
        if used + w > max_width {
            break;
        }
        used += w;
        start = i;
    }
    &text[start..]
}

/// Two-column prefix that marks a directory's expand state and keeps files
/// aligned with their siblings. Files carry no icon, so the tree reads as
/// structure rather than decoration.
fn disclosure_marker(is_dir: bool, is_expanded: bool) -> &'static str {
    match (is_dir, is_expanded) {
        (false, _) => "  ",
        (true, true) => "\u{25be} ",
        (true, false) => "\u{25b8} ",
    }
}

/// Entry name as shown in a pane: directories carry a trailing separator so
/// they stay recognizable once the disclosure marker scrolls out of a
/// breadcrumb or a truncated cell.
fn entry_display_name(name: &str, is_dir: bool) -> String {
    if is_dir {
        format!("{name}/")
    } else {
        name.to_string()
    }
}

fn format_tree_cell(indent: &str, icon: &str, name: &str, inner_width: usize) -> String {
    let prefix_width = str_column_width(indent) + str_column_width(icon);
    let name = truncate_filename_middle(name, inner_width.saturating_sub(prefix_width));
    format!("{indent}{icon}{name}")
}

/// Format a breadcrumb `parent/path › name` abbreviated in the middle using
/// Unicode display width, preserving the basename and nearest parent first.
pub fn format_breadcrumb(parent: &std::path::Path, name: &str, max_width: usize) -> String {
    let parent_str = parent.to_string_lossy();
    if parent.as_os_str().is_empty() || parent_str.is_empty() {
        return truncate_filename_middle(name, max_width);
    }
    let sep = " › ";
    let full = format!("{parent_str}{sep}{name}");
    if str_column_width(&full) <= max_width {
        return full;
    }

    if max_width == 0 {
        return String::new();
    }

    let b_len = str_column_width(name);
    let sep_len = str_column_width(sep); // 3

    // If max_width cannot even fit `… › {name}`:
    if max_width < 1 + sep_len + b_len {
        if max_width >= 6 {
            let budget = max_width.saturating_sub(4); // `… › ` is 4 columns
            return format!("… › {}", truncate_filename_middle(name, budget));
        } else {
            return truncate_filename_middle(name, max_width);
        }
    }

    let parent_avail = max_width.saturating_sub(sep_len + b_len);

    let components: Vec<&str> = parent
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();

    if components.is_empty() {
        return truncate_filename_middle(name, max_width);
    }

    let last_comp = components[components.len() - 1];
    let last_len = str_column_width(last_comp);

    if components.len() == 1 {
        if last_len <= parent_avail {
            return format!("{last_comp} › {name}");
        } else if parent_avail >= 3 {
            let t_last = truncate_filename_middle(last_comp, parent_avail.saturating_sub(2));
            return format!("…/{t_last} › {name}");
        } else {
            return format!("… › {name}");
        }
    }

    if 2 + last_len > parent_avail {
        if parent_avail >= 3 {
            let t_last = truncate_filename_middle(last_comp, parent_avail.saturating_sub(2));
            return format!("…/{t_last} › {name}");
        } else {
            return format!("… › {name}");
        }
    }

    let mut leading = Vec::new();
    let mut leading_width = 0;

    for comp in &components[..components.len() - 1] {
        let comp_w = str_column_width(comp);
        let new_leading_w = if leading.is_empty() {
            comp_w
        } else {
            leading_width + 1 + comp_w
        };
        let total_needed = new_leading_w + 3 + last_len;
        if total_needed <= parent_avail {
            leading.push(*comp);
            leading_width = new_leading_w;
        } else {
            break;
        }
    }

    if leading.is_empty() {
        format!("…/{last_comp} › {name}")
    } else {
        let prefix = leading.join("/");
        format!("{prefix}/…/{last_comp} › {name}")
    }
}

/// Truncate a file name to `max_width` terminal columns by inserting `…` between
/// a prefix and a tail so both ends stay visible. Unchanged when it already fits.
fn truncate_filename_middle(name: &str, max_width: usize) -> String {
    let total = str_column_width(name);
    if total <= max_width {
        return name.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let remaining = max_width - 1;
    let head_budget = remaining / 2;
    let tail_budget = remaining - head_budget;
    format!(
        "{}…{}",
        take_prefix_by_width(name, head_budget),
        take_suffix_by_width(name, tail_budget)
    )
}

/// Render the palette popup.
///
/// Shell: refresh the item list and sync the viewport against the laid-out list
/// height (both mut). Content paints through [`draw_palette_content`].
pub fn draw_palette(f: &mut Frame, app: &mut App) {
    app.refresh_palette_items();
    let layout = palette_layout(app.palette().items.len(), f.area());
    app.sync_palette_viewport(layout.visible_rows());
    let view = app.palette_view();
    draw_palette_content(f, &view, f.area());
}

/// Paint the palette popup inside `frame_area` (computes its own geometry).
pub fn draw_palette_content(f: &mut Frame, view: &PaletteView<'_>, frame_area: Rect) {
    let theme = view.theme;
    let layout = palette_layout(view.items.len(), frame_area);
    f.render_widget(ClearOverlay, layout.popup);

    let block = Block::default()
        .title(Span::styled(" Command Palette ", Style::default().bold()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn));
    f.render_widget(block, layout.popup);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Search: ", Style::default().fg(theme.accent)),
            Span::raw(view.query),
            Span::styled("█", Style::default().fg(theme.emphasis)),
        ])),
        layout.query,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(layout.separator.width as usize),
            Style::default().fg(theme.dim),
        ))),
        layout.separator,
    );

    if view.items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  {}", PALETTE_NO_MATCH),
                Style::default().fg(theme.dim),
            ))),
            layout.list,
        );
        draw_close_button(f, layout.popup);
        return;
    }

    // Two columns of chrome ("  ") plus the key column and its gap.
    let key_width = 10usize;
    let text_budget = (layout.list.width as usize).saturating_sub(key_width + 4);
    let mut list_items = Vec::new();
    for (i, action) in view
        .items
        .iter()
        .enumerate()
        .skip(view.scroll_offset)
        .take(layout.visible_rows())
    {
        let label = match action.disabled_reason {
            Some(reason) => format!("{} — {}", action.label, reason),
            None => action.label.clone(),
        };
        let display_text = format!(
            "  {:<key_width$}  {}",
            truncate_to_width(&action.key, key_width),
            truncate_to_width(&label, text_budget),
            key_width = key_width,
        );
        let mut style = if i == view.selected_idx {
            Style::default().bg(theme.info).fg(theme.selection_fg)
        } else {
            Style::default()
        };
        if !action.enabled() {
            style = style.fg(theme.dim);
        }
        list_items.push(ListItem::new(display_text).style(style));
    }
    f.render_widget(List::new(list_items), layout.list);
    draw_close_button(f, layout.popup);
}

/// Borrowed confirm-dialog state (headline + body + theme).
#[derive(Clone, Copy, Debug)]
pub struct ConfirmView<'a> {
    pub title: &'a str,
    /// The one sentence stating what the default choice will do.
    pub headline: &'a str,
    pub lines: &'a [String],
    pub choices: &'a [crate::app::ConfirmChoice],
    pub theme: Theme,
}

/// Render the confirm modal from an [`App`].
pub fn draw_confirm_modal(f: &mut Frame, app: &App) {
    let view = app.confirm_view();
    draw_confirm_content(f, &view, f.area());
}

/// Paint the confirm popup (no full `App` required).
///
/// Sizes itself to the body and clamps to the terminal, and wraps every body
/// line, so a long path or a three-button dialog stays readable at narrow
/// widths (Issue #235).
pub fn draw_confirm_content(f: &mut Frame, view: &ConfirmView<'_>, frame_area: Rect) {
    let theme = view.theme;
    let width = (frame_area.width * 3 / 4)
        .clamp(CONFIRM_MIN_WIDTH, CONFIRM_MAX_WIDTH)
        .min(frame_area.width);
    // Two borders plus the horizontal padding on each side.
    let inner_width = (width as usize)
        .saturating_sub(2 + 2 * CONFIRM_PAD_X as usize)
        .max(1);

    let mut body: Vec<Line> = Vec::new();
    if !view.headline.is_empty() {
        for chunk in wrap_plain(view.headline, inner_width) {
            body.push(Line::from(Span::styled(chunk, Style::default().bold())));
        }
        if !view.lines.is_empty() {
            body.push(Line::from(""));
        }
    }
    for line in view.lines {
        if line.is_empty() {
            body.push(Line::from(""));
            continue;
        }
        for chunk in wrap_with_hanging_indent(line, inner_width) {
            body.push(Line::from(Span::raw(chunk)));
        }
    }
    if !body.is_empty() {
        body.push(Line::from(""));
    }
    body.extend(confirm_button_rows(view.choices, inner_width, theme));

    // Two borders plus the vertical padding above and below the body.
    let height = (body.len() as u16 + 2 + 2 * CONFIRM_PAD_Y).min(frame_area.height);
    let area = centered_rect(width, height, frame_area);
    f.render_widget(ClearOverlay, area);

    let block = Block::default()
        .title(format!(" {} ", view.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn))
        .padding(Padding::symmetric(CONFIRM_PAD_X, CONFIRM_PAD_Y));

    f.render_widget(
        Paragraph::new(body)
            .block(block)
            .style(Style::default().fg(theme.fg)),
        area,
    );
}

/// Wrap `text` to `width`, indenting continuations under the column the value
/// starts in — a wrapped path then reads as one field or one list item instead
/// of restarting in the label column.
fn wrap_with_hanging_indent(text: &str, width: usize) -> Vec<String> {
    let indent = hanging_indent_width(text).filter(|i| *i > 0 && *i * 2 < width);
    let Some(indent) = indent else {
        return wrap_plain(text, width);
    };
    let mut out = Vec::new();
    let first = take_prefix_by_width(text, width);
    out.push(first.to_string());
    let mut rest = &text[first.len()..];
    let pad = " ".repeat(indent);
    while !rest.is_empty() {
        let chunk = take_prefix_by_width(rest, width - indent);
        if chunk.is_empty() {
            break;
        }
        out.push(format!("{pad}{chunk}"));
        rest = &rest[chunk.len()..];
    }
    out
}

/// The column a wrapped continuation should start in: past a leading indent,
/// and past a `Label` plus the two-or-more spaces separating it from its value
/// when the line has that shape.
fn hanging_indent_width(text: &str) -> Option<usize> {
    let lead = text.len() - text.trim_start_matches(' ').len();
    let rest = &text[lead..];
    let Some(label_end) = rest.find("  ") else {
        return Some(lead);
    };
    if label_end == 0 {
        return Some(lead);
    }
    let Some(value_offset) = rest[label_end..].find(|c: char| c != ' ') else {
        return Some(lead);
    };
    Some(str_column_width(&text[..lead + label_end + value_offset]))
}

/// Horizontal and vertical padding between the confirm popup's border and its
/// text, so a path never runs into the frame.
const CONFIRM_PAD_X: u16 = 2;
const CONFIRM_PAD_Y: u16 = 1;
/// The popup tracks three quarters of the terminal between these bounds, so a
/// long path stays on one line without the dialog sprawling on a large screen.
const CONFIRM_MIN_WIDTH: u16 = 52;
const CONFIRM_MAX_WIDTH: u16 = 96;
/// Blank columns between two adjacent choice chips.
const CONFIRM_BUTTON_GAP: usize = 2;

/// Lay the choice chips out centered, wrapping onto further lines rather than
/// clipping when they do not all fit — every way out of a dialog has to stay
/// reachable on a small terminal (Issue #235).
fn confirm_button_rows(
    choices: &[crate::app::ConfirmChoice],
    inner_width: usize,
    theme: Theme,
) -> Vec<Line<'static>> {
    let mut rows: Vec<Line> = Vec::new();
    let mut current: Vec<Span> = Vec::new();
    let mut used = 0usize;

    for (i, choice) in choices.iter().enumerate() {
        // The first choice is what Enter picks, so it carries the default's
        // emphasis rather than leaving the default invisible.
        let text = format!(" [{}] {} ", choice.key.to_ascii_uppercase(), choice.label);
        let style = if i == 0 {
            Style::default()
                .bg(theme.accent)
                .fg(theme.selection_fg)
                .bold()
        } else {
            Style::default().fg(theme.accent).bold()
        };
        let chip_width = str_column_width(&text);
        let gap = if current.is_empty() {
            0
        } else {
            CONFIRM_BUTTON_GAP
        };
        if !current.is_empty() && used + gap + chip_width > inner_width {
            rows.push(Line::from(std::mem::take(&mut current)).alignment(Alignment::Center));
            used = 0;
        } else if gap > 0 {
            current.push(Span::raw(" ".repeat(gap)));
            used += gap;
        }
        current.push(Span::styled(text, style));
        used += chip_width;
    }
    if !current.is_empty() {
        rows.push(Line::from(current).alignment(Alignment::Center));
    }
    rows
}

/// Clears an overlay's bounding area, padding any double-width character that
/// straddles the left boundary with a space and cleaning up any orphaned continuation
/// cell on the right boundary so overlay panel borders are not eaten (Issue #244).
pub struct ClearOverlay;

impl Widget for ClearOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        clear_overlay(buf, area);
    }
}

/// Clear `area` on `buf`, padding wide characters straddling its boundaries.
pub fn clear_overlay(buf: &mut Buffer, area: Rect) {
    let buf_area = *buf.area();
    let area = area.intersection(buf_area);
    if area.is_empty() {
        return;
    }

    // If there is a column immediately to the left of the overlay area,
    // check if any cell on that column contains a double-width character that
    // spans into the overlay's first column (`area.left()`).
    // If so, replace that cell's symbol with a space so it does not bleed into the overlay.
    if area.left() > buf_area.left() {
        let left_col = area.left() - 1;
        for y in area.top()..area.bottom() {
            if let Some(cell) = buf.cell_mut((left_col, y)) {
                if UnicodeWidthStr::width(cell.symbol()) > 1 {
                    cell.set_symbol(" ");
                }
            }
        }
    }

    // If there is a column immediately to the right of the overlay area,
    // check if any cell on that column was a continuation cell (`""`) of a wide
    // character from `area.right() - 1` that is now being cleared.
    // If so, reset it to a space so it does not remain an orphaned zero-width cell.
    if area.right() < buf_area.right() {
        let right_col = area.right();
        for y in area.top()..area.bottom() {
            if let Some(cell) = buf.cell_mut((right_col, y)) {
                if cell.symbol().is_empty() {
                    cell.set_symbol(" ");
                }
            }
        }
    }

    // Clear all cells within the overlay area.
    for x in area.left()..area.right() {
        for y in area.top()..area.bottom() {
            buf[(x, y)].reset();
        }
    }
}

/// Hard-wrap `text` to `width` display columns on character boundaries.
fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(c);
        used += w;
    }
    out.push(current);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    /// Render one frame the way the event loop does: sync the viewport for the
    /// current terminal size first, then draw. Drawing without the sync would
    /// render against stale (on the first frame, zero-sized) geometry.
    fn draw_frame(terminal: &mut Terminal<TestBackend>, app: &mut App) {
        app.sync_viewport(terminal.size().unwrap().into());
        terminal.draw(|f| draw(f, app)).unwrap();
    }

    /// `(visible_height, content_width)` for a [`DiffLayout`]'s left pane, the same way
    /// `App::sync_viewport` derives it — used by diff-content tests that build their own
    /// `DiffView`/`DiffLayout` via [`diff_layout`] instead of a full `App`.
    fn diff_content_geometry(
        layout: &DiffLayout,
        rows: &[crate::diff_view::DiffRow],
    ) -> (usize, usize) {
        let pane_inner = layout.left.width.saturating_sub(2) as usize;
        (
            layout.left.height.saturating_sub(2) as usize,
            crate::diff_view::diff_text_width(
                pane_inner,
                crate::diff_view::diff_side_line_count(rows, true),
                crate::diff_view::diff_side_line_count(rows, false),
            ),
        )
    }

    /// Owned data backing a hand-built [`DiffView`] for content-only diff tests, so each
    /// test only spells out what it actually varies (rows, theme, hashes, ...) instead of
    /// repeating the same defaulted fields (`left_root`/`right_root`/`install_method`/etc.).
    struct DiffViewFixture {
        rows: Vec<crate::diff_view::DiffRow>,
        flat: FlatRow,
        left_root: PathBuf,
        right_root: PathBuf,
        method: crate::upgrade::InstallMethod,
        theme: Theme,
        left_hash: Option<String>,
        right_hash: Option<String>,
    }

    impl DiffViewFixture {
        fn new(rows: Vec<crate::diff_view::DiffRow>, flat: FlatRow) -> Self {
            Self {
                rows,
                flat,
                left_root: PathBuf::from("/left"),
                right_root: PathBuf::from("/right"),
                method: crate::upgrade::InstallMethod::Standalone,
                theme: Theme::DARK,
                left_hash: None,
                right_hash: None,
            }
        }

        /// Same rule `FileDiffState::has_changes` uses: at least one added/removed line.
        fn has_changes(&self) -> bool {
            self.rows.iter().any(crate::diff_view::diff_row_is_change)
        }

        fn view(
            &self,
            wrap: bool,
            scroll: usize,
            h_scroll: usize,
            visible_height: usize,
            content_width: usize,
        ) -> DiffView<'_> {
            DiffView {
                rows: &self.rows,
                wrap,
                scroll,
                h_scroll,
                visible_height,
                content_width,
                left_line_count: crate::diff_view::diff_side_line_count(&self.rows, true),
                right_line_count: crate::diff_view::diff_side_line_count(&self.rows, false),
                left_root: &self.left_root,
                right_root: &self.right_root,
                row: Some(&self.flat),
                left_hash: self.left_hash.as_deref(),
                right_hash: self.right_hash.as_deref(),
                left_line_ending: None,
                right_line_ending: None,
                theme: self.theme,
                status_toast: None,
                has_changes: self.has_changes(),
                update_available: None,
                install_method: &self.method,
                left_dirty: false,
                right_dirty: false,
                can_undo: false,
            }
        }
    }

    #[test]
    fn test_ui_drawing() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        println!("Buffer output:\n{:?}", buffer);

        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("/left"),
            "Left pane title should show [1] before the path"
        );
        assert!(
            buffer_string.contains("[2]") && buffer_string.contains("/right"),
            "Right pane title should show [2] before the path"
        );
        // The State column title was removed; verify indicator symbols render
        assert!(
            !buffer_string.contains("\"State\""),
            "State column title should be removed"
        );
    }

    #[test]
    fn test_disclosure_marker_and_display_name_keep_files_and_directories_aligned() {
        // Every prefix is two columns wide, so both panes stay aligned whatever
        // the row holds.
        assert_eq!(disclosure_marker(true, false), "\u{25b8} ");
        assert_eq!(disclosure_marker(true, true), "\u{25be} ");
        assert_eq!(disclosure_marker(false, false), "  ");
        assert_eq!(disclosure_marker(false, true), "  ");
        for is_dir in [true, false] {
            for expanded in [true, false] {
                assert_eq!(str_column_width(disclosure_marker(is_dir, expanded)), 2);
            }
        }

        assert_eq!(entry_display_name("src", true), "src/");
        assert_eq!(entry_display_name("main.rs", false), "main.rs");
    }

    #[test]
    fn test_draw_tree_marks_rows_without_emoji() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let dir = |is_dir| {
            Some(crate::diff::FileInfo {
                is_dir,
                size: 0,
                modified: std::time::SystemTime::UNIX_EPOCH,
            })
        };
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                relative_path: PathBuf::from("sub"),
                name: "sub".to_string(),
                state: crate::diff::DiffState::DifferentSameTime,
                left: dir(true),
                right: dir(true),
                is_expanded: true,
                ..Default::default()
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("sub/only.txt"),
                name: "only.txt".to_string(),
                state: crate::diff::DiffState::LeftOnly,
                left: dir(false),
                ..Default::default()
            },
            crate::app::FlatRow {
                relative_path: PathBuf::from("clash"),
                name: "clash".to_string(),
                state: crate::diff::DiffState::TypeConflict,
                left: dir(true),
                right: dir(false),
                ..Default::default()
            },
        ]);
        app.apply_filter();

        draw_frame(&mut terminal, &mut app);
        let buffer_string = format!("{:?}", terminal.backend().buffer());

        assert!(
            buffer_string.contains("\u{25be} sub/"),
            "an expanded directory renders its disclosure marker and trailing separator"
        );
        assert!(
            buffer_string.contains("\u{25b8} clash/"),
            "a collapsed directory renders the collapsed marker, per side"
        );
        for emoji in [
            "\u{1f4c1}",
            "\u{1f4c4}",
            "\u{2b05}",
            "\u{27a1}",
            "\u{1f4a5}",
        ] {
            assert!(
                !buffer_string.contains(emoji),
                "the tree must not render emoji: {emoji}"
            );
        }
    }

    #[test]
    fn test_draw_help_topic_body_shows_title_and_bindings() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut()
            .select_topic(crate::app::HelpTopic::DirectoryTree);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Help · Directory Tree — Tab topics · j/k scroll · Esc back"),
            "Help topic-body header should show the topic title and operation hints"
        );
    }

    /// Content seam: top bar from a hand-built [`TopBarView`] (no full `App`).
    #[test]
    fn test_draw_top_bar_content_without_full_app() {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = TopBarView {
            view_mode: ViewMode::DirectoryTree,
            precise_mode: true,
            diff_show_full: false,
            diff_wrap: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            theme: Theme::DARK,
        };
        let area = Rect::new(0, 0, 80, 1);

        terminal
            .draw(|f| draw_top_bar_content(f, &view, area))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Directory Tree") && buffer_string.contains("Precise"),
            "top bar content should show precise tree title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Config") || buffer_string.contains("Help"),
            "top bar content should show Config/Help hints: {buffer_string}"
        );
    }

    /// Issue #250: Top bar shows spinner and scan progress item count when scan is in flight.
    #[test]
    fn test_draw_top_bar_content_scanning_indicator() {
        let backend = TestBackend::new(120, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = TopBarView {
            view_mode: ViewMode::DirectoryTree,
            precise_mode: true,
            diff_show_full: false,
            diff_wrap: false,
            scan_in_progress: true,
            scan_progress_count: 42,
            spinner_frame: 0,
            theme: Theme::DARK,
        };
        let area = Rect::new(0, 0, 120, 1);

        terminal
            .draw(|f| draw_top_bar_content(f, &view, area))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Scanning (42 items)..."),
            "top bar must show scanning count indicator: {buffer_string}"
        );
        assert!(
            buffer_string.contains(spinner_char(0)),
            "top bar must render spinner frame: {buffer_string}"
        );
    }

    /// Issue #250: Tree footer shows spinner and scan progress item count when scan is in flight.
    #[test]
    fn test_draw_tree_footer_scanning_indicator() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: false,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, 120, 20));
        let view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: true,
            scan_progress_count: 128,
            spinner_frame: 2,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Scanning in progress (128 items)... Please wait."),
            "footer must show scanning progress count: {buffer_string}"
        );
        assert!(
            buffer_string.contains(spinner_char(2)),
            "footer must show animated spinner char: {buffer_string}"
        );
    }

    /// Content seam: confirm dialog from a hand-built [`ConfirmView`] (no full `App`).
    #[test]
    fn test_draw_confirm_content_without_full_app() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let choices = vec![
            crate::app::ConfirmChoice {
                key: 'y',
                label: "Yes".to_string(),
                action: crate::app::ConfirmAction::CopyLeftToRight,
            },
            crate::app::ConfirmChoice {
                key: 'n',
                label: "No (Cancel)".to_string(),
                action: crate::app::ConfirmAction::Cancel,
            },
        ];
        let lines: Vec<String> = Vec::new();
        let view = ConfirmView {
            title: "Confirm",
            headline: "Copy foo.txt to right side?",
            lines: &lines,
            choices: &choices,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_confirm_content(f, &view, f.area()))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Confirm"),
            "confirm content should show title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Copy foo.txt to right side?"),
            "confirm content should show message: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[Y]") && buffer_string.contains("[N]"),
            "confirm content should show y/n hints: {buffer_string}"
        );
    }

    #[test]
    fn test_wrap_with_hanging_indent_keeps_a_wrapped_value_in_its_own_column() {
        // A `Label   value` line continues under the value column.
        let wrapped = wrap_with_hanging_indent("From   /aaaa/bbbb/cccc/dddd.txt", 20);
        assert_eq!(
            wrapped,
            vec![
                "From   /aaaa/bbbb/cc".to_string(),
                "       cc/dddd.txt".to_string()
            ]
        );

        // A list item continues under its own indent.
        let wrapped = wrap_with_hanging_indent("  /aaaa/bbbb/cccc/dddd.txt", 16);
        assert_eq!(
            wrapped,
            vec!["  /aaaa/bbbb/ccc".to_string(), "  c/dddd.txt".to_string()]
        );

        // A plain sentence keeps wrapping flush.
        let wrapped = wrap_with_hanging_indent("the destination will be replaced", 12);
        assert_eq!(wrapped[1].trim_start(), wrapped[1]);

        // A label so wide it would squeeze the value falls back to a flush wrap,
        // carrying only the characters the line already had.
        let wrapped = wrap_with_hanging_indent("Destination   /a/b", 12);
        assert_eq!(
            wrapped,
            vec!["Destination ".to_string(), "  /a/b".to_string()]
        );
    }

    #[test]
    fn test_confirm_button_rows_wrap_instead_of_clipping_and_mark_the_default() {
        let choices: Vec<crate::app::ConfirmChoice> =
            [('s', "Save"), ('d', "Discard"), ('c', "Cancel")]
                .into_iter()
                .map(|(key, label)| crate::app::ConfirmChoice {
                    key,
                    label: label.to_string(),
                    action: crate::app::ConfirmAction::Cancel,
                })
                .collect();

        // Wide enough for one row.
        let rows = confirm_button_rows(&choices, 60, Theme::DARK);
        assert_eq!(rows.len(), 1);

        // Too narrow for all three: they wrap rather than falling off the edge.
        let rows = confirm_button_rows(&choices, 24, Theme::DARK);
        assert!(rows.len() > 1, "chips must wrap, not clip");
        for row in &rows {
            assert!(
                row.spans
                    .iter()
                    .map(|s| str_column_width(&s.content))
                    .sum::<usize>()
                    <= 24
            );
        }
        let rendered: String = rows
            .iter()
            .flat_map(|r| r.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        for key in ["[S]", "[D]", "[C]"] {
            assert!(rendered.contains(key), "{key} must survive wrapping");
        }

        // Enter picks the first choice, so it is the only one drawn filled.
        let rows = confirm_button_rows(&choices, 60, Theme::DARK);
        let first = rows[0]
            .spans
            .iter()
            .find(|s| s.content.contains("[S]"))
            .expect("the default chip");
        assert_eq!(first.style.bg, Some(Theme::DARK.accent));
        let other = rows[0]
            .spans
            .iter()
            .find(|s| s.content.contains("[C]"))
            .expect("a non-default chip");
        assert_eq!(other.style.bg, None);
    }

    #[test]
    fn test_narrow_confirm_modal_keeps_every_exit_choice_visible() {
        let backend = TestBackend::new(30, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let choices = vec![
            crate::app::ConfirmChoice {
                key: 's',
                label: "Save".to_string(),
                action: crate::app::ConfirmAction::SaveStagedThenLeave,
            },
            crate::app::ConfirmChoice {
                key: 'd',
                label: "Discard".to_string(),
                action: crate::app::ConfirmAction::DiscardStagedThenLeave,
            },
            crate::app::ConfirmChoice {
                key: 'c',
                label: "Cancel".to_string(),
                action: crate::app::ConfirmAction::Cancel,
            },
        ];
        let lines: Vec<String> = Vec::new();
        let view = ConfirmView {
            title: "Staged changes",
            headline: "Staged changes need a decision before leaving.",
            lines: &lines,
            choices: &choices,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_confirm_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        for key in ["[S]", "[D]", "[C]"] {
            assert!(
                buffer.contains(key),
                "narrow modal must retain {key}: {buffer}"
            );
        }
    }

    #[test]
    fn narrow_exclusion_editor_modal_renders_without_overflow() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(
            std::path::PathBuf::from("left"),
            std::path::PathBuf::from("right"),
        );
        app.open_config();
        app.open_exclusion_editor();

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Global"));
    }

    /// The popup must keep the highlighted rule on screen instead of clipping
    /// everything past the old 18-row cap.
    #[test]
    fn exclusion_editor_keeps_the_selected_rule_visible_when_the_list_is_long() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(
            std::path::PathBuf::from("left"),
            std::path::PathBuf::from("right"),
        );
        app.open_config();
        app.open_exclusion_editor();
        for i in 0..20 {
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
            for ch in format!("zz{i:02}").chars() {
                app.exclusion_editor_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            }
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(
            rendered.contains("zz19"),
            "the selected last rule must stay visible: {rendered}"
        );
        assert!(
            !rendered.contains(".git/"),
            "the top of the list must scroll away once the selection is at the end: {rendered}"
        );
    }

    fn buffer_row_string(buffer: &Buffer, y: u16) -> String {
        let area = buffer.area();
        (area.left()..area.right())
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect()
    }

    fn find_row_containing(buffer: &Buffer, needle: &str) -> u16 {
        let area = buffer.area();
        for y in area.top()..area.bottom() {
            if buffer_row_string(buffer, y).contains(needle) {
                return y;
            }
        }
        panic!("buffer has no row containing {needle:?}: {buffer:?}");
    }

    fn find_cell_sequence(buffer: &Buffer, y: u16, chars: &[&str]) -> u16 {
        let area = buffer.area();
        (area.left()..area.right())
            .find(|&col| {
                chars
                    .iter()
                    .enumerate()
                    .all(|(i, ch)| buffer[(col + u16::try_from(i).unwrap(), y)].symbol() == *ch)
            })
            .unwrap_or_else(|| panic!("no sequence {chars:?} on row {y}: {buffer:?}"))
    }

    /// Content seam: while a pattern is being edited, the row must look distinct
    /// from mere selection and must reverse-video the character under the cursor.
    #[test]
    fn exclusion_editor_editing_row_shows_cursor_and_selection_background() {
        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut input = crate::text_input::TextInput::from(".git/");
        input.home();
        input.right(); // cursor on 'g'
        let view = ExclusionEditorView {
            draft: vec![".git/".to_string(), ".hg/".to_string()],
            selected_idx: 0,
            scroll_offset: 0,
            editing: true,
            input,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| {
                let layout = exclusion_editor_layout(view.draft.len(), f.area());
                draw_exclusion_editor(f, &view, &layout);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{buffer:?}");
        assert!(
            buffer_string.contains("confirm"),
            "editing should replace list-mode shortcuts with an in-edit hint: {buffer_string}"
        );
        assert!(
            !buffer_string.contains("a +"),
            "list-mode shortcuts must not show while editing: {buffer_string}"
        );

        let y = find_row_containing(buffer, ".git/");
        let x = find_cell_sequence(buffer, y, &[".", "g", "i", "t", "/"]);
        let dot = &buffer[(x, y)];
        let cursor = &buffer[(x + 1, y)];
        assert_eq!(dot.symbol(), ".");
        assert_eq!(cursor.symbol(), "g");
        assert!(
            !dot.modifier.contains(Modifier::REVERSED),
            "non-cursor chars must not reverse: {dot:?}"
        );
        assert!(
            cursor.modifier.contains(Modifier::REVERSED),
            "the char under the cursor must reverse-video: {cursor:?}"
        );
        assert_eq!(dot.bg, Theme::DARK.selection_bg);
        assert_eq!(dot.fg, Theme::DARK.selection_fg);
        assert_eq!(cursor.bg, Theme::DARK.selection_bg);
    }

    /// Content seam: a selected but idle row stays accent-coloured, with no
    /// reverse-video cursor and no selection fill — that chrome is reserved
    /// for the in-edit state so the two cannot be confused.
    #[test]
    fn exclusion_editor_selected_idle_row_is_accent_without_cursor() {
        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = ExclusionEditorView {
            draft: vec![".git/".to_string(), ".hg/".to_string()],
            selected_idx: 0,
            scroll_offset: 0,
            editing: false,
            input: crate::text_input::TextInput::default(),
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| {
                let layout = exclusion_editor_layout(view.draft.len(), f.area());
                draw_exclusion_editor(f, &view, &layout);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{buffer:?}");
        assert!(
            buffer_string.contains("a +") && buffer_string.contains("↺"),
            "idle editor should keep the compact key legend: {buffer_string}"
        );

        let y = find_row_containing(buffer, ".git/");
        let x = find_cell_sequence(buffer, y, &[".", "g", "i", "t", "/"]);
        let dot = &buffer[(x, y)];
        assert_eq!(dot.symbol(), ".");
        assert!(
            !dot.modifier.contains(Modifier::REVERSED),
            "idle selection must not reverse-video: {dot:?}"
        );
        assert_eq!(dot.fg, Theme::DARK.accent);
        assert_ne!(dot.bg, Theme::DARK.selection_bg);
    }

    /// Content seam: palette Menu from a hand-built [`PaletteView`] (no full `App`).
    #[test]
    fn test_draw_palette_content_without_full_app() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![
            CommandEntry {
                key: "q".to_string(),
                label: "Quit".to_string(),
                command: crate::commands::Command::Quit,
                disabled_reason: None,
            },
            CommandEntry {
                key: "?".to_string(),
                label: "Help".to_string(),
                command: crate::commands::Command::Help,
                disabled_reason: None,
            },
        ];
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Command Palette"),
            "palette content should show its title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Quit") && buffer_string.contains("Help"),
            "palette content should list actions: {buffer_string}"
        );
    }

    /// Content seam: Help body from a hand-built [`HelpView`] only (no full `App`).
    #[test]
    fn test_draw_help_content_without_full_app() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let method = crate::upgrade::InstallMethod::Standalone;
        let view = HelpView {
            topic: HelpTopic::DirectoryTree,
            index_open: false,
            index_sel: 0,
            scroll: 0,
            theme: Theme::DARK,
            update_available: None,
            install_method: &method,
        };
        let body_area = Rect::new(0, 1, 120, 17);

        terminal
            .draw(|f| draw_help_content(f, &view, body_area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Help · Directory Tree"),
            "help content should show topic title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("j / Down"),
            "help content should list topic bindings: {buffer_string}"
        );
    }

    #[test]
    fn test_draw_help_topic_body_first_line_keeps_leading_indent() {
        // Regression test: `"\` line-continuation in a Rust string literal strips ALL
        // leading whitespace off the following line, not just the newline. Topics whose
        // first content line is an indented key entry (not a header like DirectoryTree's
        // "Navigation") must not lose that indentation relative to the rest of the block.
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut().select_topic(crate::app::HelpTopic::FileDiff);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("  j / Down       scroll down one line"),
            "FileDiff topic's first content line should keep its 2-space indent, matching every other line in the block"
        );
    }

    #[test]
    fn test_draw_help_index_shows_all_six_topic_titles() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut().set_index_open(true);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        for title in crate::app::HelpTopic::all().iter().map(|t| t.title()) {
            assert!(
                buffer_string.contains(title),
                "Help index should list topic '{title}'"
            );
        }
    }

    #[test]
    fn test_draw_config_shows_flat_header_and_tools() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ]);
        app.set_view_mode(ViewMode::ConfigMenu);

        draw_frame(&mut terminal, &mut app);

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Config"),
            "Config screen title should be shown"
        );
        assert!(
            buffer_string.contains("External Diff Tool"),
            "Config header row should be shown inline"
        );
        assert!(
            buffer_string.contains("vim") && buffer_string.contains("code"),
            "Diff tool fields should render in the same list"
        );
        assert!(
            !buffer_string.contains("Config Categories"),
            "Old category menu should be removed"
        );
    }

    /// Content seam: Config list from a hand-built [`ConfigView`] (no full `App`).
    #[test]
    fn test_draw_config_content_without_full_app() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools = vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ];
        let diff_setting =
            crate::settings::DiffToolSetting::Pinned(crate::diff_tool::ExternalDiffTool::Vim);
        let view = ConfigView {
            rows: vec![
                crate::app::ConfigRowKind::Header("External Diff Tool"),
                crate::app::ConfigRowKind::DiffToolAuto,
                crate::app::ConfigRowKind::DiffToolDisabled,
                crate::app::ConfigRowKind::DiffTool {
                    idx: 0,
                    available: true,
                },
                crate::app::ConfigRowKind::DiffTool {
                    idx: 1,
                    available: false,
                },
                crate::app::ConfigRowKind::Header("Updates"),
                crate::app::ConfigRowKind::CheckUpdates,
            ],
            selected_idx: 1,
            detected_diff_tools: &tools,
            diff_tool_setting: &diff_setting,
            resolved_auto_tool: Some(crate::diff_tool::ExternalDiffTool::Vim),
            check_updates: true,
            mouse: true,
            theme_choice: crate::theme::ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: crate::settings::ScanMode::Fast,
            saved_scan_mode: crate::settings::ScanMode::Fast,
            respect_gitignore: true,
            global_exclusion_count: 0,
            cli_exclusion_count: 0,
            left_ignore_source: "left/.gitignore + .duodiffignore".to_string(),
            right_ignore_source: "right/.gitignore + .duodiffignore".to_string(),
            theme: Theme::DARK,
        };
        let body_area = Rect::new(0, 1, 120, 16);

        terminal
            .draw(|f| draw_config_content(f, &view, body_area))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Config"),
            "config content should show title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("External Diff Tool"),
            "config content should show header: {buffer_string}"
        );
        assert!(
            buffer_string.contains("vim") && buffer_string.contains("code"),
            "config content should list tools: {buffer_string}"
        );
    }

    /// Content seam: the provenance row must show both sides in full. A single
    /// concatenated line clips the Right path on a typical-width terminal.
    #[test]
    fn config_ignore_sources_show_both_paths_on_a_typical_width() {
        let backend = TestBackend::new(64, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)> = Vec::new();
        let left = "~/KeepSync/Notes/.gitignore + .duodiffignore";
        let right = "~/code/Notes/.gitignore + .duodiffignore";
        let diff_setting = crate::settings::DiffToolSetting::Auto;
        let view = ConfigView {
            rows: vec![crate::app::ConfigRowKind::IgnoreSources],
            selected_idx: 0,
            detected_diff_tools: &tools,
            diff_tool_setting: &diff_setting,
            resolved_auto_tool: None,
            check_updates: true,
            mouse: true,
            theme_choice: crate::theme::ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: crate::settings::ScanMode::Fast,
            saved_scan_mode: crate::settings::ScanMode::Fast,
            respect_gitignore: true,
            global_exclusion_count: 0,
            cli_exclusion_count: 2,
            left_ignore_source: left.to_string(),
            right_ignore_source: right.to_string(),
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_config_content(f, &view, Rect::new(0, 1, 64, 10)))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains(left),
            "Left source must be fully visible: {buffer_string}"
        );
        assert!(
            buffer_string.contains(right),
            "Right source must be fully visible: {buffer_string}"
        );
        assert!(
            buffer_string.contains("CLI: 2 rules"),
            "CLI count must remain visible: {buffer_string}"
        );
    }

    #[test]
    fn test_config_title_hints_for_all_row_types() {
        let width = 80;
        // Select row
        let title_auto = config_title(Some(crate::app::ConfigRowKind::DiffToolAuto), width);
        assert_eq!(
            title_auto,
            " Config — j/k move · Enter/Space select · Esc back "
        );
        let title_disabled = config_title(Some(crate::app::ConfigRowKind::DiffToolDisabled), width);
        assert_eq!(
            title_disabled,
            " Config — j/k move · Enter/Space select · Esc back "
        );
        let title_tool_avail = config_title(
            Some(crate::app::ConfigRowKind::DiffTool {
                idx: 0,
                available: true,
            }),
            width,
        );
        assert_eq!(
            title_tool_avail,
            " Config — j/k move · Enter/Space select · Esc back "
        );
        let title_exclusions =
            config_title(Some(crate::app::ConfigRowKind::GlobalExclusions), width);
        assert_eq!(
            title_exclusions,
            " Config — j/k move · Enter/Space select · Esc back "
        );

        // Toggle row
        let title_updates = config_title(Some(crate::app::ConfigRowKind::CheckUpdates), width);
        assert_eq!(
            title_updates,
            " Config — j/k move · Enter/Space toggle · Esc back "
        );
        let title_mouse = config_title(Some(crate::app::ConfigRowKind::Mouse), width);
        assert_eq!(
            title_mouse,
            " Config — j/k move · Enter/Space toggle · Esc back "
        );
        let title_theme = config_title(Some(crate::app::ConfigRowKind::Theme), width);
        assert_eq!(
            title_theme,
            " Config — j/k move · Enter/Space toggle · Esc back "
        );
        let title_scan = config_title(Some(crate::app::ConfigRowKind::ScanMode), width);
        assert_eq!(
            title_scan,
            " Config — j/k move · Enter/Space toggle · Esc back "
        );
        let title_gitignore =
            config_title(Some(crate::app::ConfigRowKind::RespectGitignore), width);
        assert_eq!(
            title_gitignore,
            " Config — j/k move · Enter/Space toggle · Esc back "
        );

        // Numeric row
        let title_context = config_title(Some(crate::app::ConfigRowKind::DiffContext), width);
        assert_eq!(title_context, " Config — j/k move · h/l adjust · Esc back ");

        // Unavailable row
        let title_unavail = config_title(
            Some(crate::app::ConfigRowKind::DiffTool {
                idx: 0,
                available: false,
            }),
            width,
        );
        assert_eq!(
            title_unavail,
            " Config — Not Found — install or choose another tool "
        );
        let title_unknown = config_title(Some(crate::app::ConfigRowKind::DiffToolUnknown), width);
        assert_eq!(
            title_unknown,
            " Config — Not Found — install or choose another tool "
        );

        // Header / IgnoreSources
        let title_header = config_title(Some(crate::app::ConfigRowKind::Header("Theme")), width);
        assert_eq!(title_header, " Config ");
        let title_sources = config_title(Some(crate::app::ConfigRowKind::IgnoreSources), width);
        assert_eq!(title_sources, " Config ");
    }

    #[test]
    fn test_config_title_narrow_terminal_truncation_drops_whole_units() {
        let row = Some(crate::app::ConfigRowKind::DiffToolAuto);
        // 51 chars needed for the full title.
        assert_eq!(
            config_title(row, 55),
            " Config — j/k move · Enter/Space select · Esc back "
        );

        // 40 chars needed for " Config — Enter/Space select · Esc back "
        assert_eq!(
            config_title(row, 45),
            " Config — Enter/Space select · Esc back "
        );

        // 29 chars needed for " Config — Enter/Space select "
        assert_eq!(config_title(row, 35), " Config — Enter/Space select ");

        // Fallback to " Config " (8 chars)
        assert_eq!(config_title(row, 20), " Config ");

        // Fallback to "Config" (6 chars)
        assert_eq!(config_title(row, 7), "Config");

        // Below 6 chars
        assert_eq!(config_title(row, 5), "");
    }

    #[test]
    fn test_draw_config_content_unavailable_and_pinned_and_unknown_diff_tools() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools = vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Nvim, false),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ];
        let diff_setting =
            crate::settings::DiffToolSetting::Pinned(crate::diff_tool::ExternalDiffTool::Code);
        let view = ConfigView {
            rows: vec![
                crate::app::ConfigRowKind::Header("External Diff Tool"),
                crate::app::ConfigRowKind::DiffToolAuto,
                crate::app::ConfigRowKind::DiffToolDisabled,
                crate::app::ConfigRowKind::DiffTool {
                    idx: 0,
                    available: true,
                },
                crate::app::ConfigRowKind::DiffTool {
                    idx: 1,
                    available: false,
                },
                crate::app::ConfigRowKind::DiffTool {
                    idx: 2,
                    available: false,
                },
                crate::app::ConfigRowKind::DiffToolUnknown,
            ],
            selected_idx: 1,
            detected_diff_tools: &tools,
            diff_tool_setting: &diff_setting,
            resolved_auto_tool: Some(crate::diff_tool::ExternalDiffTool::Vim),
            check_updates: true,
            mouse: true,
            theme_choice: crate::theme::ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: crate::settings::ScanMode::Fast,
            saved_scan_mode: crate::settings::ScanMode::Fast,
            respect_gitignore: true,
            global_exclusion_count: 0,
            cli_exclusion_count: 0,
            left_ignore_source: "left".to_string(),
            right_ignore_source: "right".to_string(),
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_config_content(f, &view, Rect::new(0, 0, 120, 20)))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        // Auto (vim)
        assert!(buffer_string.contains("Auto (vim)"));
        // Disabled
        assert!(buffer_string.contains("Disabled"));
        // vim (Available) - not active
        assert!(buffer_string.contains("[ ] vim   (Available)"));
        // nvim (Not Found) - unavailable and not active -> "[-]"
        assert!(buffer_string.contains("[-] nvim  (Not Found)"));
        // code (Not Found) - unavailable but pinned active -> "[x]"
        assert!(buffer_string.contains("[x] code  (Not Found)"));
        // unknown tool
        assert!(buffer_string.contains("[x] Unknown tool: unknown (Not Found)"));
    }

    /// Content seam: dual panes + indicator from a hand-built [`TreeView`] only
    /// (no `App`, no top bar / footer). Part of #128 fixture-cost goal.
    #[test]
    fn test_draw_tree_content_without_full_app() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("only-left.txt"),
            name: "only-left.txt".to_string(),
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        }];
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 15,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 55, 16),
            indicator: Rect::new(55, 1, 4, 16),
            right: Rect::new(59, 1, 61, 16),
            footer: Rect::new(0, 17, 120, 3),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("/left"),
            "tree content should show left pane path title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[2]") && buffer_string.contains("/right"),
            "tree content should show right pane path title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("only-left.txt"),
            "tree content should list the LeftOnly row: {buffer_string}"
        );
        assert!(
            buffer_string.contains("1/1"),
            "pane bottom border should show selected row over visible total: {buffer_string}"
        );
    }

    #[test]
    fn test_draw_tree_content_empty_state_renders_no_visible_entries() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 15,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 55, 16),
            indicator: Rect::new(55, 1, 4, 16),
            right: Rect::new(59, 1, 61, 16),
            footer: Rect::new(0, 17, 120, 3),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains(TREE_NO_VISIBLE_ENTRIES),
            "empty tree should render '{TREE_NO_VISIBLE_ENTRIES}': {buffer_string}"
        );
        assert!(
            !buffer_string.contains("0/0"),
            "empty tree should omit the scroll position label: {buffer_string}"
        );
    }

    #[test]
    fn test_draw_tree_content_empty_state_narrow_width_truncates() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 6,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 30, 1),
            left: Rect::new(0, 1, 13, 8),
            indicator: Rect::new(13, 1, 4, 8),
            right: Rect::new(17, 1, 13, 8),
            footer: Rect::new(0, 9, 30, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        // Truncated version with ellipsis should be present
        assert!(
            buffer_string.contains('…') || buffer_string.contains("No"),
            "narrow pane should truncate empty state gracefully: {buffer_string}"
        );
    }

    /// Issue #252: footer summary lists whole-tree pair counts.
    #[test]
    fn test_draw_tree_footer_shows_difference_summary() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: false,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: true,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, 120, 20));
        assert_eq!(
            layout.footer.height, 2,
            "summary adds one footer line above the keybinding row"
        );
        let view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: Some(TreeSummary {
                differ: 12,
                left_only: 3,
                right_only: 8,
                unverified: 0,
                identical: 214,
            }),
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("12 differ · 3 left-only · 8 right-only · 214 identical"),
            "footer must show the tree inventory: {buffer_string}"
        );
    }

    #[test]
    fn test_format_tree_summary_drops_trailing_units_on_narrow_width() {
        let summary = TreeSummary {
            differ: 12,
            left_only: 3,
            right_only: 8,
            unverified: 2,
            identical: 214,
        };
        assert_eq!(
            format_tree_summary(summary, 80),
            "12 differ · 3 left-only · 8 right-only · 2 unverified · 214 identical"
        );
        let without_identical = format_tree_summary(summary, 55);
        assert!(
            without_identical.contains("2 unverified") && !without_identical.contains("identical"),
            "identical should drop first: {without_identical}"
        );
        assert_eq!(format_tree_summary(summary, 12), "12 differ");
        assert_eq!(tree_scroll_label(0, 10), Some("1/10".to_string()));
        assert_eq!(tree_scroll_label(9, 10), Some("10/10".to_string()));
        assert_eq!(tree_scroll_label(0, 0), None);
    }

    #[test]
    fn test_draw_tree_footer_empty_tree_has_no_detail_line() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let footer_view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: false,
            filter_input: &crate::text_input::TextInput::default(),
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &crate::upgrade::InstallMethod::Standalone,
            theme: Theme::DARK,
            summary: None,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 55, 17),
            indicator: Rect::new(55, 1, 4, 17),
            right: Rect::new(59, 1, 61, 17),
            footer: Rect::new(0, 18, 120, 2),
        };

        terminal
            .draw(|f| draw_tree_footer(f, &footer_view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            !buffer_string.contains("B ·") && !buffer_string.contains("UTC"),
            "footer should not have file detail line when row is None: {buffer_string}"
        );
    }

    /// Issue #242: a long tree file name is middle-truncated with `…` so the
    /// prefix and tail stay visible instead of clipping at the pane edge.
    #[test]
    fn test_draw_tree_content_middle_truncates_long_file_names() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let backend = TestBackend::new(52, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("IIS_Management_Service.png"),
            name: "IIS_Management_Service.png".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }];
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 8,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 52, 1),
            left: Rect::new(0, 1, 24, 10),
            indicator: Rect::new(24, 1, 4, 10),
            right: Rect::new(28, 1, 24, 10),
            footer: Rect::new(0, 11, 52, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let y = find_row_containing(buffer, "IIS_");
        let row = buffer_row_string(buffer, y);
        assert!(
            row.contains('…'),
            "truncated names must show an ellipsis: {row}"
        );
        assert!(
            row.contains(".png"),
            "the tail (here the extension) must survive truncation: {row}"
        );
        assert!(
            !row.contains("IIS_Management_Service.png"),
            "the full name must not be hard-clipped without a marker: {row}"
        );
    }

    #[test]
    fn test_draw_tree_footer_mentions_help_key() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: false,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let area = Rect::new(0, 0, 120, 20);
        let layout = tree_layout(&inputs, area);
        let top_bar_view = TopBarView {
            view_mode: ViewMode::DirectoryTree,
            precise_mode: false,
            diff_show_full: false,
            diff_wrap: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            theme: Theme::DARK,
        };
        let tree_view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: layout.left.height.saturating_sub(2) as usize,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let footer_view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| {
                draw_top_bar_content(f, &top_bar_view, layout.top_bar);
                draw_tree_content(f, &tree_view, &layout);
                draw_tree_footer(f, &footer_view, &layout);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("(?)Help"),
            "Top bar should hint at the ? Help key"
        );
        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("[2]"),
            "Pane titles should show [1]/[2] focus shortcuts"
        );
        assert!(
            !buffer_string.contains("Left  ·") && !buffer_string.contains("Right  ·"),
            "Footer should not duplicate 1/2 pane focus hints"
        );
    }

    #[test]
    fn test_selected_row_detail_newer_left() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
            ..Default::default()
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            left_detail.contains("(newer)"),
            "Left side should contain '(newer)': {}",
            left_detail
        );
        assert!(
            !right_detail.contains("(newer)"),
            "Right side should not contain '(newer)': {}",
            right_detail
        );
        assert!(
            left_detail.contains("2.0 KB"),
            "Should show left size: {}",
            left_detail
        );
        assert!(
            right_detail.contains("1.0 KB"),
            "Should show right size: {}",
            right_detail
        );
    }

    /// Issue #245: Metadata strip is present for identical files to confirm why they match.
    #[test]
    fn test_selected_row_detail_identical_shows_metadata_for_both_sides() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 517,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 517,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            left_detail.contains("517 B"),
            "Left should contain size: {left_detail}"
        );
        assert!(
            right_detail.contains("517 B"),
            "Right should contain size: {right_detail}"
        );
        assert!(
            left_detail.contains("1970-01-01 00:00:00 UTC"),
            "Left should contain timestamp"
        );
        assert!(
            right_detail.contains("1970-01-01 00:00:00 UTC"),
            "Right should contain timestamp"
        );
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_selected_row_detail_newer_right() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerRight,
            left: Some(FileInfo {
                is_dir: false,
                size: 512,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            ..Default::default()
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            right_detail.contains("(newer)"),
            "Right side should contain '(newer)': {}",
            right_detail
        );
        assert!(
            !left_detail.contains("(newer)"),
            "Left side should not contain '(newer)': {}",
            left_detail
        );
        assert!(
            left_detail.contains("512 B"),
            "Should show left size: {}",
            left_detail
        );
        assert!(
            right_detail.contains("2.0 KB"),
            "Should show right size: {}",
            right_detail
        );
    }

    #[test]
    fn test_selected_row_detail_same_time() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentSameTime,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: mtime,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: mtime,
            }),
            ..Default::default()
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            !left_detail.contains("(newer)"),
            "Left side should not mark as newer: {}",
            left_detail
        );
        assert!(
            !right_detail.contains("(newer)"),
            "Right side should not mark as newer: {}",
            right_detail
        );
        assert!(left_detail.contains("2.0 KB"), "Should contain left size");
        assert!(right_detail.contains("1.0 KB"), "Should contain right size");
    }

    #[test]
    fn test_selected_row_detail_directory() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("subdir"),
            name: "subdir".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
            ..Default::default()
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            !left_detail.contains("KB") && !left_detail.contains("MB"),
            "Left detail should not show size: {}",
            left_detail
        );
        assert!(
            !right_detail.contains("KB") && !right_detail.contains("MB"),
            "Right detail should not show size: {}",
            right_detail
        );
        assert!(left_detail.contains("(newer)"), "Should mark left as newer");
        assert!(
            !right_detail.contains("(newer)"),
            "Should not mark right as newer"
        );
    }

    #[test]
    fn test_selected_row_detail_none_for_single_sided() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        // LeftOnly should return None
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("only_left.txt"),
            name: "only_left.txt".to_string(),
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        };
        assert!(selected_row_detail(Some(&row)).is_none());

        // RightOnly should return None
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("only_right.txt"),
            name: "only_right.txt".to_string(),
            state: DiffState::RightOnly,
            left: None,
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        assert!(selected_row_detail(Some(&row)).is_none());
    }

    #[test]
    fn test_selected_row_detail_none_for_missing_row() {
        assert!(selected_row_detail(None).is_none());
    }

    #[test]
    fn test_state_column_does_not_show_side_indicators() {
        // After the readability improvement, the State column should NOT contain
        // (L) or (R) side markers — that info moved to the footer detail line.
        // Content-only concern (the State/indicator column), no footer involved.
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 17,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 58, 18),
            indicator: Rect::new(58, 1, 4, 18),
            right: Rect::new(62, 1, 58, 18),
            footer: Rect::new(0, 19, 120, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        // The old "(L)" / "(R)" indicators should no longer appear in the State column
        assert!(
            !buffer_string.contains("(L)"),
            "State column should not contain '(L)' anymore"
        );
        assert!(
            !buffer_string.contains("(R)"),
            "State column should not contain '(R)' anymore"
        );
    }

    #[test]
    fn test_footer_detail_line_shown_for_different_file() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        // A row with a difference so the detail line appears in the footer.
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
            ..Default::default()
        };
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: true,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, 120, 20));
        let view = TreeFooterView {
            row: Some(&flat),
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("(newer)"),
            "Footer should show '(newer)' tag for the detail line: {}",
            buffer_string
        );
    }

    /// Issue #245: Metadata strip is rendered for identical files in the tree footer.
    #[test]
    fn test_footer_detail_line_shown_for_identical_file() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 517,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 517,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: true,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, 120, 20));
        let view = TreeFooterView {
            row: Some(&flat),
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("517 B"),
            "Footer must show file size for identical files: {buffer_string}"
        );
    }

    /// Issue #245: Left and right metadata must reserve a minimum gutter and truncate
    /// cleanly without colliding at narrow widths.
    #[test]
    fn test_footer_detail_line_reserves_minimum_gutter_at_narrow_width() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        // Narrow terminal of width 64
        let width = 64u16;
        let backend = TestBackend::new(width, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("data.bin"),
            name: "data.bin".to_string(),
            state: DiffState::DifferentNewerRight,
            left: Some(FileInfo {
                is_dir: false,
                size: 65638, // 64.1 KB
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_508_572_793), // 2017-10-21
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 115404, // 112.7 KB
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_781_922_891), // 2026-06-20 (newer)
            }),
            ..Default::default()
        };
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: true,
            has_status: false,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, width, 20));
        let view = TreeFooterView {
            row: Some(&flat),
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let detail_row_y = layout.footer.y;
        let row_str = buffer_row_string(buffer, detail_row_y);

        // Left and right must NOT collide into e.g. "UTC112.7"
        assert!(
            !row_str.contains("UTC112.7"),
            "Left and right metadata must not run together: {row_str}"
        );
        // There must be at least two spaces between left and right
        assert!(
            row_str.contains("64.1 KB") && row_str.contains("  "),
            "Must preserve minimum gutter and left info: {row_str}"
        );
        // Row length must not exceed width
        assert!(
            str_column_width(&row_str) <= width as usize,
            "Row width must fit terminal: {row_str}"
        );
    }

    /// Issue #246: Toast messages are truncated with ellipsis and never hard-clipped without marker.
    #[test]
    fn test_toast_message_truncation_with_ellipsis_at_narrow_width() {
        let width = 50u16;
        let backend = TestBackend::new(width, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let long_error =
            "Cannot open diff: binary file not supported: …/data.bin (press D for external diff)";
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: false,
            has_status: true,
            has_filter: false,
            has_update: false,
            has_summary: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, width, 20));
        let view = TreeFooterView {
            row: None,
            status_toast: Some((long_error, true)),
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
            summary: None,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let status_row_y = layout.footer.y;
        let row_str = buffer_row_string(buffer, status_row_y);

        assert!(
            row_str.contains('…'),
            "Truncated toast should end with ellipsis '…': {row_str}"
        );
        assert!(
            str_column_width(&row_str) <= width as usize,
            "Row width must not overflow terminal: {row_str}"
        );
    }

    #[test]
    fn test_diff_view_shows_file_paths_and_identical_notice() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // Inject an identical file pair
        app.push_flat_row(FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.set_view_mode(ViewMode::FileDiff);

        // diff rows with only Equal tags → files are identical
        app.diff_mut().set_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
        ))]);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);

        // Should show full paths for both sides in pane titles (OS-agnostic separators).
        let left_path = app.left_path().join("same.txt");
        let right_path = app.right_path().join("same.txt");
        assert!(
            buffer_string.contains(left_path.to_string_lossy().as_ref()),
            "Diff view should show left full path in title: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains(right_path.to_string_lossy().as_ref()),
            "Diff view should show right full path in title: {}",
            buffer_string
        );
        // Should show the identical notice
        assert!(
            buffer_string.contains("identical"),
            "Diff view should show identical notice: {}",
            buffer_string
        );
        // Should show [1] and [2] markers for distinguishing left and right panes
        assert!(
            buffer_string.contains("[1]"),
            "Diff view should show [1] marker in left title: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("[2]"),
            "Diff view should show [2] marker in right title: {}",
            buffer_string
        );
        // Should show relative time in title
        assert!(
            buffer_string.contains("ago"),
            "Diff view title should show relative time: {}",
            buffer_string
        );
    }

    /// Content seam: paint info bar + panes from a hand-built [`DiffView`] only
    /// (no `App`, no top bar / footer). Guards the #128 fixture-cost goal.
    #[test]
    fn test_draw_diff_content_without_full_app() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let method = crate::upgrade::InstallMethod::Standalone;
        let view = DiffView {
            rows: &rows,
            wrap: false,
            scroll: 0,
            h_scroll: 0,
            visible_height: 20,
            content_width: 50,
            left_line_count: 1,
            right_line_count: 1,
            left_root: &left_root,
            right_root: &right_root,
            row: Some(&flat),
            left_hash: Some("aabbccdd11223344"),
            right_hash: Some("aabbccdd11223344"),
            left_line_ending: Some("LF"),
            right_line_ending: Some("LF"),
            theme: Theme::DARK,
            status_toast: None,
            has_changes: false,
            update_available: None,
            install_method: &method,
            left_dirty: false,
            right_dirty: false,
            can_undo: false,
        };
        // Fixed geometry for a 120×28 content shell (notice + info + panes).
        let layout = DiffLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            notice: Rect::new(0, 1, 120, 1),
            info_left: Rect::new(0, 2, 60, 1),
            info_right: Rect::new(60, 2, 60, 1),
            left: Rect::new(0, 3, 60, 22),
            right: Rect::new(60, 3, 60, 22),
            footer: Rect::new(0, 25, 120, 3),
            show_identical: true,
        };

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("identical"),
            "content-only draw should show identical notice: {buffer_string}"
        );
        assert!(
            buffer_string.contains("same.txt") || buffer_string.contains("/left"),
            "content-only draw should show pane path titles: {buffer_string}"
        );
        assert!(
            buffer_string.contains("aabbccdd11223344"),
            "content-only draw should show SHA256 on the info bar: {buffer_string}"
        );
    }

    #[test]
    fn test_diff_view_intraline_highlight_splits_replacement_line() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "let foo = 1;".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "let bar = 1;".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.rs"),
            name: "file.rs".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("foo") && buffer_string.contains("bar"),
            "Replacement line content should render: {buffer_string}"
        );
        assert!(
            buffer_string.contains("underline")
                || buffer_string.contains("Underlined")
                || buffer_string.contains("UNDERLINED"),
            "Changed spans should use underline styling: {buffer_string}"
        );
    }

    /// Changed lines must read as more prominent than context: never `DIM`.
    /// Unchanged runs on a replacement line keep the delete/insert colour at
    /// full intensity; only the actually-changed characters add bold+underline
    /// (Issue #240).
    #[test]
    fn test_diff_view_changed_lines_are_not_dimmed() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "let foo = 1;".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "let bar = 1;".to_string(),
                }),
            )),
        ];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.rs"),
            name: "file.rs".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();

        let y = find_row_containing(buffer, "foo");
        let x = find_cell_sequence(buffer, y, &["l", "e", "t", " ", "f", "o", "o"]);
        let prefix = &buffer[(x, y)];
        let changed = &buffer[(x + 4, y)];
        assert_eq!(prefix.symbol(), "l");
        assert_eq!(changed.symbol(), "f");
        assert_eq!(prefix.fg, Theme::DARK.error);
        assert_eq!(changed.fg, Theme::DARK.error);
        assert!(
            !prefix.modifier.contains(Modifier::DIM),
            "unchanged runs on a changed line must not be dimmed: {prefix:?}"
        );
        assert!(
            !changed.modifier.contains(Modifier::DIM),
            "changed characters must not be dimmed: {changed:?}"
        );
        assert!(
            changed.modifier.contains(Modifier::BOLD)
                && changed.modifier.contains(Modifier::UNDERLINED),
            "changed characters keep bold+underline: {changed:?}"
        );

        let context_y = find_row_containing(buffer, "context");
        let context_x = find_cell_sequence(buffer, context_y, &["c", "o", "n", "t", "e", "x", "t"]);
        let context = &buffer[(context_x, context_y)];
        assert_eq!(context.fg, Theme::DARK.muted);
        assert!(
            !context.modifier.contains(Modifier::DIM),
            "context lines must not be dimmed: {context:?}"
        );
    }

    #[test]
    fn test_diff_view_no_identical_notice_when_files_differ() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // diff rows with a Delete tag → files differ
        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old line".to_string(),
            }),
            None,
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            !buffer_string.contains("identical"),
            "Diff view should NOT show identical notice when files differ: {}",
            buffer_string
        );
    }

    #[test]
    fn test_diff_view_shows_size_and_sha256_above_border() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old".to_string(),
            }),
            None,
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let mut fixture = DiffViewFixture::new(rows, flat);
        fixture.left_hash = Some("aabbccdd11223344".to_string());
        fixture.right_hash = Some("eeff001122334455".to_string());

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        // Size info should appear above the pane borders in the info bar
        assert!(
            buffer_string.contains("2.0 KB"),
            "Diff view should show left size in info bar: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("1.0 KB"),
            "Diff view should show right size in info bar: {}",
            buffer_string
        );
        // SHA-256 hashes should be displayed
        assert!(
            buffer_string.contains("SHA256: aabbccdd11223344"),
            "Diff view should show left SHA-256 hash: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("SHA256: eeff001122334455"),
            "Diff view should show right SHA-256 hash: {}",
            buffer_string
        );
    }

    #[test]
    fn test_format_system_time_is_utc() {
        use std::time::{Duration, SystemTime};
        assert_eq!(
            format_system_time(&SystemTime::UNIX_EPOCH),
            "1970-01-01 00:00:00 UTC"
        );
        // 1970-01-01 01:02:03 UTC
        assert_eq!(
            format_system_time(&(SystemTime::UNIX_EPOCH + Duration::from_secs(3723))),
            "1970-01-01 01:02:03 UTC"
        );
        // 1970-01-02 00:00:00 UTC
        assert_eq!(
            format_system_time(&(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))),
            "1970-01-02 00:00:00 UTC"
        );
    }

    #[test]
    fn test_format_relative_time() {
        use std::time::{Duration, SystemTime};

        let now = SystemTime::now();
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(30))),
            "just now"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(300))),
            "5m ago"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(7200))),
            "2h ago"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(259_200))),
            "3d ago"
        );
    }

    #[test]
    fn test_build_diff_pane_title_truncates_long_path() {
        use std::time::SystemTime;
        let long_path =
            std::path::PathBuf::from("/very/long/path/that/exceeds/the/pane/width/file.txt");
        let line = build_diff_pane_title(
            "[1] ",
            &long_path,
            Some(&SystemTime::UNIX_EPOCH),
            false,
            false,
            40,
            Theme::DARK,
        );
        let title = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            title.contains("[1]"),
            "Title should contain [1] prefix: {}",
            title
        );
        assert!(
            !title.contains("Left:") && !title.contains("Right:"),
            "Title should not contain a Left:/Right: prefix: {}",
            title
        );
        assert!(title.contains("ago"), "Title should contain relative time");
        // Long path should be truncated with "..."
        assert!(
            title.contains("..."),
            "Long path should be truncated: {}",
            title
        );
    }

    #[test]
    fn test_build_diff_pane_title_short_path() {
        use std::time::SystemTime;
        let short_path = std::path::PathBuf::from("/left/file.txt");
        let line = build_diff_pane_title(
            "[1] ",
            &short_path,
            Some(&SystemTime::UNIX_EPOCH),
            false,
            false,
            80,
            Theme::DARK,
        );
        let title = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            title.contains("/left/file.txt"),
            "Short path should not be truncated: {}",
            title
        );
        assert!(
            title.contains("[1]"),
            "Title should contain [1] prefix: {}",
            title
        );
        assert!(
            !title.contains("Left:") && !title.contains("Right:"),
            "Title should not contain a Left:/Right: prefix: {}",
            title
        );
        assert!(
            title.contains("ago"),
            "Title should contain relative time: {}",
            title
        );
    }

    #[test]
    fn test_build_diff_pane_title_right_pane_reserves_space_for_close_button() {
        use std::time::SystemTime;
        let long_path = std::path::PathBuf::from(
            "/very/long/path/that/exceeds/the/pane/width/markdownlint-cli2.jsonc",
        );
        let pane_width = 40;
        let line = build_diff_pane_title(
            "[2] ",
            &long_path,
            Some(&SystemTime::UNIX_EPOCH),
            false,
            true, // right pane with close button
            pane_width,
            Theme::DARK,
        );
        let title = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            title.contains("[2]"),
            "Right title should contain [2] prefix: {}",
            title
        );
        // Total title width starting at col 1 must not reach the close button at col width - 5 (35)
        let title_width = str_column_width(&title);
        assert!(
            title_width <= pane_width - 6,
            "Right pane title width ({}) should not collide with [x] button (max allowed: {}): {}",
            title_width,
            pane_width - 6,
            title
        );
    }

    #[test]
    fn test_build_diff_pane_title_dirty_pane_marks_with_asterisk() {
        use std::time::SystemTime;
        let path = std::path::PathBuf::from("/path/file.txt");
        let line = build_diff_pane_title(
            "[1] ",
            &path,
            Some(&SystemTime::UNIX_EPOCH),
            true, // dirty
            false,
            80,
            Theme::DARK,
        );
        let title = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(
            title.starts_with("*[1] "),
            "Dirty pane title should start with '*[1] ': {}",
            title
        );
        assert!(
            line.spans
                .iter()
                .all(|s| s.style.fg == Some(Theme::DARK.warn)),
            "Dirty pane title spans should use warn colour"
        );
    }

    #[test]
    fn test_diff_view_right_pane_title_not_clipped_by_close_button() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "line".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "line".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("Notes/.markdownlint-cli2.jsonc"),
            name: ".markdownlint-cli2.jsonc".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let left_root = PathBuf::from("/Users/user/KeepSync");
        let right_root = PathBuf::from("/Users/user/code");
        let method = crate::upgrade::InstallMethod::Standalone;
        let view = DiffView {
            rows: &rows,
            wrap: false,
            scroll: 0,
            h_scroll: 0,
            visible_height: 15,
            content_width: 35,
            left_line_count: 1,
            right_line_count: 1,
            left_root: &left_root,
            right_root: &right_root,
            row: Some(&flat),
            left_hash: None,
            right_hash: None,
            left_line_ending: None,
            right_line_ending: None,
            theme: Theme::DARK,
            status_toast: None,
            has_changes: false,
            update_available: None,
            install_method: &method,
            left_dirty: false,
            right_dirty: false,
            can_undo: false,
        };
        let layout = DiffLayout {
            top_bar: Rect::new(0, 0, 80, 1),
            notice: Rect::new(0, 1, 80, 1),
            info_left: Rect::new(0, 2, 40, 1),
            info_right: Rect::new(40, 2, 40, 1),
            left: Rect::new(0, 3, 40, 15),
            right: Rect::new(40, 3, 40, 15),
            footer: Rect::new(0, 18, 80, 2),
            show_identical: false,
        };

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);

        // Right pane should render [2] and [x] intact without collision
        assert!(
            buffer_string.contains("[1]"),
            "Left pane should show [1]: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[2]"),
            "Right pane should show [2]: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[x]"),
            "Close button [x] should render intact: {buffer_string}"
        );
    }

    #[test]
    fn test_wrap_text_splits_long_lines() {
        let text = "abcdefghijklmnopqrstuvwxyz";
        let wrapped = wrap_text(text, 10);
        assert_eq!(wrapped, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }

    #[test]
    fn test_wrap_text_preserves_short_lines() {
        let text = "hello";
        let wrapped = wrap_text(text, 10);
        assert_eq!(wrapped, vec!["hello"]);
    }

    #[test]
    fn test_wrap_text_empty_input() {
        let wrapped = wrap_text("", 10);
        assert_eq!(wrapped, vec![""]);
    }

    #[test]
    fn test_scrolled_text_basic() {
        assert_eq!(scrolled_text("hello world", 0, 5), "hello");
        assert_eq!(scrolled_text("hello world", 6, 5), "world");
        assert_eq!(scrolled_text("hello world", 20, 5), "");
        assert_eq!(
            scrolled_text("中abc", 2, 10),
            "abc",
            "horizontal scroll skips by display width, not graphemes"
        );
    }

    #[test]
    fn test_diff_view_wrap_mode_increases_physical_rows() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        // Assertion is on `app.viewport()`, computed entirely by `App::sync_viewport`
        // (via `resync_diff_geometry`) — no rendering needed, so no `Terminal`/`draw`.
        let area = Rect::new(0, 0, 40, 30);
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.push_flat_row(FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.set_view_mode(ViewMode::FileDiff);

        // One logical row with a long line (52 chars). At 40-column terminal,
        // content width is ~18, so wrapping should produce multiple physical rows.
        app.diff_mut().set_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
        ))]);

        app.diff_mut().set_wrap(false);
        app.sync_viewport(area);
        let no_wrap_rows = app.viewport().diff_physical_rows;

        app.diff_mut().set_wrap(true);
        app.sync_viewport(area);
        let wrap_rows = app.viewport().diff_physical_rows;

        assert_eq!(
            no_wrap_rows, 1,
            "Without wrapping one logical row is one physical row"
        );
        assert!(
            wrap_rows > no_wrap_rows,
            "Wrapping should produce more physical rows: {} > {}",
            wrap_rows,
            no_wrap_rows
        );
    }

    #[test]
    fn test_diff_view_horizontal_scroll_offset() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // Longer than the 38-column pane, so an offset of 5 is a legal scroll
        // position rather than one `sync_viewport` would clamp away.
        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let fixture = DiffViewFixture::new(rows, flat);

        // No changes (all Equal rows) → identical notice shown, same as `App` would compute.
        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        let view = fixture.view(false, 0, 5, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("56789abcdefghijklmno"),
            "Horizontally scrolled content should start after the offset: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("01234"),
            "Content before the horizontal scroll offset should not be visible: {}",
            buffer_string
        );
    }

    fn diff_flat_row(name: &str) -> FlatRow {
        use crate::diff::FileInfo;
        use std::time::SystemTime;
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from(name),
            name: name.to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_diff_view_gutter_shows_absolute_line_numbers_and_markers() {
        let rows = crate::diff_view::compare_texts("keep\nold\n", "keep\nnew\n", true, 3);
        let fixture = DiffViewFixture::new(rows, diff_flat_row("file.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let old_row = buffer_row_string(buffer, find_row_containing(buffer, "old"));
        let new_row = buffer_row_string(buffer, find_row_containing(buffer, "new"));
        let keep_row = buffer_row_string(buffer, find_row_containing(buffer, "keep"));
        assert!(
            old_row.contains("2") && old_row.contains('-') && old_row.contains('│'),
            "delete gutter: {old_row}"
        );
        assert!(
            new_row.contains("2") && new_row.contains('+') && new_row.contains('│'),
            "insert gutter: {new_row}"
        );
        assert!(
            keep_row.contains("1") && !keep_row.contains('-') && !keep_row.contains('+'),
            "context has a blank marker: {keep_row}"
        );
    }

    #[test]
    fn test_diff_view_insert_delete_empty_side_has_no_line_number() {
        let rows = crate::diff_view::compare_texts("keep\ngone\n", "keep\n", true, 3);
        let fixture = DiffViewFixture::new(rows, diff_flat_row("file.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let y = find_row_containing(buffer, "gone");
        let right_inner = layout.right.x + 1;
        assert_ne!(
            buffer[(layout.left.x + 1, y)].symbol(),
            " ",
            "the deleted side still has a line number"
        );
        assert_eq!(
            buffer[(right_inner, y)].symbol(),
            " ",
            "the empty insert side has no line number"
        );
    }

    #[test]
    fn test_diff_view_collapsed_gap_row_and_absolute_numbers() {
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
        let rows = crate::diff_view::compare_texts(
            &(left.join("\n") + "\n"),
            &(right.join("\n") + "\n"),
            false,
            1,
        );
        let fixture = DiffViewFixture::new(rows, diff_flat_row("file.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let y = find_row_containing(buffer, "…");
        let gap = buffer_row_string(buffer, y);
        let mid = buffer.area().width / 2;
        let left_half: String = gap.chars().take(mid as usize).collect();
        let right_half: String = gap.chars().skip(mid as usize).collect();
        assert!(
            left_half.contains('…') && right_half.contains('…'),
            "omitted range shows an ellipsis on both panes: {gap}"
        );
        let second = buffer_row_string(buffer, find_row_containing(buffer, "left-b"));
        assert!(
            second.contains("25"),
            "collapsed view keeps absolute source line 25: {second}"
        );
    }

    #[test]
    fn test_diff_view_wrapped_continuation_blanks_gutter() {
        let long_old = "D".repeat(80);
        let long_new = "I".repeat(80);
        let rows = crate::diff_view::compare_texts(
            &format!("{long_old}\n"),
            &format!("{long_new}\n"),
            true,
            3,
        );
        let fixture = DiffViewFixture::new(rows, diff_flat_row("wide.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(true, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let first_y = find_row_containing(buffer, "DDDD");
        let first = buffer_row_string(buffer, first_y);
        let second = buffer_row_string(buffer, first_y + 1);
        assert!(
            first.contains('-') && first.contains('1'),
            "first wrapped change row shows number and marker: {first}"
        );
        assert!(
            second.contains('│') && !second.contains('1') && !second.contains('-'),
            "continuation blanks the line number and marker: {second}"
        );
    }

    #[test]
    fn test_diff_view_horizontal_scroll_keeps_gutter_fixed() {
        let rows = crate::diff_view::compare_texts(
            "0123456789abcdefghij\n",
            "0123456789abcdefghij\n",
            true,
            3,
        );
        let fixture = DiffViewFixture::new(rows, diff_flat_row("wide.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 5, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let y = find_row_containing(buffer, "56789");
        let row = buffer_row_string(buffer, y);
        assert!(
            row.contains('│'),
            "gutter separator stays while text scrolls: {row}"
        );
        assert!(
            row.contains('1'),
            "line number stays while text scrolls: {row}"
        );
        assert!(
            !row.contains("01234"),
            "scrolled-away text is hidden: {row}"
        );
    }

    #[test]
    fn test_diff_view_narrow_pane_keeps_marker_without_line_numbers() {
        let rows = crate::diff_view::compare_texts("keep\nold\n", "keep\nnew\n", true, 3);
        let fixture = DiffViewFixture::new(rows, diff_flat_row("file.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(28, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 28, 20));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let y = find_row_containing(buffer, "old");
        let inner_x = layout.left.x + 1;
        assert_eq!(
            buffer[(inner_x, y)].symbol(),
            "-",
            "narrow pane still shows the delete marker in the first content column"
        );
        let old_row = buffer_row_string(buffer, y);
        assert!(
            old_row.contains("- old"),
            "narrow pane hides the number/separator and keeps the marker: {old_row}"
        );
    }

    #[test]
    fn test_diff_view_large_line_numbers_use_fixed_width() {
        let mut lines = Vec::new();
        for i in 1..=100 {
            if i == 100 {
                lines.push("changed-left");
            } else {
                lines.push("same");
            }
        }
        let mut right = lines.clone();
        right[99] = "changed-right";
        let rows = crate::diff_view::compare_texts(
            &(lines.join("\n") + "\n"),
            &(right.join("\n") + "\n"),
            true,
            3,
        );
        let fixture = DiffViewFixture::new(rows, diff_flat_row("big.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 90, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = buffer_row_string(buffer, find_row_containing(buffer, "changed-left"));
        assert!(
            row.contains("100"),
            "three-digit files keep a fixed number column: {row}"
        );
    }

    #[test]
    fn test_diff_view_markers_are_present_without_relying_on_colour() {
        let rows = crate::diff_view::compare_texts("keep\ngone\n", "keep\n", true, 3);
        let fixture = DiffViewFixture::new(rows, diff_flat_row("file.txt"));
        let inputs = DiffLayoutInputs {
            has_changes: true,
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 24));
        let (visible_height, content_width) =
            diff_content_geometry(&layout, fixture.rows.as_slice());
        let view = fixture.view(false, 0, 0, visible_height, content_width);
        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let gone = buffer_row_string(buffer, find_row_containing(buffer, "gone"));
        assert!(
            gone.contains('-'),
            "delete semantics remain in the text even if colour is ignored: {gone}"
        );
        let y = find_row_containing(buffer, "gone");
        let x = find_cell_sequence(buffer, y, &["-"]);
        // The marker is a distinct cell from the source text.
        assert_eq!(buffer[(x, y)].symbol(), "-");
    }

    #[test]
    fn test_diff_line_highlight_priority() {
        assert_eq!(
            diff_line_highlight(true, true, true),
            DiffLineHighlight::Cursor
        );
        assert_eq!(
            diff_line_highlight(true, true, false),
            DiffLineHighlight::ActiveHunk
        );
        assert_eq!(
            diff_line_highlight(true, false, false),
            DiffLineHighlight::ChangeHunk
        );
        assert_eq!(
            diff_line_highlight(false, false, false),
            DiffLineHighlight::None
        );
    }

    #[test]
    fn test_diff_view_highlights_mergeable_blocks_and_cursor() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old-line".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new-line".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
            )),
        ];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        // Cursor on context line (scroll: 0); the nearest change hunk row should
        // still be emphasized.
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Rgb(48, 48, 88)"),
            "Active mergeable hunk should use emphasized background: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("Rgb(64, 64, 64)"),
            "Cursor line should use distinct background: {}",
            buffer_string
        );
    }

    #[test]
    fn test_light_theme_changes_diff_hunk_background() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old-line".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new-line".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
            )),
        ];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        };
        let mut fixture = DiffViewFixture::new(rows, flat);
        fixture.theme = Theme::LIGHT;

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout, &fixture.rows);
        // Cursor on context line (scroll: 0), same as the dark-theme equivalent test,
        // so the nearest change hunk (not the cursor row) is the one under assertion.
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Rgb(205, 205, 240)"),
            "Light theme should use its own active-hunk background, not the dark default: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("Rgb(48, 48, 88)"),
            "Light theme must not fall back to the dark-theme active-hunk background: {}",
            buffer_string
        );
    }

    #[test]
    fn test_light_theme_changes_top_bar_title_colour() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Light);

        draw_frame(&mut terminal, &mut app);

        let light_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            light_buffer_string.contains("Black"),
            "Light theme top-bar title should use a dark (Black) foreground: {}",
            light_buffer_string
        );

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // Set explicitly rather than relying on the loaded default: other tests in this
        // binary persist `settings.theme` to the real config file, and `App::new` reloads
        // from disk, so a bare default here would be flaky under parallel test execution.
        app.set_theme(crate::theme::ThemeChoice::Dark);
        draw_frame(&mut terminal, &mut app);
        let dark_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            !dark_buffer_string.contains("Black"),
            "Dark theme top-bar title should not use Black: {}",
            dark_buffer_string
        );
    }

    #[test]
    fn test_light_theme_paints_full_canvas_background() {
        // Regression guard: `draw()` must paint the whole frame with the theme's canvas
        // background before drawing any view, otherwise cells left unpainted by inner
        // widgets (e.g. gaps between panes) keep the terminal's native colour instead of
        // showing the theme's chosen background (Issue: Light theme background stayed
        // terminal-native because nothing called `Theme::base_style()`).
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Light);
        draw_frame(&mut terminal, &mut app);
        let light_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            light_buffer_string.contains("bg: White"),
            "Light theme should paint the canvas background White: {}",
            light_buffer_string
        );

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Dark);
        draw_frame(&mut terminal, &mut app);
        let dark_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            !dark_buffer_string.contains("bg: White"),
            "Dark theme should not paint the canvas background White: {}",
            dark_buffer_string
        );
    }

    #[test]
    fn test_diff_view_header_shows_wrap_state() {
        // "Wrap" is painted by the shared top bar (`TopBarView`/`draw_top_bar_content`),
        // not the diff content/footer — no `App` or `DiffView` needed for this one.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = TopBarView {
            view_mode: ViewMode::FileDiff,
            precise_mode: false,
            diff_show_full: false,
            diff_wrap: true,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            theme: Theme::DARK,
        };
        let area = Rect::new(0, 0, 80, 1);

        terminal
            .draw(|f| draw_top_bar_content(f, &view, area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Wrap"),
            "Header should show Wrap state: {}",
            buffer_string
        );
    }

    #[test]
    fn test_draw_close_button() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                draw_close_button(f, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("[x]"),
            "Buffer should contain close button [x]"
        );
    }

    /// Issue #232: `≈` needs its own indicator glyph, distinct from both `=` and `≠`.
    #[test]
    fn test_tree_indicator_shows_approx_symbol_for_unverified_rows() {
        use crate::diff::{FileInfo, UnverifiedReason};
        use std::time::{Duration, SystemTime};

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state: DiffState::Unverified(UnverifiedReason::NotCompared),
            left: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(600),
            }),
            ..Default::default()
        }];
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 17,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
            is_filter_active: false,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 58, 18),
            indicator: Rect::new(58, 1, 4, 18),
            right: Rect::new(62, 1, 58, 18),
            footer: Rect::new(0, 19, 120, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("≈"),
            "unverified rows must render the `≈` indicator: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("≠"),
            "an unverified row must not claim a difference: {}",
            buffer_string
        );
    }

    /// Issue #232: the reason a pair is `≈` belongs in the selected-row details.
    #[test]
    fn test_selected_row_detail_exposes_the_unverified_reason() {
        use crate::diff::{FileInfo, UnverifiedReason};
        use std::time::{Duration, SystemTime};

        let row = |state| FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state,
            left: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(600),
            }),
            ..Default::default()
        };

        let fast = row(DiffState::Unverified(UnverifiedReason::NotCompared));
        let (left, right) = selected_row_detail(Some(&fast)).unwrap();
        assert!(right.contains("content unverified (fast scan)"), "{right}");
        // The newer side is still derived from the timestamps the row holds.
        assert!(!left.contains("(newer)"), "{left}");
        assert!(right.contains("(newer)"), "{right}");

        let failed = row(DiffState::Unverified(UnverifiedReason::HashFailed));
        let (_, right) = selected_row_detail(Some(&failed)).unwrap();
        assert!(
            right.contains("content unverified (read failed)"),
            "{right}"
        );

        // Established differences keep their existing detail line, reason-free.
        let different = row(DiffState::DifferentNewerRight);
        let (_, right) = selected_row_detail(Some(&different)).unwrap();
        assert!(!right.contains("content unverified"), "{right}");
    }

    /// Issue #238: while `--scan-mode` overrides the saved default, the Config row
    /// says so; once they agree again the annotation is gone.
    #[test]
    fn test_draw_config_content_annotates_a_scan_mode_session_override() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)> = Vec::new();
        let body_area = Rect::new(0, 1, 120, 16);

        let render = |terminal: &mut Terminal<TestBackend>,
                      scan_mode: crate::settings::ScanMode,
                      saved_scan_mode: crate::settings::ScanMode| {
            let view = ConfigView {
                rows: vec![
                    crate::app::ConfigRowKind::Header("Scan"),
                    crate::app::ConfigRowKind::ScanMode,
                ],
                selected_idx: 1,
                detected_diff_tools: &tools,
                diff_tool_setting: &crate::settings::DiffToolSetting::Auto,
                resolved_auto_tool: None,
                check_updates: true,
                mouse: true,
                theme_choice: crate::theme::ThemeChoice::Dark,
                diff_context: 3,
                scan_mode,
                saved_scan_mode,
                respect_gitignore: true,
                global_exclusion_count: 0,
                cli_exclusion_count: 0,
                left_ignore_source: "left/.gitignore + .duodiffignore".to_string(),
                right_ignore_source: "right/.gitignore + .duodiffignore".to_string(),
                theme: Theme::DARK,
            };
            terminal
                .draw(|f| draw_config_content(f, &view, body_area))
                .unwrap();
            format!("{:?}", terminal.backend().buffer())
        };

        let overridden = render(
            &mut terminal,
            crate::settings::ScanMode::Precise,
            crate::settings::ScanMode::Fast,
        );
        assert!(overridden.contains("Scan mode: Precise"), "{overridden}");
        assert!(
            overridden.contains("session override; saved default: Fast"),
            "{overridden}"
        );

        let in_sync = render(
            &mut terminal,
            crate::settings::ScanMode::Fast,
            crate::settings::ScanMode::Fast,
        );
        assert!(in_sync.contains("Scan mode: Fast"), "{in_sync}");
        assert!(
            !in_sync.contains("session override"),
            "the annotation disappears once the values agree: {in_sync}"
        );
    }

    /// Issue #239: an empty result set shows a non-selectable notice, not a blank list.
    #[test]
    fn test_draw_palette_content_shows_no_match_notice() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<CommandEntry> = Vec::new();
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "zzz",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(buffer.contains(PALETTE_NO_MATCH), "{buffer}");
    }

    /// Issue #239: unavailable rows stay listed with the reason they cannot run.
    #[test]
    fn test_draw_palette_content_shows_the_disabled_reason() {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![CommandEntry::gated(
            "Compare via External Diff Tool",
            crate::commands::Command::ExternalDiff,
            false,
            "no external diff tool is configured",
        )];
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer.contains("Compare via External Diff Tool"),
            "{buffer}"
        );
        assert!(
            buffer.contains("no external diff tool is configured"),
            "{buffer}"
        );
    }

    /// Issue #239: the popup clamps to the terminal, and its geometry is the one
    /// source of truth for both painting and hit-testing.
    #[test]
    fn test_palette_layout_clamps_to_the_terminal() {
        // Roomy terminal: the popup stops at its maximum width and grows to fit.
        let roomy = palette_layout(6, Rect::new(0, 0, 120, 40));
        assert_eq!(roomy.popup.width, PALETTE_MAX_WIDTH);
        assert_eq!(roomy.visible_rows(), 6);
        assert!(roomy.popup.x + roomy.popup.width <= 120);
        assert!(roomy.popup.y + roomy.popup.height <= 40);

        // Narrow, short terminal: never wider or taller than the screen.
        let tiny = palette_layout(40, Rect::new(0, 0, 24, 10));
        assert_eq!(tiny.popup.width, 24);
        assert!(tiny.popup.height <= 10);
        assert!(tiny.popup.x + tiny.popup.width <= 24);
        assert!(tiny.popup.y + tiny.popup.height <= 10);
        assert_eq!(tiny.visible_rows(), 6, "10 rows minus 4 rows of chrome");

        // A long inventory never grows past the screen either.
        let long = palette_layout(200, Rect::new(0, 0, 120, 40));
        assert!(long.popup.height <= 40);

        // The list always keeps room for the no-match notice.
        let empty = palette_layout(0, Rect::new(0, 0, 120, 40));
        assert_eq!(empty.visible_rows(), 1);
    }

    /// Issue #239: long labels truncate by display width, so CJK cannot overflow.
    #[test]
    fn test_truncate_to_width_measures_display_columns() {
        assert_eq!(truncate_to_width("short", 10), "short");
        assert_eq!(truncate_to_width("", 10), "");
        assert_eq!(truncate_to_width("abcdef", 0), "");
        assert_eq!(truncate_to_width("abcdef", 4), "abc…");

        // Each wide character is two columns wide, so only two fit in five
        // columns once the ellipsis takes one.
        let wide = "ＷｉｄｅＴｅｘｔ";
        let truncated = truncate_to_width(wide, 5);
        assert_eq!(truncated, "Ｗｉ…");
        let width: usize = truncated.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert!(width <= 5, "{truncated} is {width} columns");
    }

    /// Issue #242: names that do not fit keep a prefix, an ellipsis, and the tail
    /// instead of clipping on the right with no marker.
    #[test]
    fn test_truncate_filename_middle_keeps_prefix_and_tail() {
        assert_eq!(truncate_filename_middle("short.txt", 20), "short.txt");
        assert_eq!(truncate_filename_middle("", 10), "");
        assert_eq!(truncate_filename_middle("abcdef", 0), "");
        assert_eq!(truncate_filename_middle("abcdef", 1), "…");
        assert_eq!(
            truncate_filename_middle("IIS_Management_Service.png", 22),
            "IIS_Manage…Service.png"
        );

        // Fullwidth letters are two columns each, matching CJK/emoji occupancy.
        let wide = truncate_filename_middle("ＷｉｄｅＮａｍｅ.png", 11);
        assert_eq!(wide, "Ｗｉ….png");
        let width: usize = wide.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert!(width <= 11, "{wide} is {width} columns");
    }

    /// Issue #239: the ninth and later items are reachable and rendered.
    #[test]
    fn test_draw_palette_content_renders_items_past_the_first_screenful() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<CommandEntry> = (0..20)
            .map(|i| CommandEntry::new(&format!("Action {i}"), crate::commands::Command::Help))
            .collect();
        let layout = palette_layout(items.len(), Rect::new(0, 0, 100, 12));
        assert!(
            layout.visible_rows() < items.len(),
            "the test needs a viewport smaller than the inventory"
        );

        let view = PaletteView {
            items: &items,
            selected_idx: 12,
            scroll_offset: 12 + 1 - layout.visible_rows(),
            query: "",
            theme: Theme::DARK,
        };
        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(buffer.contains("Action 12"), "{buffer}");
        assert!(!buffer.contains("Action 0 "), "{buffer}");
    }

    /// Issue #244: When an overlay is drawn over underlying text, wide characters
    /// straddling the overlay's left boundary must be padded with space instead of
    /// emitting partial glyphs that eat the overlay border.
    #[test]
    fn test_clear_overlay_pads_wide_character_straddling_left_boundary() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 5));
        // Put a double-width character at column 9 (covering columns 9 and 10).
        buf.set_string(9, 2, "Ｗ", Style::default());
        assert_eq!(buf[(9, 2)].symbol(), "Ｗ");

        // Clear overlay covering x=10..30
        clear_overlay(&mut buf, Rect::new(10, 0, 20, 5));

        // Column 9 straddles the boundary (covers col 9 and col 10).
        // Since col 10 is inside the overlay, col 9 must be padded to a space.
        assert_eq!(buf[(9, 2)].symbol(), " ");
        // Inside the overlay, col 10 is cleared.
        assert_eq!(buf[(10, 2)].symbol(), " ");
    }

    /// Issue #244: When a wide character straddles the right boundary of the overlay
    /// (started at `area.right() - 1` and continued into `area.right()`), clearing the overlay
    /// must clean up the orphaned continuation cell at `area.right()`.
    #[test]
    fn test_clear_overlay_fixes_orphaned_continuation_at_right_boundary() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 5));
        // Put a double-width character at column 29 (covering columns 29 and 30).
        buf.set_string(29, 2, "Ｘ", Style::default());
        assert_eq!(buf[(29, 2)].symbol(), "Ｘ");

        // If column 30 was a continuation cell with empty symbol
        buf[(30, 2)].set_symbol("");

        // Clear overlay covering x=10..30 (right boundary is 30)
        clear_overlay(&mut buf, Rect::new(10, 0, 20, 5));

        // Inside the overlay, column 29 is cleared.
        assert_eq!(buf[(29, 2)].symbol(), " ");
        // Outside the overlay, column 30's orphaned continuation cell is reset to a space.
        assert_eq!(buf[(30, 2)].symbol(), " ");
    }

    /// Issue #244: Wide characters completely outside the overlay boundary are preserved.
    #[test]
    fn test_clear_overlay_preserves_wide_characters_outside_overlay() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 5));
        // Col 7-8: completely to the left of overlay at x=10..30
        buf.set_string(7, 2, "Ｗ", Style::default());
        // Col 30-31: completely to the right of overlay at x=10..30
        buf.set_string(30, 2, "Ｘ", Style::default());

        clear_overlay(&mut buf, Rect::new(10, 0, 20, 5));

        assert_eq!(buf[(7, 2)].symbol(), "Ｗ");
        assert_eq!(buf[(30, 2)].symbol(), "Ｘ");
    }

    /// Issue #244: Palette drawn over tree row with wide text preserves the left border.
    #[test]
    fn test_draw_palette_over_wide_tree_row_preserves_left_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let items = [CommandEntry::new(
            "External Diff",
            crate::commands::Command::Help,
        )];
        let layout = palette_layout(items.len(), Rect::new(0, 0, 80, 24));
        let popup_x = layout.popup.x;
        let test_y = layout.popup.y + 2;

        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                // Place "Ｗ" at popup_x - 1 on row test_y before drawing palette
                buf.set_string(popup_x - 1, test_y, "Ｗ", Style::default());
                draw_palette_content(f, &view, f.area());
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        // The cell immediately before the popup must be a space, not the wide char "Ｗ"
        assert_eq!(buf[(popup_x - 1, test_y)].symbol(), " ");
        // The popup's left border must be the intact vertical bar
        assert_eq!(buf[(popup_x, test_y)].symbol(), "│");
    }

    /// Issue #244: Confirm modal drawn over wide text preserves borders.
    #[test]
    fn test_draw_confirm_modal_over_wide_text_preserves_left_border() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let choices = [crate::app::ConfirmChoice {
            key: 'y',
            label: "Yes".to_string(),
            action: crate::app::ConfirmAction::CopyLeftToRight,
        }];
        let lines: [String; 0] = [];
        let view = ConfirmView {
            title: "Confirm",
            headline: "Confirm overwrite?",
            lines: &lines,
            choices: &choices,
            theme: Theme::DARK,
        };

        // Modal width calculation: clamp((80 * 3 / 4), 52, 96) = 60.
        // centered_rect x = (80 - 60) / 2 = 10.
        let modal_x = 10u16;
        let test_y = 12u16;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                buf.set_string(modal_x - 1, test_y, "Ｗ", Style::default());
                draw_confirm_content(f, &view, f.area());
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        assert_eq!(buf[(modal_x - 1, test_y)].symbol(), " ");
        assert_eq!(buf[(modal_x, test_y)].symbol(), "│");
    }

    /// Issue #244: App drawing directory tree with wide rows and open command palette.
    #[test]
    fn test_draw_tree_with_open_palette_pads_straddling_wide_chars() {
        let mut app = App::new(PathBuf::from("/tmp/left"), PathBuf::from("/tmp/right"));
        app.push_flat_row(FlatRow {
            name: "OAuth 1.0a Ｗｉｄｅ.odg".to_string(),
            relative_path: PathBuf::from("OAuth 1.0a Ｗｉｄｅ.odg"),
            depth: 0,
            state: DiffState::Identical,
            left: Some(crate::diff::FileInfo {
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            }),
            right: Some(crate::diff::FileInfo {
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            }),
            ..Default::default()
        });

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Open palette
        app.open_palette();

        draw_frame(&mut terminal, &mut app);

        let buf = terminal.backend().buffer();
        let layout = palette_layout(app.palette().items.len(), Rect::new(0, 0, 80, 24));
        let popup_x = layout.popup.x;

        // Verify every row within popup height has an intact left border and no 2-wide char immediately before it
        for y in layout.popup.top()..layout.popup.bottom() {
            let left_cell = &buf[(popup_x - 1, y)];
            assert!(
                UnicodeWidthStr::width(left_cell.symbol()) <= 1,
                "row {y}: cell before border must not be wide: {:?}",
                left_cell.symbol()
            );
            assert_eq!(
                buf[(popup_x, y)].symbol(),
                if y == layout.popup.top() {
                    "┌"
                } else if y == layout.popup.bottom() - 1 {
                    "└"
                } else {
                    "│"
                },
                "row {y}: border must be intact"
            );
        }
    }

    #[test]
    fn test_format_breadcrumb_basic() {
        assert_eq!(format_breadcrumb(Path::new(""), "root.txt", 20), "root.txt");
        assert_eq!(
            format_breadcrumb(Path::new("src/ui"), "mod.rs", 30),
            "src/ui › mod.rs"
        );
        // Narrow budget
        let s = format_breadcrumb(
            Path::new("a/very/deep/nested/directory/structure"),
            "target.rs",
            25,
        );
        assert!(s.ends_with(" › target.rs"));
        assert!(s.contains("structure"));
        assert!(UnicodeWidthStr::width(s.as_str()) <= 25);
    }

    #[test]
    fn test_selected_row_detail_with_case_conflict() {
        let row = FlatRow {
            name: "File.txt".to_string(),
            left_name: Some("FILE.TXT".to_string()),
            right_name: Some("file.txt".to_string()),
            relative_path: PathBuf::from("dir/File.txt"),
            has_case_conflict: true,
            state: DiffState::Identical,
            left: Some(crate::diff::FileInfo {
                size: 42,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            }),
            right: Some(crate::diff::FileInfo {
                size: 42,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            }),
            ..Default::default()
        };
        let (left_d, right_d) = selected_row_detail(Some(&row)).unwrap();
        assert!(left_d.contains("FILE.TXT"));
        assert!(right_d.contains("file.txt"));
    }

    #[test]
    fn test_selected_row_detail_with_ambiguous_case_collision() {
        let row = FlatRow {
            name: "Foo".to_string(),
            relative_path: PathBuf::from("Foo"),
            is_ambiguous_case_collision: true,
            state: DiffState::LeftOnly,
            left: Some(crate::diff::FileInfo {
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            }),
            right: None,
            ..Default::default()
        };
        let (left_d, right_d) = selected_row_detail(Some(&row)).unwrap();
        assert!(left_d.contains("Ambiguous case collision"));
        assert_eq!(right_d, "");
    }

    #[test]
    fn test_draw_tree_footer_active_filter_hints_responsive() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut filter_input = crate::text_input::TextInput::default();
        filter_input.insert('a');

        let footer_view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: true,
            filter_input: &filter_input,
            filter_pattern: "a",
            filter_diffs_only: false,
            scan_in_progress: false,
            scan_progress_count: 0,
            spinner_frame: 0,
            update_available: None,
            theme: Theme::DARK,
            summary: None,
            install_method: &crate::upgrade::InstallMethod::Standalone,
        };

        // Wide layout (100 cols): all 3 hints
        let wide_layout = TreeLayout {
            top_bar: Rect::new(0, 0, 100, 1),
            left: Rect::new(0, 1, 48, 18),
            indicator: Rect::new(48, 1, 4, 18),
            right: Rect::new(52, 1, 48, 18),
            footer: Rect::new(0, 19, 100, 1),
        };
        terminal
            .draw(|f| draw_tree_footer(f, &footer_view, &wide_layout))
            .unwrap();
        let buf = format!("{:?}", terminal.backend().buffer());
        assert!(
            buf.contains("Enter:apply")
                && buf.contains("Esc:cancel")
                && buf.contains("Ctrl+f:diffs only")
        );

        // Medium layout (40 cols): drops Ctrl+f (base_w is 11, avail is 29 >= 26)
        let med_backend = TestBackend::new(40, 20);
        let mut med_term = Terminal::new(med_backend).unwrap();
        let med_layout = TreeLayout {
            top_bar: Rect::new(0, 0, 40, 1),
            left: Rect::new(0, 1, 18, 18),
            indicator: Rect::new(18, 1, 4, 18),
            right: Rect::new(22, 1, 18, 18),
            footer: Rect::new(0, 19, 40, 1),
        };
        med_term
            .draw(|f| draw_tree_footer(f, &footer_view, &med_layout))
            .unwrap();
        let med_buf = format!("{:?}", med_term.backend().buffer());
        assert!(med_buf.contains("Enter:apply") && med_buf.contains("Esc:cancel"));
        assert!(!med_buf.contains("Ctrl+f"));

        // Narrow layout (25 cols): drops Esc:cancel too (base_w is 11, avail is 14 >= 13)
        let narrow_backend = TestBackend::new(25, 20);
        let mut narrow_term = Terminal::new(narrow_backend).unwrap();
        let narrow_layout = TreeLayout {
            top_bar: Rect::new(0, 0, 25, 1),
            left: Rect::new(0, 1, 10, 18),
            indicator: Rect::new(10, 1, 4, 18),
            right: Rect::new(14, 1, 11, 18),
            footer: Rect::new(0, 19, 25, 1),
        };
        narrow_term
            .draw(|f| draw_tree_footer(f, &footer_view, &narrow_layout))
            .unwrap();
        let narrow_buf = format!("{:?}", narrow_term.backend().buffer());
        assert!(narrow_buf.contains("Enter:apply"));
        assert!(!narrow_buf.contains("Esc:cancel"));
    }
}
