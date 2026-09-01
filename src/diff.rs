use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ignore::IgnoreMatcher;

/// Why a pair is [`DiffState::Unverified`] (`≈`) rather than `=` or `≠`.
///
/// Both mean "the bytes were never compared", which is not the same claim as
/// "the bytes differ" — the distinction Issue #232 was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnverifiedReason {
    /// Fast mode: sizes match but timestamps differ, so no content was read.
    NotCompared,
    /// Precise mode: a side could not be read or hashed, so content is unknown.
    HashFailed,
}

impl UnverifiedReason {
    /// Short suffix for the selected-row detail line.
    pub fn detail(self) -> &'static str {
        match self {
            UnverifiedReason::NotCompared => "content unverified (fast scan)",
            UnverifiedReason::HashFailed => "content unverified (read failed)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    Identical,
    /// `≈` — the active scan mode did not establish whether the contents match.
    Unverified(UnverifiedReason),
    DifferentNewerLeft,
    DifferentNewerRight,
    DifferentSameTime,
    LeftOnly,
    RightOnly,
    TypeConflict,
}

impl DiffState {
    /// Whether this state is a difference the scan actually established, as
    /// opposed to `Identical` or an unverified `≈`. Directory aggregation
    /// promotes a folder to `≠` as soon as one descendant qualifies.
    pub fn is_known_difference(self) -> bool {
        !matches!(self, DiffState::Identical | DiffState::Unverified(_))
    }
}

/// Inventory of aligned **leaf** pairs in a scanned tree (Issue #252).
///
/// Directory rollups are skipped so a folder is not counted alongside the
/// files that made it `≠`. Collapsed descendants still count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeSummary {
    pub differ: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub unverified: usize,
    pub identical: usize,
}

impl TreeSummary {
    /// Walk `root`'s descendants (not `root` itself).
    pub fn from_root(root: &AlignedNode) -> Self {
        let mut summary = Self::default();
        for child in &root.children {
            summary.add_tree(child);
        }
        summary
    }

    fn add_tree(&mut self, node: &AlignedNode) {
        if node.children.is_empty() {
            match node.state {
                DiffState::Identical => self.identical += 1,
                DiffState::Unverified(_) => self.unverified += 1,
                DiffState::LeftOnly => self.left_only += 1,
                DiffState::RightOnly => self.right_only += 1,
                DiffState::DifferentNewerLeft
                | DiffState::DifferentNewerRight
                | DiffState::DifferentSameTime
                | DiffState::TypeConflict => self.differ += 1,
            }
            return;
        }
        for child in &node.children {
            self.add_tree(child);
        }
    }
}

/// Aggregate a directory's state from its children's: any established
/// difference wins, otherwise unverified descendants leave the folder `≈`
/// (carrying the first such reason in child order), otherwise the folder is `=`.
fn aggregate_child_states<'a>(states: impl Iterator<Item = &'a DiffState>) -> DiffState {
    let mut unverified = None;
    for state in states {
        if state.is_known_difference() {
            return DiffState::DifferentSameTime;
        }
        if let DiffState::Unverified(reason) = state {
            unverified.get_or_insert(*reason);
        }
    }
    match unverified {
        Some(reason) => DiffState::Unverified(reason),
        None => DiffState::Identical,
    }
}

use unicode_normalization::UnicodeNormalization;

/// Normalize a string for matching using Unicode default case folding (lowercase) plus NFC.
pub fn normalize_for_matching(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).nfc().collect()
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
    pub left_name: Option<String>,
    pub right_name: Option<String>,
    pub left_relative_path: Option<PathBuf>,
    pub right_relative_path: Option<PathBuf>,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
    pub state: DiffState,
    pub is_expanded: bool,
    pub has_case_conflict: bool,
    pub contains_case_conflict: bool,
    pub is_ambiguous_case_collision: bool,
    pub children: Vec<AlignedNode>,
}

impl Default for AlignedNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            relative_path: PathBuf::new(),
            left_name: None,
            right_name: None,
            left_relative_path: None,
            right_relative_path: None,
            left: None,
            right: None,
            state: DiffState::Identical,
            is_expanded: false,
            has_case_conflict: false,
            contains_case_conflict: false,
            is_ambiguous_case_collision: false,
            children: Vec::new(),
        }
    }
}

impl AlignedNode {
    /// A node for a name present on both sides.
    ///
    /// The scan builds nodes in a handful of shapes; these constructors keep the
    /// flag combination for each shape in one place instead of restating all
    /// fourteen fields at every site (Issue #306).
    pub(crate) fn matched(left: ScannedEntry, right: &ScannedEntry, state: DiffState) -> Self {
        Self {
            name: left.name.clone(),
            relative_path: left.rel_path.clone(),
            left_name: Some(left.name),
            right_name: Some(right.name.clone()),
            left_relative_path: Some(left.rel_path),
            right_relative_path: Some(right.rel_path.clone()),
            left: Some(left.info),
            right: Some(right.info.clone()),
            state,
            ..Default::default()
        }
    }

