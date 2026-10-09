//! A Command's confirmation, whole: putting the question on screen with the
//! approval its answer must match, re-checking that approval when the answer
//! arrives, and running the work it approved (ADR-0003).
//!
//! The modal lives on `App`, because views assemble from `&App` (ADR-0001),
//! but only this module shows or closes it, so the dialog on screen and the
//! approval it waits for never disagree. Each effect reports its canonical
//! [`Outcome`] itself; writing to disk goes through `write`.
use super::{copy_prompt, save_conflict_prompt, Commands, Outcome};
use crate::app::{self, App};

/// The approval a confirm dialog is waiting for, which its answer must still
/// match when it arrives (Issue #282).
#[derive(Debug)]
pub(super) enum Pending {
    /// An action on the entry the question was asked about, if any.
    Subject(Option<std::path::PathBuf>),
    /// A copy, which runs only while planning it again still gives this same
    /// plan.
    Copy(app::CopyPlan),
}

impl Commands {
    /// Ask before doing anything, remembering the entry the answer will apply
    /// to so a selection that moves in the meantime cannot be acted on.
    ///
    /// A save conflict asks on top of the approval that reached it, so the
    /// pending subject simply follows whichever question is now waiting.
    pub(super) fn confirm(&mut self, app: &mut App, prompt: app::ConfirmModal) -> Outcome {
        let subject = app.confirmation_subject();
        self.ask(app, prompt, Pending::Subject(subject))
    }

    /// Ask about the copy the gate planned, keeping the plan the answer must
    /// still match.
    pub(super) fn request_copy(&mut self, app: &mut App, plan: app::CopyPlan) -> Outcome {
        let prompt = copy_prompt(&app.copy_preview(&plan), plan.direction);
        self.ask(app, prompt, Pending::Copy(plan))
    }

    /// Put the question on screen together with what its answer must match,
    /// so every adapter raises a confirmation the same way (Issue #284).
    /// Nothing has run yet, so the Command itself is complete.
    fn ask(&mut self, app: &mut App, prompt: app::ConfirmModal, pending: Pending) -> Outcome {
        app.show_confirm(prompt);
        self.pending = Some(pending);
        Outcome::Completed
    }

    /// Carry out the work a confirm dialog approved.
    ///
    /// The approval names one entry, so it is refused rather than redirected
    /// when the selection moved underneath it (Issue #282).
    pub(super) fn answer_confirmation(
        &mut self,
        app: &mut App,
        action: app::ConfirmAction,
    ) -> Outcome {
        // Whatever the answer, the question is settled: the dialog closes and
        // its approval is spent.
        let pending = self.pending.take();
        app.dismiss_confirm();
        let direction = match action {
            app::ConfirmAction::CopyLeftToRight => Some(app::CopyDirection::LeftToRight),
            app::ConfirmAction::CopyRightToLeft => Some(app::CopyDirection::RightToLeft),
            _ => None,
        };
        let approved = match direction {
            // The copy runs only as the plan the user saw: planning it again
            // must give the same source, destination, and direction.
            Some(direction) => match pending {
                Some(Pending::Copy(plan)) => app.plan_copy(direction).ok() == Some(plan),
                _ => false,
            },
            None => {
                matches!(action, app::ConfirmAction::Cancel)
                    || matches!(pending, Some(Pending::Subject(Some(target)))
                        if app.confirmation_subject().as_ref() == Some(&target))
            }
        };
        if !approved {
            // The dialog is already closed, as it must be: left open it would
            // trap the user, since its approval can never be answered now.
            // A copy's plan can change without the selection moving — a rescan
            // made the sides identical, or edits were staged — so its refusal
            // names neither.
            let message = if direction.is_some() {
                "Nothing was copied — what you confirmed no longer applies"
            } else {
                "The confirmed entry is no longer selected — nothing was changed"
            };
            return Outcome::Unavailable {
                message: message.to_string(),
            };
        }
        match direction.and_then(|direction| app.plan_copy(direction).ok()) {
            Some(plan) => copy_planned(app, &plan),
            None => self.run_approved(app, action),
        }
    }

