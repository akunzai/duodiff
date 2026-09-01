//! Display width and line breaking for every screen.
//!
//! One convention, one implementation: a tab occupies four columns and every
//! other character occupies its Unicode display width. The File Diff
//! clamps scrolling from [`lines`] and paints from [`lines_masked`], so the two
//! cannot disagree about how many rows a wrapped line occupies (Issue #298).

use unicode_width::UnicodeWidthChar;

/// Columns a tab occupies.
const TAB_WIDTH: usize = 4;

/// Display width of `ch`, counting a tab as four columns.
pub fn char_display_width(ch: char) -> usize {
    if ch == '\t' {
        TAB_WIDTH
    } else {
        ch.width().unwrap_or(0)
    }
}

/// Display width of `text` under the same convention as [`char_display_width`].
pub fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

/// Break `text` into rows of at most `width` display columns.
///
/// Breaks on character boundaries, never mid-character. Empty input yields one
/// empty row so a blank line still occupies a row. A `width` of 0 yields the
/// text unwrapped rather than dropping it.
pub fn lines(text: &str, width: usize) -> Vec<String> {
    lines_masked(text, &[], width)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

/// [`lines`], carrying a per-character highlight mask alongside the text.
///
/// `mask` is aligned to `text` before wrapping — a shorter mask is padded with
/// `false`, a longer one is truncated — so each returned row carries exactly one
/// flag per character.
pub fn lines_masked(text: &str, mask: &[bool], width: usize) -> Vec<(String, Vec<bool>)> {
    let chars: Vec<char> = text.chars().collect();
    let mut aligned = mask.to_vec();
    aligned.truncate(chars.len());
    aligned.resize(chars.len(), false);

    if width == 0 {
        return vec![(text.to_string(), aligned)];
    }

    let mut rows: Vec<(String, Vec<bool>)> = Vec::new();
    let mut row_chars: Vec<char> = Vec::new();
    let mut row_mask: Vec<bool> = Vec::new();
    let mut row_width = 0usize;

    for (ch, highlighted) in chars.into_iter().zip(aligned) {
        let ch_width = char_display_width(ch);
        if row_width + ch_width > width && !row_chars.is_empty() {
            rows.push((
                std::mem::take(&mut row_chars).into_iter().collect(),
                std::mem::take(&mut row_mask),
            ));
            row_width = 0;
        }
        row_chars.push(ch);
        row_mask.push(highlighted);
        row_width += ch_width;
    }

    if !row_chars.is_empty() || rows.is_empty() {
        rows.push((row_chars.into_iter().collect(), row_mask));
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_counts_ascii_wide_and_tabs() {
        assert_eq!(display_width("ab"), 2);
        assert_eq!(display_width("中中"), 4);
        assert_eq!(display_width("\tX"), TAB_WIDTH + 1);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn lines_breaks_on_character_boundaries() {
        assert_eq!(lines("abcdef", 4), vec!["abcd", "ef"]);
    }

    #[test]
    fn lines_keeps_a_wide_character_whole() {
        // The second '中' would reach column 4 of a 3-column row, so it moves down.
        assert_eq!(lines("中中", 3), vec!["中", "中"]);
    }

    #[test]
    fn lines_charges_a_tab_the_full_tab_width() {
        assert_eq!(lines("\tab", 4), vec!["\t", "ab"]);
    }

    #[test]
    fn lines_yields_one_empty_row_for_empty_input() {
        assert_eq!(lines("", 10), vec![String::new()]);
    }

    #[test]
    fn lines_fills_an_exact_boundary_without_a_trailing_row() {
        assert_eq!(lines("abcd", 4), vec!["abcd"]);
    }

    #[test]
    fn lines_returns_the_text_unwrapped_at_width_zero() {
        assert_eq!(lines("abc", 0), vec!["abc"]);
    }

    #[test]
    fn lines_keeps_an_over_wide_character_rather_than_looping() {
        assert_eq!(lines("中a", 1), vec!["中", "a"]);
    }

    #[test]
    fn lines_masked_splits_the_mask_with_the_text() {
        let mask = vec![true, true, false, false, true, false];
        assert_eq!(
            lines_masked("abcdef", &mask, 4),
            vec![
                ("abcd".to_string(), vec![true, true, false, false]),
                ("ef".to_string(), vec![true, false]),
            ]
        );
    }

    #[test]
    fn lines_masked_pads_a_short_mask_and_truncates_a_long_one() {
        assert_eq!(
            lines_masked("abc", &[true], 10),
            vec![("abc".to_string(), vec![true, false, false])]
        );
        assert_eq!(
            lines_masked("ab", &[true, true, true, true], 10),
            vec![("ab".to_string(), vec![true, true])]
        );
    }

    #[test]
    fn lines_masked_aligns_the_mask_at_width_zero() {
        assert_eq!(
            lines_masked("abc", &[true], 0),
            vec![("abc".to_string(), vec![true, false, false])]
        );
    }
}
