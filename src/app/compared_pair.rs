//! The Compared pair (`GLOSSARY.md`) and the plans the Commands that act on it
//! build: a copy, an external diff, an editor. A plan is built from the pair
//! and the session's state alone, so the gate, the confirmation, and the effect
//! all read the same one (ADR-0003, ADR-0004).

use super::{App, ConfirmAction, FlatRow, ViewMode};
use crate::diff::FileInfo;
use crate::side::{Pair, Side};
use std::path::{Path, PathBuf};

/// The Compared pair (`GLOSSARY.md`): the two sides File Diff shows and the
/// copy, external-diff, and editor Commands act on. A file-pair session's pair,
/// or the selected Directory Tree row under each root. The one place that tells
/// the two apart, so every gate and effect asks it the same question
/// (ADR-0004).
#[derive(Clone, Copy, Debug)]
pub(crate) enum ComparedPair<'a> {
    Files {
        pair: &'a crate::target::FilePair,
        /// Each side's size and modification time, as last loaded or saved.
        info: &'a Pair<Option<FileInfo>>,
    },
    Row {
        row: &'a FlatRow,
        roots: Pair<&'a Path>,
    },
}

impl<'a> ComparedPair<'a> {
    /// The two files to read, write, and hand to external tools: a file-pair
    /// side's target (a symlink resolved, the null device under the platform's
    /// name), or the row's entry under each root.
    pub(crate) fn paths(&self) -> Pair<PathBuf> {
        Pair::from_fn(|side| self.path(side))
    }

    /// How File Diff reads and writes each side.
    pub(crate) fn sources(&self) -> Pair<super::file_diff_session::SideSource> {
        match self {
            Self::Files { pair, .. } => pair.as_ref().map(crate::target::FileSide::source),
            Self::Row { .. } => self
                .paths()
                .map(super::file_diff_session::SideSource::entry),
        }
    }

    /// The file to read, write, and hand to external tools on `side`. See
    /// [`ComparedPair::paths`].
    pub(crate) fn path(&self, side: Side) -> PathBuf {
        match self {
            Self::Files { pair, .. } => pair.side(side).target_path().to_path_buf(),
            Self::Row { row, roots } => roots.side(side).join(match side {
                Side::Left => row.left_relative_path(),
                Side::Right => row.right_relative_path(),
            }),
        }
    }

    /// The paths the panes' titles show: a file-pair side as the user typed
    /// it, or the row's entry under each root.
    pub(crate) fn titles(&self) -> Pair<PathBuf> {
        match self {
            Self::Files { pair, .. } => pair.as_ref().map(|side| side.path().to_path_buf()),
            Self::Row { .. } => self.paths(),
        }
    }

    /// Size and modification time of each side, for File Diff's panes: a
    /// file-pair side's as last loaded or saved (`None` for one that is not a
    /// regular file), a row's as scanned. `None` for a row with nothing on
    /// either side, which File Diff has no panes for.
    pub(crate) fn info(&self) -> Option<Pair<Option<&'a FileInfo>>> {
        match self {
            Self::Files { info, .. } => Some(info.as_ref().map(Option::as_ref)),
            Self::Row { row, .. } => (row.left.is_some() || row.right.is_some())
                .then(|| Pair::new(row.left.as_ref(), row.right.as_ref())),
        }
    }

    /// Whether `side` is a file — not a directory, not nothing, not a pipe or
    /// the null device. What an editor can open.
    pub(crate) fn has_file(&self, side: Side) -> bool {
        match self {
            Self::Files { pair, .. } => pair.side(side).is_regular_file(),
            Self::Row { row, .. } => row.side(side).as_ref().is_some_and(|file| !file.is_dir),
        }
    }

    /// Whether `side` has anything to copy: the null device, like a row's
    /// absent side, has nothing.
    pub(crate) fn has_content(&self, side: Side) -> bool {
        match self {
            Self::Files { pair, .. } => !pair.side(side).is_null_device(),
            Self::Row { row, .. } => row.side(side).is_some(),
        }
    }

    /// Whether `side` can be written. A row's side always can; a file-pair
    /// side only when its file opened for writing.
    pub(crate) fn is_writable(&self, side: Side) -> bool {
        match self {
            Self::Files { pair, .. } => pair.side(side).is_writable(),
            Self::Row { .. } => true,
        }
    }

    /// Whether both sides can be read again from disk, as an external tool
    /// needs. A side captured from a pipe cannot.
    pub(crate) fn can_reopen(&self) -> bool {
        match self {
            Self::Files { pair, .. } => pair.left.can_reopen() && pair.right.can_reopen(),
            Self::Row { .. } => true,
        }
    }

    /// Whether both sides are present files, as an external diff needs.
    pub(crate) fn are_files(&self) -> bool {
        match self {
            Self::Files { .. } => true,
            Self::Row { row, .. } => !row.is_dir() && row.left.is_some() && row.right.is_some(),
        }
    }

    /// Whether the pair is an ambiguous case collision, which nothing may copy.
    pub(crate) fn is_ambiguous(&self) -> bool {
        match self {
            Self::Files { .. } => false,
            Self::Row { row, .. } => row.is_ambiguous_case_collision,
        }
    }

    /// What a pending confirmation applies to, so an answer is refused when the
    /// selection moved underneath it. A file pair never moves.
    pub(crate) fn subject(&self) -> PathBuf {
        match self {
            Self::Files { pair, .. } => pair.left.path().to_path_buf(),
            Self::Row { row, .. } => row.relative_path.clone(),
        }
    }
}