    /// A node for a name present only on the left. The side fixes the state, so
    /// there is no way for a caller to contradict it.
    pub(crate) fn left_only(entry: ScannedEntry, children: Vec<AlignedNode>) -> Self {
        Self {
            name: entry.name.clone(),
            relative_path: entry.rel_path.clone(),
            left_name: Some(entry.name),
            left_relative_path: Some(entry.rel_path),
            left: Some(entry.info),
            state: DiffState::LeftOnly,
            children,
            ..Default::default()
        }
    }

    /// A node for a name present only on the right.
    pub(crate) fn right_only(entry: ScannedEntry, children: Vec<AlignedNode>) -> Self {
        Self {
            name: entry.name.clone(),
            relative_path: entry.rel_path.clone(),
            right_name: Some(entry.name),
            right_relative_path: Some(entry.rel_path),
            right: Some(entry.info),
            state: DiffState::RightOnly,
            children,
            ..Default::default()
        }
    }

    /// The two sides of this node differ only by case.
    pub(crate) fn with_case_conflict(mut self) -> Self {
        self.has_case_conflict = true;
        self.contains_case_conflict = true;
        self
    }

    /// Several entries fold to this node's name, so no pairing is unambiguous.
    pub(crate) fn with_ambiguous_case_collision(mut self) -> Self {
        self.contains_case_conflict = true;
        self.is_ambiguous_case_collision = true;
        self
    }

    /// Return the real left relative path if this node exists on the left.
    pub fn left_relative_path(&self) -> Option<&Path> {
        self.left_relative_path
            .as_deref()
            .or(if self.left.is_some() {
                Some(&self.relative_path)
            } else {
                None
            })
    }

    /// Return the real right relative path if this node exists on the right.
    pub fn right_relative_path(&self) -> Option<&Path> {
        self.right_relative_path
            .as_deref()
            .or(if self.right.is_some() {
                Some(&self.relative_path)
            } else {
                None
            })
    }

    /// Return the real left basename if this node exists on the left.
    pub fn left_name(&self) -> Option<&str> {
        self.left_name.as_deref().or(if self.left.is_some() {
            Some(&self.name)
        } else {
            None
        })
    }

