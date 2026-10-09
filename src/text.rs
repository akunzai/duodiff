//! A file's text as File Diff loads, edits, and writes it back.

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

/// Line-ending style of content already in memory, judged from its first
/// 8 KiB.
fn detect_line_ending(bytes: &[u8]) -> Option<String> {
    let chunk = &bytes[..bytes.len().min(8192)];
    let bytes_read = chunk.len();
    if bytes_read == 0 {
        return None;
    }
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

/// Maximum size of a single side accepted by the built-in text diff viewer.
/// Larger files should be opened with an external tool.
pub const MAX_DIFF_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB

/// Why content cannot be shown in the built-in diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRejection {
    /// More than [`MAX_DIFF_FILE_BYTES`]; the size when it is known.
    TooLarge(Option<u64>),
    Binary,
    NonUtf8,
}

impl std::fmt::Display for TextRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(Some(len)) => write!(
                f,
                "file too large ({len} bytes > {MAX_DIFF_FILE_BYTES} limit)"
            ),
            Self::TooLarge(None) => write!(
                f,
                "input too large (over the {MAX_DIFF_FILE_BYTES} bytes limit)"
            ),
            Self::Binary => f.write_str("binary file not supported"),
            Self::NonUtf8 => f.write_str("non-UTF-8 file not supported"),
        }
    }
}

/// Decode one side's bytes for the built-in diff, normalizing CRLF to LF.
fn decode_diff_text(buf: Vec<u8>) -> Result<String, TextRejection> {
    // NUL in the sample strongly indicates binary content.
    let sample_len = buf.len().min(8192);
    if buf[..sample_len].contains(&0) {
        return Err(TextRejection::Binary);
    }
    let text = String::from_utf8(buf).map_err(|_| TextRejection::NonUtf8)?;
    Ok(text.replace("\r\n", "\n"))
}

/// One side's content plus the facts the File Diff info bar shows about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadedText {
    pub text: String,
    pub sha256: Option<String>,
    pub line_ending: Option<String>,
}

impl LoadedText {
    /// Content already read into memory.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TextRejection> {
        if bytes.len() as u64 > MAX_DIFF_FILE_BYTES {
            return Err(TextRejection::TooLarge(Some(bytes.len() as u64)));
        }
        let sha256 = Some(crate::diff::sha256_hex(&bytes));
        let line_ending = detect_line_ending(&bytes);
        Ok(Self {
            text: decode_diff_text(bytes)?,
            sha256,
            line_ending,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_buffer_round_trips_every_shape() {
        for text in ["", "a", "a\n", "a\nb", "a\r\nb\r\n", "a\rb\r", "\n"] {
            assert_eq!(TextBuffer::from_text(text).to_text(), text, "{text:?}");
        }
        assert!(TextBuffer::from_text("").is_empty());
        assert_eq!(TextBuffer::from_text("a\nb").lines, vec!["a", "b"]);
    }

    #[test]
    fn test_detect_line_ending() {
        assert_eq!(detect_line_ending(b"hello\nworld"), Some("LF".to_string()));
        assert_eq!(
            detect_line_ending(b"hello\r\nworld"),
            Some("CRLF".to_string())
        );
    }
}