/// Which way a copy runs. Two-valued, unlike [`ConfirmAction`], so a copy
/// preview has no impossible direction to reject.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyDirection {
    LeftToRight,
    RightToLeft,
}

impl CopyDirection {
    /// The side a copy in this direction reads.
    pub(crate) fn source(self) -> Side {
        match self {
            Self::LeftToRight => Side::Left,
            Self::RightToLeft => Side::Right,
        }
    }

    /// The action a confirmed copy in this direction runs.
    pub(crate) fn confirmed(self) -> ConfirmAction {
        match self {
            Self::LeftToRight => ConfirmAction::CopyLeftToRight,
            Self::RightToLeft => ConfirmAction::CopyRightToLeft,
        }
    }
}

/// What a copy would do to its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    /// Nothing is there yet.
    Create,
    /// Something is there and will be replaced.
    Overwrite,
    /// Both sides are directories; listed entries land in the existing one.
    Merge,
}

/// The facts a copy confirmation is built from, with no wording attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPreview {
    pub kind: CopyKind,
    pub source_name: String,
    pub destination_name: String,
    /// Absolute, lexically normalized; not canonicalized (Issue #235).
    pub source: PathBuf,
    pub destination: PathBuf,
    /// The two sides spell the name differently; the destination keeps its own.
    pub case_mismatch: bool,
}

/// Why [`App::plan_copy`] refuses. Facts only; `commands` words them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRefusal {
    /// No Compared pair: nothing is selected.
    NoSelection,
    AmbiguousCaseCollision,
    /// The source side has nothing: absent, or the null device.
    NothingToCopy,
    /// The destination side cannot be written.
    ReadOnly,
    /// The File Diff holds staged edits that a whole-file copy would discard.
    StagedChangesUnsaved,
    AlreadyIdentical,
}

/// A copy whose preconditions hold: what it copies and where. Built without
/// touching the filesystem, so the Palette's gate can ask for it every frame;
/// the confirmation shows [`App::copy_preview`] of it, and the effect runs
/// exactly it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPlan {
    pub direction: CopyDirection,
    pub target: CopyTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyTarget {
    /// Replace one side of the file pair with the other.
    FilePair,
    /// Copy the selected row's entry from one root to the other.
    Entry {
        relative_path: PathBuf,
        source_name: String,
        destination_name: String,
        source: PathBuf,
        destination: PathBuf,
        /// The destination side's root, which a copy may not escape.
        destination_root: PathBuf,
        /// The two sides spell the name differently; the destination keeps its own.
        case_mismatch: bool,
        source_is_dir: bool,
    },
}

/// An external diff whose preconditions hold: the tool and the two files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPlan {
    pub tool: crate::diff_tool::ExternalDiffTool,
    pub left: PathBuf,
    pub right: PathBuf,
}

/// Why [`App::plan_external_diff`] refuses. Facts only; `commands` words them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRefusal {
    ReadFromPipe,
    NotBothFiles,
    Disabled,
    /// Auto, and none of the supported tools was found at startup.
    NoTool,
    /// A pinned or unknown tool that was not found at startup.
    ToolMissing,
}

