use std::fs;
use std::path::Path;

/// A simple glob-based ignore matcher.
///
/// Supports a subset of `.gitignore` semantics sufficient for common
/// exclusion patterns such as `.git`, `node_modules`, `target`, `dist`,
/// or `*.log`.
///
/// Supported features:
/// - `*` matches any sequence of characters except `/`.
/// - `?` matches any single character except `/`.
/// - A trailing `/` restricts the pattern to directories.
/// - A pattern containing `/` is anchored to the scan root.
/// - Patterns without `/` match any path component at any depth.
/// - Lines starting with `#` are treated as comments and ignored.
/// - Lines starting with `!` are negations (re-include previously ignored
///   paths).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IgnoreMatcher {
    patterns: Vec<IgnorePattern>,
}

#[derive(Debug, Clone, PartialEq)]
struct IgnorePattern {
    raw: String,
    is_dir_only: bool,
    is_anchored: bool,
    negated: bool,
    segments: Vec<String>,
}

impl IgnoreMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a single glob pattern. Empty lines and comments are skipped.
    pub fn add_pattern(&mut self, pattern: &str) {
        let trimmed = pattern.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return;
        }

        let negated = trimmed.starts_with('!');
        let content = if negated { &trimmed[1..] } else { trimmed };

        let is_dir_only = content.ends_with('/');
        let content = content.trim_end_matches('/');

        // Strip a leading slash to normalise anchored patterns.
        let content = content.strip_prefix('/').unwrap_or(content);

        if content.is_empty() {
            return;
        }

        let is_anchored = content.contains('/');
        let segments: Vec<String> = content.split('/').map(String::from).collect();

        self.patterns.push(IgnorePattern {
            raw: trimmed.to_string(),
            is_dir_only,
            is_anchored,
            negated,
            segments,
        });
    }

    pub fn add_patterns(&mut self, patterns: &[String]) {
        for p in patterns {
            self.add_pattern(p);
        }
    }

    /// Load patterns from `.duodiffignore` if it exists, otherwise fall back
    /// to `.gitignore`.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let duodiff_ignore = dir.join(".duodiffignore");
        let git_ignore = dir.join(".gitignore");
        if duodiff_ignore.is_file() {
            self.load_from_file(&duodiff_ignore);
        } else if git_ignore.is_file() {
            self.load_from_file(&git_ignore);
        }
    }

    fn load_from_file(&mut self, path: &Path) {
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                self.add_pattern(line);
            }
        }
    }

    /// Returns `true` if the given relative path should be skipped.
    /// `is_dir` indicates whether the path is a directory.
    pub fn is_ignored(&self, relative_path: &Path, is_dir: bool) -> bool {
        let mut ignored = false;

        for pattern in &self.patterns {
            if pattern.negated && !ignored {
                // Negations only matter when the path is currently ignored.
                continue;
            }

            if self.matches_pattern(pattern, relative_path, is_dir) {
                ignored = !pattern.negated;
            }
        }

        ignored
    }

    fn matches_pattern(&self, pattern: &IgnorePattern, relative_path: &Path, is_dir: bool) -> bool {
        if pattern.is_dir_only && !is_dir {
            return false;
        }

        if pattern.is_anchored {
            return self.matches_anchored(relative_path, &pattern.segments);
        }

        // Unanchored: match the pattern against any path component.
        for component in relative_path.components() {
            let component_str = component.as_os_str().to_string_lossy();
            if self.glob_match(&pattern.segments[0], &component_str) {
                return true;
            }
        }

        false
    }

    /// Anchored patterns must match consecutive segments starting at the root.
    /// Uses `Path::components` so Windows `\` and Unix `/` both work.
    fn matches_anchored(&self, relative_path: &Path, segments: &[String]) -> bool {
        let parts: Vec<String> = relative_path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if segments.len() > parts.len() {
            return false;
        }

        for (pat_seg, path_seg) in segments.iter().zip(parts.iter()) {
            if !self.glob_match(pat_seg, path_seg) {
                return false;
            }
        }

        true
    }

    /// Simple glob matching supporting `*` and `?`.
    fn glob_match(&self, pattern: &str, text: &str) -> bool {
        let pat: Vec<char> = pattern.chars().collect();
        let txt: Vec<char> = text.chars().collect();
        let mut dp = vec![vec![false; txt.len() + 1]; pat.len() + 1];
        dp[0][0] = true;

        for i in 1..=pat.len() {
            if pat[i - 1] == '*' {
                dp[i][0] = dp[i - 1][0];
            }
        }

        for i in 1..=pat.len() {
            for j in 1..=txt.len() {
                match pat[i - 1] {
                    '*' => dp[i][j] = dp[i - 1][j] || dp[i][j - 1],
                    '?' => dp[i][j] = dp[i - 1][j - 1],
                    c => dp[i][j] = dp[i - 1][j - 1] && c == txt[j - 1],
                }
            }
        }

        dp[pat.len()][txt.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_matcher_ignores_nothing() {
        let matcher = IgnoreMatcher::new();
        assert!(!matcher.is_ignored(Path::new("node_modules"), true));
        assert!(!matcher.is_ignored(Path::new(".git"), true));
    }

    #[test]
    fn test_simple_name_pattern() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("node_modules");
        assert!(matcher.is_ignored(Path::new("node_modules"), true));
        assert!(matcher.is_ignored(Path::new("node_modules"), false));
        assert!(matcher.is_ignored(Path::new("foo/node_modules"), true));
        assert!(!matcher.is_ignored(Path::new("node_modules_info"), true));
    }

    #[test]
    fn test_directory_only_pattern() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("target/");
        assert!(matcher.is_ignored(Path::new("target"), true));
        assert!(!matcher.is_ignored(Path::new("target"), false));
    }

    #[test]
    fn test_glob_star_pattern() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("*.log");
        assert!(matcher.is_ignored(Path::new("debug.log"), false));
        assert!(matcher.is_ignored(Path::new("foo/bar.log"), false));
        assert!(!matcher.is_ignored(Path::new("debug.log.old"), false));
    }

    #[test]
    fn test_anchored_pattern() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("src/generated");
        assert!(matcher.is_ignored(Path::new("src/generated"), true));
        assert!(!matcher.is_ignored(Path::new("other/src/generated"), true));
    }

    #[test]
    fn test_anchored_pattern_uses_path_components() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("src/generated");
        // Build with join so components are separator-agnostic on every OS.
        // (Previously matching split on '/' against to_string_lossy(), which
        // broke when the display form used '\'.)
        let nested = std::path::PathBuf::from("src").join("generated");
        let nested_file = nested.join("file.rs");
        let other = std::path::PathBuf::from("other")
            .join("src")
            .join("generated");
        assert!(matcher.is_ignored(&nested, true));
        assert!(matcher.is_ignored(&nested_file, false));
        assert!(!matcher.is_ignored(&other, true));
    }

    #[cfg(windows)]
    #[test]
    fn test_anchored_pattern_backslash_string_on_windows() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("src/generated");
        assert!(matcher.is_ignored(Path::new(r"src\generated"), true));
        assert!(!matcher.is_ignored(Path::new(r"other\src\generated"), true));
    }

    #[test]
    fn test_negation_pattern() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("*.log");
        matcher.add_pattern("!important.log");
        assert!(matcher.is_ignored(Path::new("debug.log"), false));
        assert!(!matcher.is_ignored(Path::new("important.log"), false));
    }

    #[test]
    fn test_comments_and_empty_lines_are_skipped() {
        let mut matcher = IgnoreMatcher::new();
        matcher.add_pattern("");
        matcher.add_pattern("# comment");
        matcher.add_pattern("dist");
        assert_eq!(matcher.patterns.len(), 1);
    }

    #[test]
    fn test_load_from_file() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(".duodiffignore"),
            "# ignored\nnode_modules\n*.tmp\n",
        )
        .unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.load_from_dir(temp.path());

        assert!(matcher.is_ignored(Path::new("node_modules"), true));
        assert!(matcher.is_ignored(Path::new("foo.tmp"), false));
        assert!(!matcher.is_ignored(Path::new("foo.txt"), false));
    }

    #[test]
    fn test_load_from_dir_prefers_duodiffignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".duodiffignore"), "duodiff_only\n").unwrap();
        fs::write(temp.path().join(".gitignore"), "git_only\n").unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.load_from_dir(temp.path());

        assert!(matcher.is_ignored(Path::new("duodiff_only"), true));
        assert!(!matcher.is_ignored(Path::new("git_only"), true));
    }

    #[test]
    fn test_load_from_dir_falls_back_to_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".gitignore"), "git_only\n").unwrap();

        let mut matcher = IgnoreMatcher::new();
        matcher.load_from_dir(temp.path());

        assert!(matcher.is_ignored(Path::new("git_only"), true));
    }
}
