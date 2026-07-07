use similar::{ChangeTag, TextDiff};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub text: String,
}
pub type DiffRow = (Option<DiffLine>, Option<DiffLine>);

pub fn compare_files(left: &Path, right: &Path) -> Result<Vec<DiffRow>, std::io::Error> {
    // If the path points to a directory (e.g. if one of them is missing/None and we try to read,
    // or if a directory was somehow selected), fs::read_to_string will fail, which is handled by unwrap_or_else.
    let left_text = fs::read_to_string(left).unwrap_or_else(|_| String::new());
    let right_text = fs::read_to_string(right).unwrap_or_else(|_| String::new());

    let diff = TextDiff::from_lines(&left_text, &right_text);
    let mut rows = Vec::new();

    for group in diff.grouped_ops(3) {
        for op in group {
            for change in diff.iter_changes(&op) {
                let line_content = change.value().to_string();
                match change.tag() {
                    ChangeTag::Equal => {
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
                    ChangeTag::Delete => {
                        rows.push((
                            Some(DiffLine {
                                tag: ChangeTag::Delete,
                                text: line_content,
                            }),
                            None,
                        ));
                    }
                    ChangeTag::Insert => {
                        rows.push((
                            None,
                            Some(DiffLine {
                                tag: ChangeTag::Insert,
                                text: line_content,
                            }),
                        ));
                    }
                }
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

        let rows = compare_files(left_file.path(), right_file.path()).unwrap();

        // Let's assert we have the changes
        assert!(!rows.is_empty());

        // Find if there is a delete for "world" and insert for "bar"
        let has_delete = rows.iter().any(|(left, right)| {
            left.as_ref()
                .is_some_and(|l| l.tag == ChangeTag::Delete && l.text.contains("world"))
                && right.is_none()
        });
        let has_insert = rows.iter().any(|(left, right)| {
            left.is_none()
                && right
                    .as_ref()
                    .is_some_and(|r| r.tag == ChangeTag::Insert && r.text.contains("bar"))
        });

        assert!(has_delete, "Should contain delete of 'world'");
        assert!(has_insert, "Should contain insert of 'bar'");
    }
}