    /// Return the real right basename if this node exists on the right.
    pub fn right_name(&self) -> Option<&str> {
        self.right_name.as_deref().or(if self.right.is_some() {
            Some(&self.name)
        } else {
            None
        })
    }
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

#[derive(Clone, Debug)]
pub(crate) struct ScannedEntry {
    pub(crate) name: String,
    pub(crate) raw_name: std::ffi::OsString,
    pub(crate) rel_path: PathBuf,
    pub(crate) info: FileInfo,
    pub(crate) is_valid_unicode: bool,
}

fn compute_file_state(
    left_root: &Path,
    right_root: &Path,
    left_rel_path: &Path,
    right_rel_path: &Path,
    left: &FileInfo,
    right: &FileInfo,
    precise_mode: bool,
) -> DiffState {
    if left.size != right.size {
        // A size mismatch is a difference the scan established.
        different_by_mtime(left.modified, right.modified)
    } else if precise_mode {
        let left_full = left_root.join(left_rel_path);
        let right_full = right_root.join(right_rel_path);
        match (
            compute_file_sha256(&left_full),
            compute_file_sha256(&right_full),
        ) {
            (Ok(left_hash), Ok(right_hash)) if left_hash == right_hash => DiffState::Identical,
            (Ok(_), Ok(_)) => different_by_mtime(left.modified, right.modified),
            _ => DiffState::Unverified(UnverifiedReason::HashFailed),
        }
    } else if left.modified == right.modified {
        DiffState::Identical
    } else {
        DiffState::Unverified(UnverifiedReason::NotCompared)
    }
}

#[cfg(test)]
impl ScannedEntry {
    pub(crate) fn new_file(name: &str, size: u64) -> Self {
        Self {
            name: name.to_string(),
            raw_name: std::ffi::OsString::from(name),
            rel_path: PathBuf::from(name),
            info: FileInfo {
                size,
                modified: SystemTime::UNIX_EPOCH,
                is_dir: false,
            },
            is_valid_unicode: true,
        }
    }
}

#[cfg(test)]
pub(crate) fn align_scanned_entries(
    left_root: &Path,
    right_root: &Path,
    left_entries: Vec<ScannedEntry>,
    right_entries: Vec<ScannedEntry>,
    precise_mode: bool,
    left_ignore: &mut IgnoreMatcher,
    right_ignore: &mut IgnoreMatcher,
) -> Result<Vec<AlignedNode>, std::io::Error> {
    let mut count = 0usize;
    align_scanned_entries_internal(
        left_root,
        right_root,
        left_entries,
        right_entries,
        precise_mode,
        left_ignore,
        right_ignore,
        &mut count,
        &mut |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
fn align_scanned_entries_internal(
    left_root: &Path,
    right_root: &Path,
    left_entries: Vec<ScannedEntry>,
    right_entries: Vec<ScannedEntry>,
    precise_mode: bool,
    left_ignore: &mut IgnoreMatcher,
    right_ignore: &mut IgnoreMatcher,
    count: &mut usize,
    on_progress: &mut dyn FnMut(usize),
) -> Result<Vec<AlignedNode>, std::io::Error> {
    let mut children = Vec::new();

    // 1. Exact match pass by raw_name
    let mut unmatched_left = Vec::new();
    let mut matched_right_indices = std::collections::HashSet::new();

    for left_entry in left_entries {
        if let Some((r_idx, right_entry)) = right_entries
            .iter()
            .enumerate()
            .find(|(i, r)| !matched_right_indices.contains(i) && r.raw_name == left_entry.raw_name)
        {
            matched_right_indices.insert(r_idx);
            let node = if left_entry.info.is_dir != right_entry.info.is_dir {
                AlignedNode::matched(left_entry, right_entry, DiffState::TypeConflict)
            } else if left_entry.info.is_dir {
                align_directories_with_paths_internal(
                    left_root,
                    right_root,
                    &left_entry.rel_path,
                    &right_entry.rel_path,
                    precise_mode,
                    left_ignore,
                    right_ignore,
                    count,
                    on_progress,
                )?
            } else {
                let state = compute_file_state(
                    left_root,
                    right_root,
                    &left_entry.rel_path,
                    &right_entry.rel_path,
                    &left_entry.info,
                    &right_entry.info,
                    precise_mode,
                );
                AlignedNode::matched(left_entry, right_entry, state)
            };
            children.push(node);
        } else {
            unmatched_left.push(left_entry);
        }
    }

    let mut unmatched_right = Vec::new();
    for (i, right_entry) in right_entries.into_iter().enumerate() {
        if !matched_right_indices.contains(&i) {
            unmatched_right.push(right_entry);
        }
    }

    // 2. Case-folded match on remaining entries with valid Unicode basenames
    let mut left_folded: BTreeMap<String, Vec<ScannedEntry>> = BTreeMap::new();
    let mut left_non_unicode = Vec::new();
    for entry in unmatched_left {
        if entry.is_valid_unicode {
            let key = normalize_for_matching(&entry.name);
            left_folded.entry(key).or_default().push(entry);
        } else {
            left_non_unicode.push(entry);
        }
    }

    let mut right_folded: BTreeMap<String, Vec<ScannedEntry>> = BTreeMap::new();
    let mut right_non_unicode = Vec::new();
    for entry in unmatched_right {
        if entry.is_valid_unicode {
            let key = normalize_for_matching(&entry.name);
            right_folded.entry(key).or_default().push(entry);
        } else {
            right_non_unicode.push(entry);
        }
    }

    let mut all_folded_keys: Vec<String> = left_folded.keys().cloned().collect();
    for k in right_folded.keys() {
        if !all_folded_keys.contains(k) {
            all_folded_keys.push(k.clone());
        }
    }
    all_folded_keys.sort();

    for key in all_folded_keys {
        let left_c = left_folded.remove(&key).unwrap_or_default();
        let right_c = right_folded.remove(&key).unwrap_or_default();

        match (left_c.len(), right_c.len()) {
            (1, 1) => {
                // Unique case-folded match!
                let left_entry = left_c.into_iter().next().unwrap();
                let right_entry = right_c.into_iter().next().unwrap();

                let node = if left_entry.info.is_dir != right_entry.info.is_dir {
                    AlignedNode::matched(left_entry, &right_entry, DiffState::TypeConflict)
                        .with_case_conflict()
                } else if left_entry.info.is_dir {
                    let mut dir_node = align_directories_with_paths_internal(
                        left_root,
                        right_root,
                        &left_entry.rel_path,
                        &right_entry.rel_path,
                        precise_mode,
                        left_ignore,
                        right_ignore,
                        count,
                        on_progress,
                    )?;
                    dir_node.has_case_conflict = true;
                    dir_node.contains_case_conflict = true;
                    dir_node.left_name = Some(left_entry.name);
                    dir_node.right_name = Some(right_entry.name);
                    dir_node.left_relative_path = Some(left_entry.rel_path);
                    dir_node.right_relative_path = Some(right_entry.rel_path);
                    dir_node
                } else {
                    let state = compute_file_state(
                        left_root,
                        right_root,
                        &left_entry.rel_path,
                        &right_entry.rel_path,
                        &left_entry.info,
                        &right_entry.info,
                        precise_mode,
                    );
                    AlignedNode::matched(left_entry, &right_entry, state).with_case_conflict()
                };
                children.push(node);
            }
            (l_len, r_len) if l_len > 0 && r_len > 0 => {
                // Ambiguous case collision (e.g. 2 vs 1, 1 vs 2, 2 vs 2)
                for l_entry in left_c {
                    let sub_children = if l_entry.info.is_dir {
                        make_single_sided_tree_internal(
                            left_root,
                            &l_entry.rel_path,
                            true,
                            left_ignore,
                            count,
                            on_progress,
                        )?
                    } else {
                        Vec::new()
                    };
                    children.push(
                        AlignedNode::left_only(l_entry, sub_children)
                            .with_ambiguous_case_collision(),
                    );
                }
                for r_entry in right_c {
                    let sub_children = if r_entry.info.is_dir {
                        make_single_sided_tree_internal(
                            right_root,
                            &r_entry.rel_path,
                            false,
                            right_ignore,
                            count,
                            on_progress,
                        )?
                    } else {
                        Vec::new()
                    };
                    children.push(
                        AlignedNode::right_only(r_entry, sub_children)
                            .with_ambiguous_case_collision(),
                    );
                }
            }
            (l_len, 0) if l_len > 0 => {
                for l_entry in left_c {
                    let sub_children = if l_entry.info.is_dir {
                        make_single_sided_tree_internal(
                            left_root,
                            &l_entry.rel_path,
                            true,
                            left_ignore,
                            count,
                            on_progress,
                        )?
                    } else {
                        Vec::new()
                    };
                    children.push(AlignedNode::left_only(l_entry, sub_children));
                }
            }
            (0, r_len) if r_len > 0 => {
                for r_entry in right_c {
                    let sub_children = if r_entry.info.is_dir {
                        make_single_sided_tree_internal(
                            right_root,
                            &r_entry.rel_path,
                            false,
                            right_ignore,
                            count,
                            on_progress,
                        )?
                    } else {
                        Vec::new()
                    };
                    children.push(AlignedNode::right_only(r_entry, sub_children));
                }
            }
            _ => {}
        }
    }

    // 3. Non-Unicode entries
    for l_entry in left_non_unicode {
        let sub_children = if l_entry.info.is_dir {
            make_single_sided_tree_internal(
                left_root,
                &l_entry.rel_path,
                true,
                left_ignore,
                count,
                on_progress,
            )?
        } else {
            Vec::new()
        };
        children.push(AlignedNode::left_only(l_entry, sub_children));
    }
    for r_entry in right_non_unicode {
        let sub_children = if r_entry.info.is_dir {
            make_single_sided_tree_internal(
                right_root,
                &r_entry.rel_path,
                false,
                right_ignore,
                count,
                on_progress,
            )?
        } else {
            Vec::new()
        };
        children.push(AlignedNode::right_only(r_entry, sub_children));
    }

    // Sort children: folded normalized name first, then original name
    children.sort_by(|a, b| {
        let a_key = normalize_for_matching(&a.name);
        let b_key = normalize_for_matching(&b.name);
        a_key.cmp(&b_key).then_with(|| a.name.cmp(&b.name))
    });

    Ok(children)
}

fn make_single_sided_tree_internal(
    root: &Path,
    rel_path: &Path,
    is_left: bool,
    ignore: &mut IgnoreMatcher,
    count: &mut usize,
    on_progress: &mut dyn FnMut(usize),
) -> Result<Vec<AlignedNode>, std::io::Error> {
    let dir = root.join(rel_path);
    let mut children = Vec::new();
    // Do not follow a symlink directory when building a one-sided tree.
    if fs::symlink_metadata(&dir)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
        || !dir.is_dir()
    {
        return Ok(children);
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let raw_name = entry.file_name();
            let name = raw_name.to_string_lossy().into_owned();
            let node_rel_path = rel_path.join(&raw_name);
            if let Some(info) = file_info_from_dir_entry(&entry) {
                if ignore.is_ignored(&node_rel_path, info.is_dir)? {
                    continue;
                }
                *count += 1;
                on_progress(*count);
                let is_dir = info.is_dir;
                let sub_children = if is_dir {
                    make_single_sided_tree_internal(
                        root,
                        &node_rel_path,
                        is_left,
                        ignore,
                        count,
                        on_progress,
                    )?
                } else {
                    Vec::new()
                };
                let (left_info, right_info, left_name, right_name, left_rel, right_rel, state) =
                    if is_left {
                        (
                            Some(info),
                            None,
                            Some(name.clone()),
                            None,
                            Some(node_rel_path.clone()),
                            None,
                            DiffState::LeftOnly,
                        )
                    } else {
                        (
                            None,
                            Some(info),
                            None,
                            Some(name.clone()),
                            None,
                            Some(node_rel_path.clone()),
                            DiffState::RightOnly,
                        )
                    };
                children.push(AlignedNode {
                    name,
                    relative_path: node_rel_path,
                    left_name,
                    right_name,
                    left_relative_path: left_rel,
                    right_relative_path: right_rel,
                    left: left_info,
                    right: right_info,
                    state,
                    children: sub_children,
                    ..Default::default()
                });
            }
        }
    }
    children.sort_by(|a, b| {
        let a_key = normalize_for_matching(&a.name);
        let b_key = normalize_for_matching(&b.name);
        a_key.cmp(&b_key).then_with(|| a.name.cmp(&b.name))
    });
    Ok(children)
}

/// Scan and align two directory trees, each under its own exclusion rules.
///
/// The one way to scan: `on_progress` receives a running item count, and a
/// caller that does not want progress passes `&mut |_| {}` (Issue #306).
#[allow(clippy::too_many_arguments)]
pub fn align_directories(
    left_root: &Path,
    right_root: &Path,
    relative_path: &Path,
    precise_mode: bool,
    left_ignore: &mut IgnoreMatcher,
    right_ignore: &mut IgnoreMatcher,
    on_progress: &mut dyn FnMut(usize),
) -> Result<AlignedNode, std::io::Error> {
    let mut count = 0usize;
    align_directories_with_paths_internal(
        left_root,
        right_root,
        relative_path,
        relative_path,
        precise_mode,
        left_ignore,
        right_ignore,
        &mut count,
        on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn align_directories_with_paths_internal(
    left_root: &Path,
    right_root: &Path,
    left_rel_path: &Path,
    right_rel_path: &Path,
    precise_mode: bool,
    left_ignore: &mut IgnoreMatcher,
    right_ignore: &mut IgnoreMatcher,
    count: &mut usize,
    on_progress: &mut dyn FnMut(usize),
) -> Result<AlignedNode, std::io::Error> {
    let left_dir = left_root.join(left_rel_path);
    let right_dir = right_root.join(right_rel_path);

    let mut left_entries: Vec<ScannedEntry> = Vec::new();
    if left_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&left_dir) {
            for entry in entries.flatten() {
                let raw_name = entry.file_name();
                let is_valid_unicode = raw_name.to_str().is_some();
                let name = raw_name.to_string_lossy().into_owned();
                let node_rel_path = left_rel_path.join(&raw_name);
                if let Some(info) = file_info_from_dir_entry(&entry) {
                    if left_ignore.is_ignored(&node_rel_path, info.is_dir)? {
                        continue;
                    }
                    *count += 1;
                    on_progress(*count);
                    left_entries.push(ScannedEntry {
                        name,
                        raw_name,
                        rel_path: node_rel_path,
                        info,
                        is_valid_unicode,
                    });
                }
            }
        }
    }

    let mut right_entries: Vec<ScannedEntry> = Vec::new();
    if right_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&right_dir) {
            for entry in entries.flatten() {
                let raw_name = entry.file_name();
                let is_valid_unicode = raw_name.to_str().is_some();
                let name = raw_name.to_string_lossy().into_owned();
                let node_rel_path = right_rel_path.join(&raw_name);
                if let Some(info) = file_info_from_dir_entry(&entry) {
                    if right_ignore.is_ignored(&node_rel_path, info.is_dir)? {
                        continue;
                    }
                    *count += 1;
                    on_progress(*count);
                    right_entries.push(ScannedEntry {
                        name,
                        raw_name,
                        rel_path: node_rel_path,
                        info,
                        is_valid_unicode,
                    });
                }
            }
        }
    }