impl App {
    /// The Compared pair: the file pair, or the selected row. `None` in a
    /// Directory Tree session with nothing selected.
    pub(crate) fn compared_pair(&self) -> Option<ComparedPair<'_>> {
        match &self.file_pair {
            Some(pair) => Some(ComparedPair::Files {
                pair,
                info: &self.file_pair_info,
            }),
            None => self
                .file_diff
                .opened_row()
                .or_else(|| self.selected_row())
                .map(|row| ComparedPair::Row {
                    row,
                    roots: self.roots.as_ref().map(|root| root.path.as_path()),
                }),
        }
    }

    /// Plan an external diff of the Compared pair with the tool the settings
    /// pick from those detected at startup, or say why it cannot run. The gate
    /// and the launch read the same plan, so a tool the gate offered is the
    /// tool that runs (ADR-0003).
    pub(crate) fn plan_external_diff(&self) -> Result<DiffPlan, DiffRefusal> {
        let pair = self.compared_pair();
        if pair.is_some_and(|pair| !pair.can_reopen()) {
            return Err(DiffRefusal::ReadFromPipe);
        }
        let Some(pair) = pair.filter(|pair| pair.are_files()) else {
            return Err(DiffRefusal::NotBothFiles);
        };
        let Some(tool) = self.resolve_effective_diff_tool() else {
            return Err(match &self.settings.saved().external_diff_tool {
                crate::settings::DiffToolSetting::Disabled => DiffRefusal::Disabled,
                crate::settings::DiffToolSetting::Auto => DiffRefusal::NoTool,
                crate::settings::DiffToolSetting::Pinned(_)
                | crate::settings::DiffToolSetting::Unknown(_) => DiffRefusal::ToolMissing,
            });
        };
        let Pair { left, right } = pair.paths();
        Ok(DiffPlan { tool, left, right })
    }

    /// The file the external editor opens: the focused side of the Compared
    /// pair, when that side is a file.
    pub(crate) fn plan_editor(&self) -> Option<PathBuf> {
        let pair = self.compared_pair()?;
        let side = self.active_side;
        pair.has_file(side).then(|| pair.path(side))
    }

    /// What a pending confirmation applies to, so an answer is refused when the
    /// selection moved underneath it. A file pair never moves.
    pub(crate) fn confirmation_subject(&self) -> Option<PathBuf> {
        self.compared_pair().map(|pair| pair.subject())
    }

    /// Plan a copy in `direction`, or say why it cannot run. The one place a
    /// copy's preconditions live: the gate, the confirmation, and the effect
    /// all read the plan (ADR-0003).
    pub(crate) fn plan_copy(&self, direction: CopyDirection) -> Result<CopyPlan, CopyRefusal> {
        let (source, destination) = (direction.source(), direction.source().other());
        let pair = self.compared_pair().ok_or(CopyRefusal::NoSelection)?;
        if pair.is_ambiguous() {
            return Err(CopyRefusal::AmbiguousCaseCollision);
        }
        if !pair.has_content(source) {
            return Err(CopyRefusal::NothingToCopy);
        }
        if !pair.is_writable(destination) {
            return Err(CopyRefusal::ReadOnly);
        }
        if self.view_mode == ViewMode::FileDiff && self.diff().is_dirty() {
            return Err(CopyRefusal::StagedChangesUnsaved);
        }
        let target = match pair {
            ComparedPair::Files { .. } => {
                if self.diff().hash(Side::Left) == self.diff().hash(Side::Right) {
                    return Err(CopyRefusal::AlreadyIdentical);
                }
                CopyTarget::FilePair
            }
            ComparedPair::Row { row, roots } => {
                // The synthetic root row stands for the whole tree.
                if row.relative_path.as_os_str().is_empty() {
                    return Err(CopyRefusal::NothingToCopy);
                }
                if row.state == crate::diff::DiffState::Identical && !row.has_case_conflict {
                    return Err(CopyRefusal::AlreadyIdentical);
                }
                let (src_name, dst_name) = match source {
                    Side::Left => (row.left_name(), row.right_name()),
                    Side::Right => (row.right_name(), row.left_name()),
                };
                let paths = pair.paths();
                let dst_root = roots.side(destination);
                CopyTarget::Entry {
                    relative_path: row.relative_path.clone(),
                    source_name: src_name.to_string(),
                    destination_name: dst_name.to_string(),
                    source: paths.side(source).clone(),
                    destination: paths.side(destination).clone(),
                    destination_root: dst_root.to_path_buf(),
                    case_mismatch: row.has_case_conflict && src_name != dst_name,
                    source_is_dir: row.side(source).as_ref().is_some_and(|info| info.is_dir),
                }
            }
        };
        Ok(CopyPlan { direction, target })
    }

    /// Describe what `plan` would do to its destination, for the confirmation.
    ///
    /// The facts only — the operation, both absolute paths, and whether the two
    /// sides spell the name differently. `Commands` turns them into the prompt
    /// the user reads (Issue #284). Paths are deliberately not canonicalized:
    /// resolving symlinks would show a different identity from the one the copy
    /// actually writes (Issue #235). Reads the destination's metadata, so it
    /// runs when a copy is requested, never while drawing.
    pub(crate) fn copy_preview(&self, plan: &CopyPlan) -> CopyPreview {
        match &plan.target {
            CopyTarget::FilePair => {
                let pair = self
                    .file_pair
                    .as_ref()
                    .expect("a file-pair plan comes from a file-pair session");
                let source_side = plan.direction.source();
                let (source, destination) =
                    (pair.side(source_side), pair.side(source_side.other()));
                CopyPreview {
                    kind: CopyKind::Overwrite,
                    source_name: source.name(),
                    destination_name: destination.name(),
                    source: crate::write::absolute_lexical(source.path()),
                    destination: crate::write::absolute_lexical(destination.path()),
                    case_mismatch: false,
                }
            }
            CopyTarget::Entry {
                source_name,
                destination_name,
                source,
                destination,
                case_mismatch,
                source_is_dir,
                ..
            } => {
                let src = crate::write::absolute_lexical(source);
                let dst = crate::write::absolute_lexical(destination);
                let dst_meta = std::fs::symlink_metadata(&dst).ok();
                let dst_is_dir = dst_meta
                    .as_ref()
                    .is_some_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink());
                let kind = if dst_meta.is_none() {
                    CopyKind::Create
                } else if *source_is_dir && dst_is_dir {
                    CopyKind::Merge
                } else {
                    CopyKind::Overwrite
                };
                CopyPreview {
                    kind,
                    source_name: source_name.clone(),
                    destination_name: destination_name.clone(),
                    source: src,
                    destination: dst,
                    case_mismatch: *case_mismatch,
                }
            }
        }
    }
}

