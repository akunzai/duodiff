//! Pure key-outcome builders: read `App` state and describe an IO intent without
//! performing any IO themselves. `input::handle_key` calls these instead of embedding
//! external-process launches inline; `actions::dispatch_key_outcome` performs the IO.
use crate::app::App;
use crate::diff_tool::ExternalDiffTool;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome {
    /// No IO needed — the key was fully handled by pure state mutation (or ignored).
    None,
    LaunchDiff {
        tool: ExternalDiffTool,
        left: PathBuf,
        right: PathBuf,
    },
    LaunchEditor {
        path: PathBuf,
    },
}

/// Build the diff-launch intent for the currently selected row (the `D` key). Returns
/// `KeyOutcome::None` when there's no comparable file pair or no configured/valid tool.
pub fn diff_launch_outcome(app: &App) -> KeyOutcome {
    let Some(row) = app.selected_row() else {
        return KeyOutcome::None;
    };
    let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
        || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
    if is_dir || row.left.is_none() || row.right.is_none() {
        return KeyOutcome::None;
    }
    let Some(tool_str) = app.settings().external_diff_tool.as_ref() else {
        return KeyOutcome::None;
    };
    let Ok(tool) = ExternalDiffTool::from_str(tool_str) else {
        return KeyOutcome::None;
    };
    KeyOutcome::LaunchDiff {
        tool,
        left: app.left_path().join(&row.relative_path),
        right: app.right_path().join(&row.relative_path),
    }
}

/// Build the editor-launch intent for the active side's selected file (the `E` key).
/// Returns `KeyOutcome::None` when the active side has no file selected (directory, or
/// missing on that side).
pub fn editor_launch_outcome(app: &App) -> KeyOutcome {
    let Some(row) = app.selected_row() else {
        return KeyOutcome::None;
    };
    let file_exists = if app.active_side_left() {
        row.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
    } else {
        row.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
    };
    if !file_exists {
        return KeyOutcome::None;
    }
    let path = if app.active_side_left() {
        app.left_path().join(&row.relative_path)
    } else {
        app.right_path().join(&row.relative_path)
    };
    KeyOutcome::LaunchEditor { path }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::FlatRow;
    use crate::diff::{DiffState, FileInfo};
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
        }
    }

    #[test]
    fn diff_launch_outcome_none_without_configured_tool() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(None);
        app.set_filtered_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_none_for_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.set_filtered_rows(vec![file_row("dir", true, true, true)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_none_for_single_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.set_filtered_rows(vec![file_row("a.txt", true, false, false)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_builds_paths_for_both_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.set_filtered_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(
            diff_launch_outcome(&app),
            KeyOutcome::LaunchDiff {
                tool: ExternalDiffTool::Vim,
                left: PathBuf::from("/left/a.txt"),
                right: PathBuf::from("/right/a.txt"),
            }
        );
    }

    #[test]
    fn editor_launch_outcome_none_for_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_left_pane();
        app.set_filtered_rows(vec![file_row("dir", true, false, true)]);
        app.set_selected_idx(0);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn editor_launch_outcome_follows_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_filtered_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);

        app.focus_left_pane();
        assert_eq!(
            editor_launch_outcome(&app),
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("/left/a.txt"),
            }
        );

        app.focus_right_pane();
        assert_eq!(
            editor_launch_outcome(&app),
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("/right/a.txt"),
            }
        );
    }

    #[test]
    fn editor_launch_outcome_none_when_missing_on_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_right_pane();
        app.set_filtered_rows(vec![file_row("a.txt", true, false, false)]);
        app.set_selected_idx(0);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn outcomes_are_none_when_selection_out_of_range() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }
}