    let children = align_scanned_entries_internal(
        left_root,
        right_root,
        left_entries,
        right_entries,
        precise_mode,
        left_ignore,
        right_ignore,
        count,
        on_progress,
    )?;

    let folder_state = aggregate_child_states(children.iter().map(|c| &c.state));
    let contains_conflict = children
        .iter()
        .any(|c| c.has_case_conflict || c.contains_case_conflict || c.is_ambiguous_case_collision);

    let root_name = left_rel_path
        .file_name()
        .or_else(|| right_rel_path.file_name())
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
        name: root_name.clone(),
        relative_path: left_rel_path.to_path_buf(),
        left_name: Some(root_name.clone()),
        right_name: Some(root_name),
        left_relative_path: Some(left_rel_path.to_path_buf()),
        right_relative_path: Some(right_rel_path.to_path_buf()),
        left: left_info,
        right: right_info,
        state: folder_state,
        is_expanded: true,
        contains_case_conflict: contains_conflict,
        children,
        ..Default::default()
    })
}

/// Recompute a directory node's aggregate state from its children.
pub fn recompute_folder_state_from_children(node: &mut AlignedNode) {
    let is_dir = node.left.as_ref().is_some_and(|f| f.is_dir)
        || node.right.as_ref().is_some_and(|f| f.is_dir);
    if !is_dir {
        return;
    }
    node.state = aggregate_child_states(node.children.iter().map(|c| &c.state));
    node.contains_case_conflict = node.has_case_conflict
        || node.children.iter().any(|c| {
            c.has_case_conflict || c.contains_case_conflict || c.is_ambiguous_case_collision
        });
}

