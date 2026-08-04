use crossterm::event::KeyCode;
use std::ops::Deref;

/// A single-line text buffer with a char-indexed cursor, used by the filter bar. The
/// cursor is kept within `[0, char_len]` and always on a char boundary, so multi-byte
/// (e.g. CJK) input is edited safely.
///
/// `Deref<Target = str>` lets read sites treat it like the old `String` (`is_empty`,
/// `to_lowercase`, `&input` as `&str`); only the mutating paths go through the explicit
/// editing methods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    /// Number of chars before the cursor (0 = line start, char_len = line end).
    cursor: usize,
}

impl TextInput {
    /// Cursor position as a char count from the line start.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn char_len(&self) -> usize {
        self.value.chars().count()
    }

    /// Byte offset of char index `i` (clamped to the end for `i >= char_len`).
    fn byte_at(&self, i: usize) -> usize {
        self.value
            .char_indices()
            .nth(i)
            .map(|(b, _)| b)
            .unwrap_or(self.value.len())
    }

    /// Insert `c` at the cursor and advance past it.
    pub fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
    }

    /// Delete the char before the cursor; returns whether anything was removed.
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let start = self.byte_at(self.cursor - 1);
        let end = self.byte_at(self.cursor);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
        true
    }

    /// Delete the char at the cursor; returns whether anything was removed.
    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.char_len() {
            return false;
        }
        let start = self.byte_at(self.cursor);
        let end = self.byte_at(self.cursor + 1);
        self.value.replace_range(start..end, "");
        true
    }

    /// Move the cursor one char left; returns whether it moved.
    pub fn left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    /// Move the cursor one char right; returns whether it moved.
    pub fn right(&mut self) -> bool {
        if self.cursor >= self.char_len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    /// Move the cursor to the line start; returns whether it moved.
    pub fn home(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    /// Move the cursor to the line end; returns whether it moved.
    pub fn end(&mut self) -> bool {
        let len = self.char_len();
        if self.cursor == len {
            return false;
        }
        self.cursor = len;
        true
    }

    /// Clear the text and reset the cursor to the start.
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    /// Replace the whole value, placing the cursor at the end.
    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.char_len();
    }

    /// Apply one editing key. `Esc`/`Enter` are intentionally NOT handled here — the
    /// filter bar owns those (clear-and-exit / commit-and-exit). Unrecognized keys are
    /// no-ops; callers never branched on a return value.
    pub fn apply_edit(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => {
                self.backspace();
            }
            KeyCode::Delete => {
                self.delete();
            }
            KeyCode::Left => {
                self.left();
            }
            KeyCode::Right => {
                self.right();
            }
            KeyCode::Home => {
                self.home();
            }
            KeyCode::End => {
                self.end();
            }
            _ => {}
        }
    }
}

impl Deref for TextInput {
    type Target = str;
    fn deref(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for TextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value)
    }
}

impl From<&str> for TextInput {
    fn from(value: &str) -> Self {
        let mut input = Self::default();
        input.set(value.to_string());
        input
    }
}

impl From<String> for TextInput {
    fn from(value: String) -> Self {
        let mut input = Self::default();
        input.set(value);
        input
    }
}

impl PartialEq<str> for TextInput {
    fn eq(&self, other: &str) -> bool {
        self.value == other
    }
}

impl PartialEq<&str> for TextInput {
    fn eq(&self, other: &&str) -> bool {
        self.value == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> TextInput {
        let mut input = TextInput::default();
        for c in s.chars() {
            input.insert(c);
        }
        input
    }

    #[test]
    fn insert_at_cursor_after_moving_left() {
        let mut input = typed("ac");
        assert!(input.left()); // between a|c
        input.insert('b');
        assert_eq!(&*input, "abc");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_removes_char_before_cursor() {
        let mut input = typed("abc");
        assert!(input.left()); // ab|c
        assert!(input.backspace()); // a|c
        assert_eq!(&*input, "ac");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut input = typed("ab");
        input.home();
        assert!(!input.backspace());
        assert_eq!(&*input, "ab");
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut input = typed("abc");
        input.home(); // |abc
        assert!(input.delete()); // |bc
        assert_eq!(&*input, "bc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let input_end = typed("ab");
        let mut input = input_end.clone();
        assert!(!input.delete());
        assert_eq!(input, input_end);
    }

    #[test]
    fn left_right_clamp_at_bounds() {
        let mut input = typed("ab");
        assert!(!input.right()); // already at end
        assert!(input.left());
        assert!(input.left());
        assert!(!input.left()); // already at start
        assert!(input.right());
    }

    #[test]
    fn home_and_end_move_to_bounds() {
        let mut input = typed("abc");
        assert!(input.home());
        assert_eq!(input.cursor(), 0);
        assert!(!input.home());
        assert!(input.end());
        assert_eq!(input.cursor(), 3);
        assert!(!input.end());
    }

    #[test]
    fn cjk_multibyte_insert_and_backspace() {
        // Each CJK char is 3 bytes in UTF-8 but exactly 1 "char" for cursor purposes.
        let mut input = typed("你好");
        assert_eq!(input.cursor(), 2);
        assert!(input.left());
        input.insert('中');
        assert_eq!(&*input, "你中好");
        assert_eq!(input.cursor(), 2);
        assert!(input.backspace());
        assert_eq!(&*input, "你好");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn cjk_delete_at_cursor_removes_one_char_not_one_byte() {
        let mut input = typed("A你B");
        input.home();
        assert!(input.right()); // A|你B
        assert!(input.delete()); // A|B
        assert_eq!(&*input, "AB");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn apply_edit_dispatches_by_key_code() {
        let mut input = TextInput::default();
        input.apply_edit(KeyCode::Char('a'));
        assert_eq!(&*input, "a");
        assert_eq!(input.cursor(), 1);
        input.apply_edit(KeyCode::Left);
        assert_eq!(input.cursor(), 0);
        // Already at start / unknown key — no-ops.
        input.apply_edit(KeyCode::Left);
        assert_eq!(input.cursor(), 0);
        input.apply_edit(KeyCode::Backspace);
        assert_eq!(&*input, "a");
        input.apply_edit(KeyCode::Tab);
        assert_eq!(&*input, "a");
    }

    #[test]
    fn set_replaces_value_and_moves_cursor_to_end() {
        let mut input = typed("ab");
        input.set("hello");
        assert_eq!(&*input, "hello");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn clear_resets_value_and_cursor() {
        let mut input = typed("abc");
        input.clear();
        assert_eq!(&*input, "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn deref_and_equality_treat_it_like_a_str() {
        let input = typed("abc");
        assert!(!input.is_empty());
        assert_eq!(input, "abc");
        assert_eq!(&*input, "abc");
        assert_eq!(format!("{input}"), "abc");
    }

    #[test]
    fn from_str_and_string_place_cursor_at_end() {
        let from_str: TextInput = "café".into();
        assert_eq!(from_str.cursor(), 4);
        let from_string: TextInput = "café".to_string().into();
        assert_eq!(from_string.cursor(), 4);
    }
}
