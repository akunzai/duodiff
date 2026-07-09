use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ignore::IgnoreMatcher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    Identical,
    DifferentNewerLeft,
    DifferentNewerRight,
    DifferentSameTime,
    LeftOnly,
    RightOnly,
    TypeConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    pub modified: SystemTime,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedNode {
    pub name: String,
    pub relative_path: PathBuf,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
    pub state: DiffState,
    pub is_expanded: bool,
    pub children: Vec<AlignedNode>,
}

pub fn compute_file_md5(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut context = md5::Context::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.consume(&buffer[..count]);
    }
    let digest = context.finalize();
    Ok(format!("{:x}", digest))
}

/// Classify a differing file pair using modification times only.
fn different_by_mtime(left: SystemTime, right: SystemTime) -> DiffState {
    if left > right {
        DiffState::DifferentNewerLeft
    } else if right > left {
        DiffState::DifferentNewerRight
    } else {
        DiffState::DifferentSameTime
    }
}

pub fn align_directories(
    left_root: &Path,
    right_root: &Path,
    relative_path: &Path,
    precise_mode: bool,
    ignore: &IgnoreMatcher,
) -> Result<AlignedNode, std::io::Error> {
    let left_dir = left_root.join(relative_path);
    let right_dir = right_root.join(relative_path);

    let mut left_entries = BTreeMap::new();
    if left_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&left_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let node_rel_path = relative_path.join(&name);
                if let Ok(metadata) = entry.metadata() {
                    let is_dir = metadata.is_dir();
                    if ignore.is_ignored(&node_rel_path, is_dir) {
                        continue;
                    }
                    left_entries.insert(
                        name,
                        FileInfo {
                            size: metadata.len(),
                            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                            is_dir,
                        },
                    );
                }
            }
        }
    }

    let mut right_entries = BTreeMap::new();
    if right_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&right_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let node_rel_path = relative_path.join(&name);
                if let Ok(metadata) = entry.metadata() {
                    let is_dir = metadata.is_dir();
                    if ignore.is_ignored(&node_rel_path, is_dir) {
                        continue;
                    }
                    right_entries.insert(
                        name,
                        FileInfo {
                            size: metadata.len(),
                            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                            is_dir,
                        },
                    );
                }
            }
        }
    }

    let mut all_names: Vec<String> = left_entries.keys().cloned().collect();
    for name in right_entries.keys() {
        if !all_names.contains(name) {
            all_names.push(name.clone());
        }
    }
    all_names.sort();

    let mut children = Vec::new();
    let mut folder_state = DiffState::Identical;

    for name in all_names {
        let node_rel_path = relative_path.join(&name);
        let left_opt = left_entries.get(&name).cloned();
        let right_opt = right_entries.get(&name).cloned();

        let node = match (left_opt, right_opt) {
            (Some(left), None) => {
                let mut sub_children = Vec::new();
                if left.is_dir {
                    sub_children = make_single_sided_tree(left_root, &node_rel_path, true, ignore)?;
                }
                AlignedNode {
                    name,
                    relative_path: node_rel_path,
                    left: Some(left),
                    right: None,
                    state: DiffState::LeftOnly,
                    is_expanded: false,
                    children: sub_children,
                }
            }
            (None, Some(right)) => {
                let mut sub_children = Vec::new();
                if right.is_dir {
                    sub_children =
                        make_single_sided_tree(right_root, &node_rel_path, false, ignore)?;
                }
                AlignedNode {
                    name,
                    relative_path: node_rel_path,
                    left: None,
                    right: Some(right),
                    state: DiffState::RightOnly,
                    is_expanded: false,
                    children: sub_children,
                }
            }
            (Some(left), Some(right)) => {
                if left.is_dir != right.is_dir {
                    AlignedNode {
                        name,
                        relative_path: node_rel_path,
                        left: Some(left),
                        right: Some(right),
                        state: DiffState::TypeConflict,
                        is_expanded: false,
                        children: Vec::new(),
                    }
                } else if left.is_dir {
                    align_directories(left_root, right_root, &node_rel_path, precise_mode, ignore)?
                } else {
                    let state = if left.size != right.size {
                        different_by_mtime(left.modified, right.modified)
                    } else if precise_mode {
                        let left_full = left_root.join(&node_rel_path);
                        let right_full = right_root.join(&node_rel_path);
                        // Never treat hash failures as Identical: empty default
                        // hashes would match each other and hide real problems.
                        match (compute_file_md5(&left_full), compute_file_md5(&right_full)) {
                            (Ok(left_hash), Ok(right_hash)) if left_hash == right_hash => {
                                DiffState::Identical
                            }
                            _ => different_by_mtime(left.modified, right.modified),
                        }
                    } else if left.modified == right.modified {
                        DiffState::Identical
                    } else {
                        different_by_mtime(left.modified, right.modified)
                    };

                    AlignedNode {
                        name,
                        relative_path: node_rel_path,
                        left: Some(left),
                        right: Some(right),
                        state,
                        is_expanded: false,
                        children: Vec::new(),
                    }
                }
            }
            (None, None) => unreachable!(),
        };

        if node.state != DiffState::Identical {
            folder_state = DiffState::DifferentSameTime;
        }
        children.push(node);
    }

    let root_name = relative_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let left_meta = left_dir.metadata().ok();
    let right_meta = right_dir.metadata().ok();

    let left_info = left_meta.map(|m| FileInfo {
        size: m.len(),
        modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        is_dir: true,
    });

    let right_info = right_meta.map(|m| FileInfo {
        size: m.len(),
        modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        is_dir: true,
    });

    Ok(AlignedNode {
        name: root_name,
        relative_path: relative_path.to_path_buf(),
        left: left_info,
        right: right_info,
        state: folder_state,
        is_expanded: true,
        children,
    })
}

