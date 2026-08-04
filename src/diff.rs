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

/// Streaming SHA-256 hex digest of a file (precise scan mode + file-diff info bar).
/// Streams so large files are not held in RAM.
pub fn compute_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    Ok(out)
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

/// Build [`FileInfo`] for a directory entry **without following symlinks** for
/// recursion. Symlinks are always treated as leaves (`is_dir = false`) so cyclic
/// links cannot stack-overflow the scanner.
fn file_info_from_dir_entry(entry: &fs::DirEntry) -> Option<FileInfo> {
    let file_type = entry.file_type().ok()?;
    if file_type.is_symlink() {
        let meta = fs::symlink_metadata(entry.path()).ok()?;
        return Some(FileInfo {
            size: meta.len(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            // Never recurse into symlink targets during scan.
            is_dir: false,
        });
    }
    let meta = entry.metadata().ok()?;
    Some(FileInfo {
        size: meta.len(),
        modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        is_dir: meta.is_dir(),
    })
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
                if let Some(info) = file_info_from_dir_entry(&entry) {
                    if ignore.is_ignored(&node_rel_path, info.is_dir) {
                        continue;
                    }
                    left_entries.insert(name, info);
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
                if let Some(info) = file_info_from_dir_entry(&entry) {
                    if ignore.is_ignored(&node_rel_path, info.is_dir) {
                        continue;
                    }
                    right_entries.insert(name, info);
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
                        match (
                            compute_file_sha256(&left_full),
                            compute_file_sha256(&right_full),
                        ) {
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

/// Recompute a directory node's aggregate state from its children.
pub fn recompute_folder_state_from_children(node: &mut AlignedNode) {
    let is_dir = node.left.as_ref().is_some_and(|f| f.is_dir)
        || node.right.as_ref().is_some_and(|f| f.is_dir);
    if !is_dir {
        return;
    }
    node.state = if node
        .children
        .iter()
        .any(|c| c.state != DiffState::Identical)
    {
        DiffState::DifferentSameTime
    } else {
        DiffState::Identical
    };
}

/// Replace the subtree at `path` (relative) with `new_node`, then refresh
/// ancestor folder states. Returns `false` if `path` is not in the tree.
pub fn replace_subtree(root: &mut AlignedNode, path: &Path, mut new_node: AlignedNode) -> bool {
    if root.relative_path == path {
        let expanded = root.is_expanded;
        // Keep expansion preference for this directory.
        new_node.is_expanded = expanded || new_node.is_expanded;
        *root = new_node;
        return true;
    }
    for child in &mut root.children {
        if path == child.relative_path || path.starts_with(&child.relative_path) {
            if replace_subtree(child, path, new_node) {
                recompute_folder_state_from_children(root);
                return true;
            }
            return false;
        }
    }
    false
}

fn make_single_sided_tree(
    root: &Path,
    relative_path: &Path,
    is_left: bool,
    ignore: &IgnoreMatcher,
) -> Result<Vec<AlignedNode>, std::io::Error> {
    let full_dir = root.join(relative_path);
    let mut children = Vec::new();
    // Do not follow a symlink directory when building a one-sided tree.
    if fs::symlink_metadata(&full_dir)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        || !full_dir.is_dir()
    {
        return Ok(children);
    }
    if let Ok(entries) = fs::read_dir(&full_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let node_rel_path = relative_path.join(&name);
            if let Some(info) = file_info_from_dir_entry(&entry) {
                if ignore.is_ignored(&node_rel_path, info.is_dir) {
                    continue;
                }
                let sub_children = if info.is_dir {
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

    #[test]
    fn test_compute_file_sha256_known_digest() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("blob.txt");
        fs::write(&path, b"match_content").unwrap();
        // `printf 'match_content' | shasum -a 256`
        assert_eq!(
            compute_file_sha256(&path).unwrap(),
            "c180c1416fc1876d75ef325daf343193268f42d689c3a0bbc69ab02736043941"
        );
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

        // Make left unreadable so compute_file_sha256 fails while metadata still works.
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
    fn test_replace_subtree_updates_child_and_parent_state() {
        let mut root = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: true,
            }),
            right: Some(FileInfo {
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: true,
            }),
            state: DiffState::DifferentSameTime,
            is_expanded: true,
            children: vec![AlignedNode {
                name: "sub".into(),
                relative_path: PathBuf::from("sub"),
                left: Some(FileInfo {
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                    is_dir: true,
                }),
                right: None,
                state: DiffState::LeftOnly,
                is_expanded: true,
                children: vec![],
            }],
        };

        let new_sub = AlignedNode {
            name: "sub".into(),
            relative_path: PathBuf::from("sub"),
            left: Some(FileInfo {
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: true,
            }),
            right: Some(FileInfo {
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: true,
            }),
            state: DiffState::Identical,
            is_expanded: false,
            children: vec![],
        };

        assert!(replace_subtree(&mut root, Path::new("sub"), new_sub));
        assert_eq!(root.children[0].state, DiffState::Identical);
        assert!(
            root.children[0].is_expanded,
            "previous expand flag should be preserved"
        );
        assert_eq!(
            root.state,
            DiffState::Identical,
            "parent folder state recomputed"
        );
    }

    /// Cyclic directory symlinks must not hang or stack-overflow the scanner.
    #[cfg(unix)]
    #[test]
    fn test_scan_does_not_follow_symlink_cycles() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();
        fs::create_dir(left.join("real")).unwrap();
        File::create(left.join("real").join("file.txt")).unwrap();
        // Cycle: left/loop -> left
        std::os::unix::fs::symlink(&left, left.join("loop")).unwrap();
        // Mirror a plain tree on the right so align still runs both sides.
        fs::create_dir(right.join("real")).unwrap();
        File::create(right.join("real").join("file.txt")).unwrap();

        let root = align_directories(
            &left,
            &right,
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .expect("scan should finish despite symlink cycle");

        let loop_node = root
            .children
            .iter()
            .find(|n| n.name == "loop")
            .expect("symlink should appear as a leaf entry");
        assert!(
            !loop_node.left.as_ref().unwrap().is_dir,
            "symlink must not be treated as an expandable directory"
        );
        assert!(
            loop_node.children.is_empty(),
            "symlink must not be expanded"
        );
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