    /// Run the work a confirmed dialog approved, other than a copy, which runs
    /// the plan the answer matched through [`copy_planned`].
    fn run_approved(&mut self, app: &mut App, action: app::ConfirmAction) -> Outcome {
        match action {
            app::ConfirmAction::SaveStaged => self.save_staged(app, false),
            app::ConfirmAction::SaveStagedThenLeave => self.save_staged(app, true),
            app::ConfirmAction::DiscardStagedThenLeave => {
                app.discard_staged();
                app.leave_file_diff();
                Outcome::Completed
            }
            app::ConfirmAction::ReloadDiscardStaged => {
                match app.reload_file_diff(Some("Reloaded from disk; staged changes discarded")) {
                    Ok(()) => Outcome::Completed,
                    Err(error) => reload_failed(error),
                }
            }
            app::ConfirmAction::Cancel
            | app::ConfirmAction::CopyLeftToRight
            | app::ConfirmAction::CopyRightToLeft => Outcome::Completed,
        }
    }

    /// Write the staged buffers, then rescan the tree so the row states follow.
    ///
    /// A conflict writes nothing and asks again, naming the paths that moved:
    /// it is the one effect that answers with another question. `then_leave`
    /// only returns to the tree once the write actually succeeded (Issue #235).
    fn save_staged(&mut self, app: &mut App, then_leave: bool) -> Outcome {
        match app.save_staged() {
            Ok(app::StagedSave::Written) => {
                if then_leave {
                    app.leave_file_diff();
                }
                app.request_rescan();
                Outcome::Message {
                    text: "Saved staged changes".to_string(),
                }
            }
            Ok(app::StagedSave::Conflicted(paths)) => {
                self.confirm(app, save_conflict_prompt(&paths))
            }
            Err(error) => Outcome::Failed {
                message: format!("Save failed: {error}"),
            },
        }
    }
}

fn reload_failed(error: String) -> Outcome {
    Outcome::Failed {
        message: format!("Reload failed: {error}"),
    }
}

fn copy_failed(error: impl std::fmt::Display) -> Outcome {
    Outcome::Failed {
        message: format!("Copy failed: {error}"),
    }
}

fn copied(name: &str) -> Outcome {
    Outcome::Message {
        text: format!("Copied '{name}'"),
    }
}

/// Run a copy the user confirmed, exactly as planned: the plan already holds
/// every precondition, so nothing here checks one again (ADR-0003).
fn copy_planned(app: &mut App, plan: &app::CopyPlan) -> Outcome {
    let app::CopyTarget::Entry {
        relative_path,
        source_name: name,
        source: src,
        destination: dst,
        destination_root: dst_root,
        ..
    } = &plan.target
    else {
        return copy_within_file_pair(app, plan.direction);
    };
    let (relative_path, name, src, dst, dst_root) = (
        relative_path.clone(),
        name.clone(),
        src.clone(),
        dst.clone(),
        dst_root.clone(),
    );

    // Directory copies walk the scan model, not the filesystem, so excluded
    // entries (`.git`, …) and files that appeared after the scan are never
    // copied implicitly (Issue #235).
    let res = match app.scanned_subtree_entries(&relative_path, plan.direction.source()) {
        Some(entries) => crate::write::copy_scanned_subtree(&src, &dst, &dst_root, &entries),
        None => crate::write::copy_entry_checked(&src, &dst, &dst_root),
    };

    match res {
        Ok(()) => {
            app.leave_file_diff();
            let copied_is_dir = std::fs::symlink_metadata(&dst)
                .map(|m| {
                    let ft = m.file_type();
                    ft.is_dir() && !ft.is_symlink()
                })
                .unwrap_or(false);
            app.request_subtree_rescan(&relative_path, copied_is_dir);
            copied(&name)
        }
        Err(e) => copy_failed(e),
    }
}

/// Replace one side of a file pair with the other and reload the pair in the
/// background, staying on File Diff: there is no tree to return to (Issue #327).
///
/// A side read from a pipe cannot be read again, so its captured bytes are
/// written instead of copying the path.
fn copy_within_file_pair(app: &mut App, direction: app::CopyDirection) -> Outcome {
    let Some(pair) = app.file_pair() else {
        // A file-pair plan is only ever built in a file-pair session.
        return Outcome::Completed;
    };
    let source = direction.source();
    let (source, destination) = (pair.side(source), pair.side(source.other()));
    let name = source.name();
    let written = match source.captured_bytes() {
        Some(bytes) => std::fs::write(destination.target_path(), bytes),
        None => std::fs::copy(source.target_path(), destination.target_path()).map(|_| ()),
    };
    if let Err(e) = written {
        return copy_failed(e);
    }
    match app.reload_file_diff(None) {
        Ok(()) => copied(&name),
        Err(e) => reload_failed(e),
    }
}