fn make_single_sided_tree(
    root: &Path,
    relative_path: &Path,
    is_left: bool,
    ignore: &IgnoreMatcher,
) -> Result<Vec<AlignedNode>, std::io::Error> {
    let full_dir = root.join(relative_path);
    let mut children = Vec::new();
    if !full_dir.is_dir() {
        return Ok(children);
    }
    if let Ok(entries) = fs::read_dir(&full_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let node_rel_path = relative_path.join(&name);
            if let Ok(metadata) = entry.metadata() {
                let is_dir = metadata.is_dir();
                if ignore.is_ignored(&node_rel_path, is_dir) {
                    continue;
                }
                let info = FileInfo {
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    is_dir,
                };
                let sub_children = if is_dir {
                    make_single_sided_tree(root, &node_rel_path, is_left, ignore)?
                } else {
                    Vec::new()
                };
                children.push(AlignedNode {
                    name,
                    relative_path: node_rel_path,
                    left: if is_left { Some(info.clone()) } else { None },
                    right: if is_left { None } else { Some(info.clone()) },
                    state: if is_left {
                        DiffState::LeftOnly
                    } else {
                        DiffState::RightOnly
                    },
                    is_expanded: false,
                    children: sub_children,
                });
            }
        }
    }
    children.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_alignment_logic() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        // Left only
        File::create(left.join("left_only.txt")).unwrap();
        // Right only
        File::create(right.join("right_only.txt")).unwrap();
        // Both identical
        let mtime = SystemTime::now();
        {
            let mut f1 = File::create(left.join("same.txt")).unwrap();
            f1.write_all(b"hello").unwrap();
            f1.set_modified(mtime).unwrap();
        }
        {
            let mut f2 = File::create(right.join("same.txt")).unwrap();
            f2.write_all(b"hello").unwrap();
            f2.set_modified(mtime).unwrap();
        }

        let root_node = align_directories(
            &left,
            &right,
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        assert_eq!(root_node.children.len(), 3);

        let left_only_node = root_node
            .children
            .iter()
            .find(|n| n.name == "left_only.txt")
            .unwrap();
        assert_eq!(left_only_node.state, DiffState::LeftOnly);

        let right_only_node = root_node
            .children
            .iter()
            .find(|n| n.name == "right_only.txt")
            .unwrap();
        assert_eq!(right_only_node.state, DiffState::RightOnly);

        let same_node = root_node
            .children
            .iter()
            .find(|n| n.name == "same.txt")
            .unwrap();
        assert_eq!(same_node.state, DiffState::Identical);
    }

    #[test]
    fn test_alignment_precise_mode() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        // Write same size, same contents, but different modified times
        let left_file = left.join("precise_same.txt");
        let right_file = right.join("precise_same.txt");

        {
            let mut f1 = File::create(&left_file).unwrap();
            f1.write_all(b"match_content").unwrap();
            f1.set_modified(SystemTime::now() - std::time::Duration::from_secs(10))
                .unwrap();
        }
        {
            let mut f2 = File::create(&right_file).unwrap();
            f2.write_all(b"match_content").unwrap();
            f2.set_modified(SystemTime::now()).unwrap();
        }

        // precise_mode = true should detect it as Identical
        let root_node = align_directories(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        let node = root_node
            .children
            .iter()
            .find(|n| n.name == "precise_same.txt")
            .unwrap();
        assert_eq!(node.state, DiffState::Identical);
    }

    /// Unreadable files must not collapse to Identical via empty default hashes.
    #[cfg(unix)]
    #[test]
    fn test_precise_mode_hash_failure_is_not_identical() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        let left_file = left.join("locked.txt");
        let right_file = right.join("locked.txt");
        let content = b"same-bytes-len";
        {
            let mut f1 = File::create(&left_file).unwrap();
            f1.write_all(content).unwrap();
            f1.set_modified(SystemTime::UNIX_EPOCH).unwrap();
        }
        {
            let mut f2 = File::create(&right_file).unwrap();
            f2.write_all(content).unwrap();
            f2.set_modified(SystemTime::UNIX_EPOCH).unwrap();
        }

        // Make left unreadable so compute_file_md5 fails while metadata still works.
        let mut perms = fs::metadata(&left_file).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&left_file, perms).unwrap();

        let root_node = align_directories(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        let node = root_node
            .children
            .iter()
            .find(|n| n.name == "locked.txt")
            .unwrap();
        assert_ne!(
            node.state,
            DiffState::Identical,
            "hash failure must not look Identical"
        );
        assert_eq!(node.state, DiffState::DifferentSameTime);

        // Restore permissions so tempdir cleanup succeeds.
        let mut perms = fs::metadata(&left_file).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&left_file, perms).unwrap();
    }

    #[test]
    fn test_different_by_mtime() {
        let earlier = SystemTime::UNIX_EPOCH;
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        assert_eq!(
            different_by_mtime(later, earlier),
            DiffState::DifferentNewerLeft
        );
        assert_eq!(
            different_by_mtime(earlier, later),
            DiffState::DifferentNewerRight
        );
        assert_eq!(
            different_by_mtime(earlier, earlier),
            DiffState::DifferentSameTime
        );
    }

    #[test]
    fn test_alignment_type_conflict() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        // Left side is directory, Right side is file
        fs::create_dir(left.join("conflict")).unwrap();
        File::create(right.join("conflict")).unwrap();

        let root_node = align_directories(
            &left,
            &right,
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        let node = root_node
            .children
            .iter()
            .find(|n| n.name == "conflict")
            .unwrap();
        assert_eq!(node.state, DiffState::TypeConflict);
    }

    #[test]
    fn test_alignment_ignores_excluded_paths() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        File::create(left.join("keep.txt")).unwrap();
        File::create(left.join("skip.txt")).unwrap();
        File::create(right.join("keep.txt")).unwrap();
        File::create(right.join("skip.txt")).unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("skip.txt");

        let root_node = align_directories(&left, &right, Path::new(""), false, &matcher).unwrap();
        assert_eq!(root_node.children.len(), 1);
        assert!(root_node.children.iter().any(|n| n.name == "keep.txt"));
    }

    #[test]
    fn test_alignment_ignores_excluded_directories_recursively() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        let left_target = left.join("target");
        let right_target = right.join("target");
        fs::create_dir_all(&left_target).unwrap();
        fs::create_dir_all(&right_target).unwrap();
        File::create(left_target.join("artifact")).unwrap();
        File::create(right_target.join("artifact")).unwrap();
        File::create(left.join("main.rs")).unwrap();
        File::create(right.join("main.rs")).unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("target/");

        let root_node = align_directories(&left, &right, Path::new(""), false, &matcher).unwrap();
        assert_eq!(root_node.children.len(), 1);
        assert!(root_node.children.iter().any(|n| n.name == "main.rs"));
    }

    #[test]
    fn test_alignment_single_sided_tree_respects_ignores() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        let left_only = left.join("left_only");
        fs::create_dir_all(&left_only).unwrap();
        File::create(left_only.join("ignored")).unwrap();
        File::create(left_only.join("visible")).unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("ignored");

        let root_node = align_directories(&left, &right, Path::new(""), false, &matcher).unwrap();
        let left_only_node = root_node
            .children
            .iter()
            .find(|n| n.name == "left_only")
            .unwrap();
        assert_eq!(left_only_node.children.len(), 1);
        assert!(left_only_node.children.iter().any(|n| n.name == "visible"));
    }
}
