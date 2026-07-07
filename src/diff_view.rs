use similar::{ChangeTag, TextDiff};
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub text: String,
}
pub type DiffRow = (Option<DiffLine>, Option<DiffLine>);

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
            rows.push((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: line_content.clone(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: line_content,
                }),
            ));
        }
    } else if !deletes.is_empty() && inserts.is_empty() {
        // Delete changes only
        for change in deletes {
            rows.push((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: change.value().to_string(),
                }),
                None,
            ));
        }
    } else if deletes.is_empty() && !inserts.is_empty() {
        // Insert changes only
        for change in inserts {
            rows.push((
                None,
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: change.value().to_string(),
                }),
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
            rows.push((left, right));
        }
    }
}

pub fn compare_files(
    left: &Path,
    right: &Path,
    full_context: bool,
) -> Result<Vec<DiffRow>, std::io::Error> {
    // If the path points to a directory (e.g. if one of them is missing/None and we try to read,
    // or if a directory was somehow selected), fs::read_to_string will fail, which is handled by unwrap_or_else.
    let left_text = fs::read_to_string(left)
        .unwrap_or_else(|_| String::new())
        .replace("\r\n", "\n");
    let right_text = fs::read_to_string(right)
        .unwrap_or_else(|_| String::new())
        .replace("\r\n", "\n");

    let diff = TextDiff::from_lines(&left_text, &right_text);
    let mut rows = Vec::new();

    if full_context {
        for op in diff.ops() {
            process_op(&diff, op, &mut rows);
        }
    } else {
        for group in diff.grouped_ops(3) {
            for op in group {
                process_op(&diff, &op, &mut rows);
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_compare_files_basic() {
        let mut left_file = NamedTempFile::new().unwrap();
        let mut right_file = NamedTempFile::new().unwrap();

        writeln!(left_file, "hello\nworld\nfoo").unwrap();
        writeln!(right_file, "hello\nbar\nfoo").unwrap();

        let rows = compare_files(left_file.path(), right_file.path(), false).unwrap();

        // Let's assert we have the changes
        assert!(!rows.is_empty());

        // Verify that the replacement of "world" with "bar" is aligned side-by-side
        let has_aligned_replace = rows.iter().any(|(left, right)| {
            left.as_ref()
                .is_some_and(|l| l.tag == ChangeTag::Delete && l.text.contains("world"))
                && right
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

        let rows = compare_files(left_file.path(), right_file.path(), false).unwrap();

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

        let rows = compare_files(left_file.path(), right_file.path(), true).unwrap();
        assert_eq!(rows.len(), 3);
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
}
