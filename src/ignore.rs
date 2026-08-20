//! Standard Git-ignore matching for one scan root.
//!
//! Each [`IgnoreMatcher`] is deliberately root-specific: project rules from
//! one comparison side must never hide entries on the other (Issue #237).

use ::ignore::gitignore::{Gitignore, GitignoreBuilder};
use ::ignore::{Match, WalkBuilder};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct IgnoreMatcher {
    root: PathBuf,
    global: Gitignore,
    cli: Gitignore,
    project: ::ignore::IncrementalIgnore,
}

impl IgnoreMatcher {
    pub fn validate_patterns(root: &Path, patterns: &[String]) -> Result<(), String> {
        build_patterns(root, patterns).map(|_| ())
    }
    /// Build the effective matcher for one root. Precedence is global rules,
    /// project ignore files, then CLI patterns; the last matching layer wins.
    pub fn for_root(
        root: PathBuf,
        global_patterns: &[String],
        respect_gitignore: bool,
        cli_patterns: &[String],
    ) -> Result<Self, String> {
        let global = build_patterns(&root, global_patterns)?;
        let cli = build_patterns(&root, cli_patterns)?;
        let mut builder = WalkBuilder::new(&root);
        builder
            .hidden(false)
            .ignore(false)
            .git_ignore(respect_gitignore)
            // These comparison roots need not themselves be Git worktrees.
            // A nearby `.gitignore` is still an explicit project rule.
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .add_custom_ignore_filename(".duodiffignore");
        let project = builder
            .build_matchers()
            .into_iter()
            .next()
            .expect("WalkBuilder has one configured root");
        Ok(Self {
            root,
            global,
            cli,
            project,
        })
    }

    /// Returns true when the effective rules hide this root-relative path.
    /// A project ignore read/parse error is returned so scans retain their last
    /// successful tree instead of silently scanning with incomplete rules.
    pub fn is_ignored(&mut self, relative_path: &Path, is_dir: bool) -> std::io::Result<bool> {
        let mut result = match_result(&self.global, &self.root, relative_path, is_dir);
        let (project, error) = self.project.matched_with_errors(relative_path, is_dir);
        if let Some(error) = error {
            return Err(std::io::Error::other(error.to_string()));
        }
        if !project.is_none() {
            result = Some(project.is_ignore());
        }
        if let Some(cli) = match_result(&self.cli, &self.root, relative_path, is_dir) {
            result = Some(cli);
        }
        Ok(result.unwrap_or(false))
    }
}

impl Default for IgnoreMatcher {
    fn default() -> Self {
        Self::for_root(std::env::current_dir().unwrap_or_default(), &[], true, &[])
            .expect("empty ignore matcher is valid")
    }
}

fn build_patterns(root: &Path, patterns: &[String]) -> Result<Gitignore, String> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        builder
            .add_line(None, pattern)
            .map_err(|error| format!("invalid exclusion pattern `{pattern}`: {error}"))?;
    }
    builder.build().map_err(|error| error.to_string())
}

fn match_result(matcher: &Gitignore, root: &Path, relative: &Path, is_dir: bool) -> Option<bool> {
    match matcher.matched_path_or_any_parents(root.join(relative), is_dir) {
        Match::Ignore(_) => Some(true),
        Match::Whitelist(_) => Some(false),
        Match::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn matcher(root: &Path, global: &[&str], gitignore: bool, cli: &[&str]) -> IgnoreMatcher {
        IgnoreMatcher::for_root(
            root.to_path_buf(),
            &global.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            gitignore,
            &cli.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn standard_gitignore_features_and_nested_rules_apply() {
        let root = tempdir().unwrap();
        fs::write(root.path().join(".gitignore"), "*.tmp\ncache/\n[ab].log\n").unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(
            root.path().join("nested/.gitignore"),
            "private/\n!keep.tmp\n",
        )
        .unwrap();
        let mut matcher = matcher(root.path(), &[], true, &[]);

        assert!(matcher.is_ignored(Path::new("a.tmp"), false).unwrap());
        assert!(matcher.is_ignored(Path::new("cache/x"), false).unwrap());
        assert!(matcher.is_ignored(Path::new("a.log"), false).unwrap());
        assert!(matcher
            .is_ignored(Path::new("nested/private/x"), false)
            .unwrap());
        assert!(!matcher
            .is_ignored(Path::new("nested/keep.tmp"), false)
            .unwrap());
    }

    #[test]
    fn precedence_and_gitignore_switch_are_respected() {
        let root = tempdir().unwrap();
        fs::write(
            root.path().join(".gitignore"),
            "!visible.log\nproject-only\n",
        )
        .unwrap();
        fs::write(root.path().join(".duodiffignore"), "duodiff-only\n").unwrap();
        let mut enabled = matcher(root.path(), &["*.log"], true, &["!cli.log"]);
        assert!(!enabled.is_ignored(Path::new("visible.log"), false).unwrap());
        assert!(!enabled.is_ignored(Path::new("cli.log"), false).unwrap());
        assert!(enabled
            .is_ignored(Path::new("project-only"), false)
            .unwrap());
        assert!(enabled
            .is_ignored(Path::new("duodiff-only"), false)
            .unwrap());

        let mut disabled = matcher(root.path(), &["*.log"], false, &[]);
        assert!(disabled
            .is_ignored(Path::new("visible.log"), false)
            .unwrap());
        assert!(!disabled
            .is_ignored(Path::new("project-only"), false)
            .unwrap());
        assert!(disabled
            .is_ignored(Path::new("duodiff-only"), false)
            .unwrap());
    }

    #[test]
    fn roots_are_isolated() {
        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        fs::write(left.path().join(".gitignore"), "left-only\n").unwrap();
        let mut left_matcher = matcher(left.path(), &[], true, &[]);
        let mut right_matcher = matcher(right.path(), &[], true, &[]);
        assert!(left_matcher
            .is_ignored(Path::new("left-only"), false)
            .unwrap());
        assert!(!right_matcher
            .is_ignored(Path::new("left-only"), false)
            .unwrap());
    }
}