/// Replace the subtree at `path` (relative) with `new_node`, then refresh
/// ancestor folder states. Returns `false` if `path` is not in the tree.
pub fn replace_subtree(root: &mut AlignedNode, path: &Path, mut new_node: AlignedNode) -> bool {
    let matches_root = root.relative_path == path
        || root.left_relative_path.as_deref() == Some(path)
        || root.right_relative_path.as_deref() == Some(path);
    if matches_root {
        let expanded = root.is_expanded;
        // Keep expansion preference for this directory.
        new_node.is_expanded = expanded || new_node.is_expanded;
        *root = new_node;
        return true;
    }
    for child in &mut root.children {
        let child_matches = path == child.relative_path
            || path.starts_with(&child.relative_path)
            || child
                .left_relative_path
                .as_ref()
                .is_some_and(|p| path == p || path.starts_with(p))
            || child
                .right_relative_path
                .as_ref()
                .is_some_and(|p| path == p || path.starts_with(p));
        if child_matches {
            if replace_subtree(child, path, new_node) {
                recompute_folder_state_from_children(root);
                return true;
            }
            return false;
        }
    }
    false
}

/// Test-only convenience for scans that intentionally apply one exclusion
/// matcher to both roots. Production scans go through [`align_directories`].
#[cfg(test)]
pub fn align_directories_with_shared_matcher(
    left_root: &Path,
    right_root: &Path,
    relative_path: &Path,
    precise_mode: bool,
    ignore_matcher: &IgnoreMatcher,
) -> Result<AlignedNode, std::io::Error> {
    let mut left_ignore = ignore_matcher.clone();
    let mut right_ignore = ignore_matcher.clone();
    align_directories(
        left_root,
        right_root,
        relative_path,
        precise_mode,
        &mut left_ignore,
        &mut right_ignore,
        &mut |_| {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn named_node(name: &str, state: DiffState, children: Vec<AlignedNode>) -> AlignedNode {
        AlignedNode {
            name: name.to_string(),
            relative_path: PathBuf::from(name),
            state,
            children,
            ..Default::default()
        }
    }

    /// Issue #252: summary counts every descendant, including collapsed folders.
    #[test]
    fn test_tree_summary_counts_descendants_not_the_synthetic_root() {
        let root = AlignedNode {
            children: vec![
                named_node("same.txt", DiffState::Identical, vec![]),
                named_node("only-left.txt", DiffState::LeftOnly, vec![]),
                named_node("only-right.txt", DiffState::RightOnly, vec![]),
                named_node("changed.txt", DiffState::DifferentNewerLeft, vec![]),
                named_node(
                    "stale.txt",
                    DiffState::Unverified(UnverifiedReason::NotCompared),
                    vec![],
                ),
                named_node("conflict", DiffState::TypeConflict, vec![]),
                named_node(
                    "folder",
                    DiffState::DifferentSameTime,
                    vec![named_node(
                        "nested.txt",
                        DiffState::DifferentNewerRight,
                        vec![],
                    )],
                ),
            ],
            ..Default::default()
        };

        assert_eq!(
            TreeSummary::from_root(&root),
            TreeSummary {
                differ: 3, // changed, conflict, nested (folder is a rollup)
                left_only: 1,
                right_only: 1,
                unverified: 1,
                identical: 1,
            }
        );
    }

    #[test]
    fn test_tree_summary_empty_root_is_all_zeros() {
        assert_eq!(
            TreeSummary::from_root(&AlignedNode::default()),
            TreeSummary::default()
        );
    }

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

        let root_node = align_directories_with_shared_matcher(
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
        let root_node = align_directories_with_shared_matcher(
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

        let root_node = align_directories_with_shared_matcher(
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
        // An unreadable side says nothing about whether the bytes differ, so it
        // is `≈` rather than `≠` (Issue #232).
        assert_eq!(
            node.state,
            DiffState::Unverified(UnverifiedReason::HashFailed)
        );

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

        let root_node = align_directories_with_shared_matcher(
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
                ..Default::default()
            }],
            ..Default::default()
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
            ..Default::default()
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

        let root = align_directories_with_shared_matcher(
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

        let matcher =
            IgnoreMatcher::for_root(left.clone(), &["skip.txt".to_string()], true, &[]).unwrap();

        let root_node =
            align_directories_with_shared_matcher(&left, &right, Path::new(""), false, &matcher)
                .unwrap();
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

        let matcher =
            IgnoreMatcher::for_root(left.clone(), &["target/".to_string()], true, &[]).unwrap();

        let root_node =
            align_directories_with_shared_matcher(&left, &right, Path::new(""), false, &matcher)
                .unwrap();
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

        let matcher =
            IgnoreMatcher::for_root(left.clone(), &["ignored".to_string()], true, &[]).unwrap();

        let root_node =
            align_directories_with_shared_matcher(&left, &right, Path::new(""), false, &matcher)
                .unwrap();
        let left_only_node = root_node
            .children
            .iter()
            .find(|n| n.name == "left_only")
            .unwrap();
        assert_eq!(left_only_node.children.len(), 1);
        assert!(left_only_node.children.iter().any(|n| n.name == "visible"));
    }

    /// Issue #232: the backup-vs-working-copy case. Same bytes, same size, but a
    /// rewritten mtime must not be reported as a content difference in Fast mode.
    #[test]
    fn test_fast_mode_equal_size_different_mtime_is_unverified_not_different() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        let earlier = SystemTime::UNIX_EPOCH;
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(600);
        {
            let mut f = File::create(left.join("image.png")).unwrap();
            f.write_all(b"identical-bytes").unwrap();
            f.set_modified(earlier).unwrap();
        }
        {
            let mut f = File::create(right.join("image.png")).unwrap();
            f.write_all(b"identical-bytes").unwrap();
            f.set_modified(later).unwrap();
        }

        let fast = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        assert_eq!(
            fast.children[0].state,
            DiffState::Unverified(UnverifiedReason::NotCompared),
            "Fast mode never read the bytes, so it cannot claim they differ"
        );
        assert_eq!(
            fast.state,
            DiffState::Unverified(UnverifiedReason::NotCompared),
            "a folder whose descendants are only unverified stays unverified"
        );

        // Precise mode hashes the bytes and can claim equality despite the mtime.
        let precise = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        assert_eq!(precise.children[0].state, DiffState::Identical);
        assert_eq!(precise.state, DiffState::Identical);
    }

    #[test]
    fn test_fast_mode_size_mismatch_is_still_a_known_difference() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        let earlier = SystemTime::UNIX_EPOCH;
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(600);
        {
            let mut f = File::create(left.join("notes.md")).unwrap();
            f.write_all(b"short").unwrap();
            f.set_modified(earlier).unwrap();
        }
        {
            let mut f = File::create(right.join("notes.md")).unwrap();
            f.write_all(b"much longer content").unwrap();
            f.set_modified(later).unwrap();
        }

        let root = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        assert_eq!(root.children[0].state, DiffState::DifferentNewerRight);
        assert!(root.children[0].state.is_known_difference());
        assert_eq!(
            root.state,
            DiffState::DifferentSameTime,
            "one established difference promotes the folder to `≠`"
        );
    }

    #[test]
    fn test_aggregate_child_states_ranks_known_difference_over_unverified() {
        use DiffState::*;
        assert_eq!(aggregate_child_states([].iter()), Identical);
        assert_eq!(
            aggregate_child_states([Identical, Identical].iter()),
            Identical
        );
        assert_eq!(
            aggregate_child_states([Identical, Unverified(UnverifiedReason::NotCompared)].iter()),
            Unverified(UnverifiedReason::NotCompared)
        );
        // Mixed unverified peers keep the first reason in child order; the
        // folder is `≈` either way and no reason outranks another.
        assert_eq!(
            aggregate_child_states(
                [
                    Unverified(UnverifiedReason::NotCompared),
                    Unverified(UnverifiedReason::HashFailed),
                ]
                .iter()
            ),
            Unverified(UnverifiedReason::NotCompared)
        );
        // Any established difference wins outright.
        assert_eq!(
            aggregate_child_states([Unverified(UnverifiedReason::HashFailed), LeftOnly].iter()),
            DifferentSameTime
        );
    }

    #[test]
    fn test_recompute_folder_state_keeps_unverified_descendants_unverified() {
        let mut node = AlignedNode {
            name: "dir".to_string(),
            relative_path: PathBuf::from("dir"),
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
                name: "a.txt".to_string(),
                relative_path: PathBuf::from("dir/a.txt"),
                left: None,
                right: None,
                state: DiffState::Unverified(UnverifiedReason::NotCompared),
                is_expanded: false,
                children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        recompute_folder_state_from_children(&mut node);
        assert_eq!(
            node.state,
            DiffState::Unverified(UnverifiedReason::NotCompared)
        );

        node.children.push(AlignedNode {
            name: "b.txt".to_string(),
            relative_path: PathBuf::from("dir/b.txt"),
            left: None,
            right: None,
            state: DiffState::LeftOnly,
            is_expanded: false,
            children: Vec::new(),
            ..Default::default()
        });
        recompute_folder_state_from_children(&mut node);
        assert_eq!(node.state, DiffState::DifferentSameTime);
    }

    #[test]
    fn test_exact_before_folded_alignment() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();

        // Left has "exact_file.txt" and "FOLDED_FILE.txt"
        // Right has "exact_file.txt" and "folded_file.txt"
        fs::write(left.join("exact_file.txt"), "exact content").unwrap();
        fs::write(left.join("FOLDED_FILE.txt"), "upper content").unwrap();
        fs::write(right.join("exact_file.txt"), "exact content").unwrap();
        fs::write(right.join("folded_file.txt"), "upper content").unwrap();

        let root_node = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        // There should be 2 pairs:
        // 1. "exact_file.txt" paired exactly (has_case_conflict = false)
        // 2. "FOLDED_FILE.txt" vs "folded_file.txt" paired uniquely by case-folding (has_case_conflict = true)
        assert_eq!(root_node.children.len(), 2);

        let exact = root_node
            .children
            .iter()
            .find(|n| {
                n.left_name.as_deref() == Some("exact_file.txt")
                    && n.right_name.as_deref() == Some("exact_file.txt")
            })
            .expect("exact pair must exist");
        assert!(!exact.has_case_conflict);
        assert_eq!(exact.state, DiffState::Identical);

        let folded = root_node
            .children
            .iter()
            .find(|n| {
                n.left_name.as_deref() == Some("FOLDED_FILE.txt")
                    && n.right_name.as_deref() == Some("folded_file.txt")
            })
            .expect("folded pair must exist");
        assert!(folded.has_case_conflict);
        assert_eq!(folded.state, DiffState::Identical);
    }

    #[test]
    fn test_unique_case_mismatch_directory_recursive_alignment() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir_all(left.join("Folder/Sub")).unwrap();
        fs::create_dir_all(right.join("folder/sub")).unwrap();

        fs::write(left.join("Folder/Sub/A.txt"), "hello").unwrap();
        fs::write(right.join("folder/sub/a.txt"), "hello").unwrap();

        let root_node = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        assert_eq!(root_node.children.len(), 1);
        let top_dir = &root_node.children[0];
        assert!(top_dir.has_case_conflict);
        assert!(top_dir.contains_case_conflict);
        assert_eq!(top_dir.left_name.as_deref(), Some("Folder"));
        assert_eq!(top_dir.right_name.as_deref(), Some("folder"));
        assert_eq!(
            top_dir.left_relative_path.as_deref(),
            Some(Path::new("Folder"))
        );
        assert_eq!(
            top_dir.right_relative_path.as_deref(),
            Some(Path::new("folder"))
        );

        let sub_dir = &top_dir.children[0];
        assert!(sub_dir.has_case_conflict);
        assert_eq!(sub_dir.left_name.as_deref(), Some("Sub"));
        assert_eq!(sub_dir.right_name.as_deref(), Some("sub"));
        assert_eq!(
            sub_dir.left_relative_path.as_deref(),
            Some(Path::new("Folder/Sub"))
        );
        assert_eq!(
            sub_dir.right_relative_path.as_deref(),
            Some(Path::new("folder/sub"))
        );

        let file_node = &sub_dir.children[0];
        assert!(file_node.has_case_conflict);
        assert_eq!(file_node.state, DiffState::Identical);
        assert_eq!(file_node.left_name.as_deref(), Some("A.txt"));
        assert_eq!(file_node.right_name.as_deref(), Some("a.txt"));
        assert_eq!(
            file_node.left_relative_path.as_deref(),
            Some(Path::new("Folder/Sub/A.txt"))
        );
        assert_eq!(
            file_node.right_relative_path.as_deref(),
            Some(Path::new("folder/sub/a.txt"))
        );
    }

    #[test]
    fn test_ambiguous_case_collision_preserved_separate() {
        let left_entries = vec![
            ScannedEntry::new_file("Foo", 1),
            ScannedEntry::new_file("foo", 2),
        ];
        let right_entries = vec![ScannedEntry::new_file("FOO", 3)];

        let mut left_ignore = IgnoreMatcher::default();
        let mut right_ignore = IgnoreMatcher::default();
        let children = align_scanned_entries(
            Path::new("/left"),
            Path::new("/right"),
            left_entries,
            right_entries,
            true,
            &mut left_ignore,
            &mut right_ignore,
        )
        .unwrap();

        // Should produce 3 separate nodes marked as ambiguous collision
        assert_eq!(children.len(), 3);
        for child in &children {
            assert!(child.is_ambiguous_case_collision);
            assert!(child.contains_case_conflict);
            assert!(!child.has_case_conflict);
        }

        let left_names: Vec<_> = children
            .iter()
            .filter(|n| n.state == DiffState::LeftOnly)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(left_names, vec!["Foo", "foo"]);

        let right_names: Vec<_> = children
            .iter()
            .filter(|n| n.state == DiffState::RightOnly)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(right_names, vec!["FOO"]);
    }

    #[test]
    fn test_ancestor_aggregation_of_case_conflict() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir_all(left.join("parent/sub")).unwrap();
        fs::create_dir_all(right.join("parent/sub")).unwrap();

        fs::write(left.join("parent/sub/README.md"), "test").unwrap();
        fs::write(right.join("parent/sub/readme.md"), "test").unwrap();

        let root_node = align_directories_with_shared_matcher(
            &left,
            &right,
            Path::new(""),
            true,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        assert!(root_node.contains_case_conflict);
        let parent = &root_node.children[0];
        assert_eq!(parent.name, "parent");
        assert!(!parent.has_case_conflict);
        assert!(parent.contains_case_conflict);
        assert_eq!(parent.state, DiffState::Identical);

        let sub = &parent.children[0];
        assert_eq!(sub.name, "sub");
        assert!(!sub.has_case_conflict);
        assert!(sub.contains_case_conflict);
        assert_eq!(sub.state, DiffState::Identical);

        let file = &sub.children[0];
        assert!(file.has_case_conflict);
        assert_eq!(file.state, DiffState::Identical);
    }

    #[test]
    fn test_unicode_normalization_nfc_and_case_folding() {
        // e with acute accent: composed (NFC) vs decomposed (NFD)
        let nfc = "café";
        let nfd = "cafe\u{0301}";
        assert_ne!(nfc, nfd);
        assert_eq!(normalize_for_matching(nfc), normalize_for_matching(nfd));

        // Case folding + NFC
        let upper_nfd = "CAFE\u{0301}";
        assert_eq!(
            normalize_for_matching(upper_nfd),
            normalize_for_matching(nfc)
        );
    }

    /// Issue #250: align_directories fires progress callback with increasing counts.
    #[test]
    fn test_align_directories_with_progress() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir_all(left.join("sub")).unwrap();
        fs::create_dir_all(right.join("sub")).unwrap();
        fs::write(left.join("file1.txt"), "a").unwrap();
        fs::write(left.join("sub/file2.txt"), "b").unwrap();
        fs::write(right.join("file1.txt"), "a").unwrap();
        fs::write(right.join("sub/file3.txt"), "c").unwrap();

        let mut counts = Vec::new();
        let mut left_ignore = IgnoreMatcher::default();
        let mut right_ignore = IgnoreMatcher::default();

        let _root = align_directories(
            &left,
            &right,
            Path::new(""),
            false,
            &mut left_ignore,
            &mut right_ignore,
            &mut |count| counts.push(count),
        )
        .unwrap();

        assert!(!counts.is_empty(), "progress callback must be called");
        assert_eq!(counts.last().copied(), Some(6));
        for window in counts.windows(2) {
            assert!(window[0] < window[1], "counts must strictly increase");
        }
    }
}