/// The external diff and editor plans: what each Command would launch on
/// the selected row, or why it refuses.
#[cfg(test)]
mod tests {
    use crate::app::{self, App, FlatRow};
    use crate::diff::{DiffState, FileInfo};
    use crate::diff_tool::ExternalDiffTool;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn file_row(name: &str, left: bool, right: bool, is_dir: bool) -> FlatRow {
        let info = FileInfo {
            is_dir,
            size: 10,
            modified: SystemTime::UNIX_EPOCH,
        };
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from(name),
            name: name.to_string(),
            state: DiffState::DifferentNewerLeft,
            left: left.then_some(info.clone()),
            right: right.then_some(info),
            ..Default::default()
        }
    }

    #[test]
    fn plan_external_diff_refuses_when_disabled() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::Disabled));
    }

    #[test]
    fn plan_external_diff_refuses_a_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("dir", true, true, true)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
    }

    #[test]
    fn plan_external_diff_refuses_a_single_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
    }

    #[test]
    /// The tool list is the one detected at startup, which the gate and the
    /// launch both read, so a pinned tool missing then is refused up front.
    fn plan_external_diff_refuses_a_pinned_tool_missing_at_startup() {
        let _guard = crate::test_support::PathEnvGuard::set("/nonexistent_dir_123");
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Meld,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);

        assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::ToolMissing));
    }

    #[test]
    fn plan_external_diff_builds_paths_for_both_sided_file_when_available() {
        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        #[cfg(windows)]
        let vim_exe = bin_dir.join("vim.exe");
        #[cfg(not(windows))]
        let vim_exe = bin_dir.join("vim");
        std::fs::write(&vim_exe, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&vim_exe).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&vim_exe, perms).unwrap();
        }

        let _guard = crate::test_support::PathEnvGuard::set(&bin_dir);

        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                detected_diff_tools: crate::diff_tool::detect_diff_tools(),
                ..crate::startup::Startup::for_test()
            },
        );
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Ok(app::DiffPlan {
                tool: ExternalDiffTool::Vim,
                left: PathBuf::from("/left/a.txt"),
                right: PathBuf::from("/right/a.txt"),
            })
        );
    }

    #[test]
    fn plan_editor_refuses_a_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_left_pane();
        app.directory_tree_mut()
            .set_rows(vec![file_row("dir", true, false, true)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_editor(), None);
    }

    #[test]
    fn plan_editor_follows_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);

        app.focus_left_pane();
        assert_eq!(app.plan_editor(), Some(PathBuf::from("/left/a.txt")));

        app.focus_right_pane();
        assert_eq!(app.plan_editor(), Some(PathBuf::from("/right/a.txt")));
    }

    #[test]
    fn plan_editor_refuses_a_side_with_no_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_right_pane();
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_editor(), None);
    }

    #[test]
    fn plans_refuse_when_nothing_is_selected() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
        assert_eq!(app.plan_editor(), None);
    }
}
