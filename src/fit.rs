//! Fitting text to a width: every place duodiff shortens a string to a
//! column budget for display.
//!
//! One convention, one implementation: every fitting operation here measures
//! with [`crate::wrap::display_width`] / [`crate::wrap::char_display_width`],
//! the same convention line breaking uses in `crate::wrap`. `wrap` stays
//! about breaking text into rows; this module is about shortening one line
//! so it fits, always marking a cut with `…` (U+2026) rather than ASCII
//! `...`. Measuring in bytes or `chars().count()` instead of display columns
//! let a CJK path that fit on screen get truncated anyway.

use crate::wrap::{char_display_width, display_width};
use std::path::Path;

/// The longest prefix of `text` that fits in `max_width` display columns.
pub fn take_prefix_by_width(text: &str, max_width: usize) -> &str {
    let mut used = 0usize;
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        let w = char_display_width(ch);
        if used + w > max_width {
            break;
        }
        used += w;
        end = i + ch.len_utf8();
    }
    &text[..end]
}

/// The longest suffix of `text` that fits in `max_width` display columns.
pub fn take_suffix_by_width(text: &str, max_width: usize) -> &str {
    let mut used = 0usize;
    let mut start = text.len();
    for (i, ch) in text.char_indices().rev() {
        let w = char_display_width(ch);
        if used + w > max_width {
            break;
        }
        used += w;
        start = i;
    }
    &text[start..]
}

/// Truncate `text` to `max_width` display columns, appending `…` when it does
/// not fit. CJK and emoji are measured by display width, so they cannot
/// overflow the budget the way a byte or `char` count would let them.
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(text) <= max_width {
        return text.to_string();
    }
    format!(
        "{}…",
        take_prefix_by_width(text, max_width.saturating_sub(1))
    )
}

/// Truncate `name` to `max_width` display columns by inserting `…` in the
/// middle so both a prefix and a tail stay visible. Unchanged when it
/// already fits.
pub fn truncate_filename_middle(name: &str, max_width: usize) -> String {
    let total = display_width(name);
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

/// Truncate `path` from the left to `max_width` display columns, keeping the
/// filename and as many trailing directories as fit, prefixed with `…/`
/// (or bare `…` when there is no room for the separator) when it does not
/// fit whole.
///
/// Used for both root-directory titles and File Diff pane titles: a path
/// that already fits its budget is returned unchanged.
pub fn truncate_path_left(path: &Path, max_width: usize) -> String {
    let path_str = path.to_string_lossy();
    if display_width(&path_str) <= max_width {
        return path_str.into_owned();
    }
    if max_width <= 1 {
        return "…".chars().take(max_width).collect();
    }

    let sep = std::path::MAIN_SEPARATOR.to_string();
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|s| !s.is_empty() && s != &sep)
        .collect();

    let prefix = format!("…{sep}");
    let prefix_width = display_width(&prefix);

    if components.is_empty() {
        return format!("…{}", take_suffix_by_width(&path_str, max_width - 1));
    }

    let last = &components[components.len() - 1];
    if display_width(last) + prefix_width > max_width {
        if max_width <= prefix_width {
            return "…".chars().take(max_width).collect();
        }
        let tail = take_suffix_by_width(last, max_width - prefix_width);
        return format!("{prefix}{tail}");
    }

    let mut right_part = last.to_string();
    for comp in components[..components.len() - 1].iter().rev() {
        let candidate = format!("{comp}{sep}{right_part}");
        if display_width(&candidate) + prefix_width <= max_width {
            right_part = candidate;
        } else {
            break;
        }
    }
    format!("{prefix}{right_part}")
}

/// Format a breadcrumb `parent › name` abbreviated in the middle using
/// display width, preserving the basename and nearest parent first.
pub fn format_breadcrumb(parent: &Path, name: &str, max_width: usize) -> String {
    let parent_str = parent.to_string_lossy();
    if parent.as_os_str().is_empty() || parent_str.is_empty() {
        return truncate_filename_middle(name, max_width);
    }
    let sep = " › ";
    let full = format!("{parent_str}{sep}{name}");
    if display_width(&full) <= max_width {
        return full;
    }

    if max_width == 0 {
        return String::new();
    }

    let b_len = display_width(name);
    let sep_len = display_width(sep); // 3

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
    let last_len = display_width(last_comp);

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
        let comp_w = display_width(comp);
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

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    /// A CJK root path that fits its column budget must not be truncated.
    /// Each wide character is 3 UTF-8 bytes but only 2 display columns, so a
    /// path measured in bytes looked too long even when it fit on screen.
    #[test]
    fn test_truncate_path_left_fits_a_cjk_path_that_is_short_in_columns() {
        let path = Path::new("/根目录/文件名.txt");
        // 24 bytes, but 18 display columns: fits a 20-column budget.
        assert_eq!(truncate_path_left(path, 20), path.to_string_lossy());
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
        assert!(
            display_width(&truncated) <= 5,
            "{truncated} is {} columns",
            display_width(&truncated)
        );
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
        assert!(
            display_width(&wide) <= 11,
            "{wide} is {} columns",
            display_width(&wide)
        );
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
}
