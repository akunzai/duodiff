//! What a session compares — two directory trees or one file pair — resolved
//! from the command-line arguments before the terminal is touched.

use crate::diff_view::{LoadedText, MAX_DIFF_FILE_BYTES};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The two things a session compares.
#[derive(Debug)]
pub enum ComparisonTarget {
    Directories { left: PathBuf, right: PathBuf },
    Files(FilePair),
}

/// Both sides of a direct file comparison.
#[derive(Debug)]
pub struct FilePair {
    pub left: FileSide,
    pub right: FileSide,
}

impl FilePair {
    /// The `left` (or right) side.
    pub fn side(&self, left: bool) -> &FileSide {
        if left {
            &self.left
        } else {
            &self.right
        }
    }
}

/// One side of a direct file comparison.
#[derive(Debug)]
pub struct FileSide {
    path: PathBuf,
    /// Where writes, external tools, and the editor go. See
    /// [`FileSide::target_path`].
    target: PathBuf,
    source: Source,
    writable: bool,
}

/// Where a side's bytes come from.
#[derive(Debug)]
enum Source {
    /// A regular file, re-read on every load.
    Disk,
    /// `/dev/null` (or `NUL` on Windows): always empty. `git difftool` passes
    /// the literal `/dev/null` for the missing side of an added or deleted file
    /// on every platform — see `prepare_temp_file` in
    /// https://github.com/git/git/blob/master/diff.c — and a native Windows
    /// build cannot open that path, so it is recognized by name.
    NullDevice,
    /// A pipe or other non-regular file, read once at startup because it
    /// cannot be read again.
    Captured(Vec<u8>),
}

impl FileSide {
    /// The path as the user typed it, or the directory joined with the other
    /// side's file name.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file name shown in confirmations and toasts.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Whether staging, saving, or copying may write this side.
    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// The path saves, copies, external tools, and the editor use: a symlink
    /// resolved to its file, so a save's temp-file rename replaces the file
    /// rather than the link, and the null device under this platform's name.
    pub fn target_path(&self) -> &Path {
        &self.target
    }

    /// Whether an external tool can open this side again by
    /// [`FileSide::target_path`]; a pipe was consumed at startup.
    pub fn can_reopen(&self) -> bool {
        !matches!(self.source, Source::Captured(_))
    }

    /// Whether this side is the null device, which has nothing to copy.
    pub fn is_null_device(&self) -> bool {
        matches!(self.source, Source::NullDevice)
    }

    /// Whether this side is a regular file an editor can open.
    pub fn is_regular_file(&self) -> bool {
        matches!(self.source, Source::Disk)
    }

    /// The bytes a copy from this side writes, when they cannot be re-read
    /// from [`FileSide::path`].
    pub fn captured_bytes(&self) -> Option<&[u8]> {
        match &self.source {
            Source::Disk => None,
            Source::NullDevice => Some(&[]),
            Source::Captured(bytes) => Some(bytes),
        }
    }

    /// Size and modification time for the pane title and info bar; `None` for
    /// a side that is not a regular file.
    pub fn info(&self) -> Option<crate::diff::FileInfo> {
        if !self.is_regular_file() {
            return None;
        }
        let meta = std::fs::metadata(&self.path).ok()?;
        Some(crate::diff::FileInfo {
            is_dir: false,
            size: meta.len(),
            modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        })
    }

    /// Load this side for the built-in diff. The error is the cause alone; the
    /// caller names the path.
    pub fn load(&self) -> Result<LoadedText, String> {
        let bytes = match &self.source {
            Source::NullDevice => return Ok(LoadedText::default()),
            Source::Captured(bytes) => bytes.clone(),
            Source::Disk => {
                let len = std::fs::metadata(&self.path)
                    .map_err(|e| e.to_string())?
                    .len();
                if len > MAX_DIFF_FILE_BYTES {
                    return Err(crate::diff_view::TextRejection::TooLarge(Some(len)).to_string());
                }
                std::fs::read(&self.path).map_err(|e| e.to_string())?
            }
        };
        LoadedText::from_bytes(bytes).map_err(|rejection| rejection.to_string())
    }
}

/// Why the arguments cannot be compared.
#[derive(Debug)]
pub enum StartupError {
    /// Nothing exists at this path.
    Missing(PathBuf),
    /// The file a directory argument was expected to hold is itself a directory.
    NotAFile(PathBuf),
    /// A directory paired with something that has no file name to look up in it.
    DirectoryWithoutFile { directory: PathBuf, other: PathBuf },
    /// The path exists but could not be read or shown in the built-in diff.
    Unreadable { path: PathBuf, cause: String },
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (what, cause) = match self {
            Self::Missing(path) => (
                format!("Cannot open '{}'", path.display()),
                "the path does not exist".to_string(),
            ),
            Self::NotAFile(path) => (
                format!("Cannot compare '{}'", path.display()),
                "it is a directory, and the other argument is a file".to_string(),
            ),
            Self::DirectoryWithoutFile { directory, other } => (
                format!(
                    "Cannot compare '{}' with '{}'",
                    other.display(),
                    directory.display()
                ),
                "a directory compares only with a regular file or another directory".to_string(),
            ),
            Self::Unreadable { path, cause } => {
                (format!("Cannot open '{}'", path.display()), cause.clone())
            }
        };
        write!(f, "Error: {what}\nCause: {cause}")
    }
}

/// What one argument names.
enum ArgKind {
    Directory,
    File,
    NullDevice,
    /// A pipe, device, or anything else that is neither a file nor a directory.
    Stream,
}

fn is_null_device(path: &Path) -> bool {
    path == Path::new("/dev/null")
        || (cfg!(windows) && path.as_os_str().eq_ignore_ascii_case("NUL"))
}

fn classify(path: &Path) -> Result<ArgKind, StartupError> {
    if is_null_device(path) {
        return Ok(ArgKind::NullDevice);
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(ArgKind::Directory),
        Ok(meta) if meta.is_file() => Ok(ArgKind::File),
        Ok(_) => Ok(ArgKind::Stream),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(StartupError::Missing(path.to_path_buf()))
        }
        Err(error) => Err(StartupError::Unreadable {
            path: path.to_path_buf(),
            cause: error.to_string(),
        }),
    }
}

/// The file inside `directory` that shares `file`'s name, the way `diff` pairs
/// a file with a directory.
fn file_in_directory(directory: &Path, file: &Path) -> Result<PathBuf, StartupError> {
    let joined = match file.file_name() {
        Some(name) => directory.join(name),
        None => directory.to_path_buf(),
    };
    match classify(&joined)? {
        ArgKind::Directory => Err(StartupError::NotAFile(joined)),
        _ => Ok(joined),
    }
}

fn side(path: PathBuf) -> Result<FileSide, StartupError> {
    let unreadable = |cause: String| StartupError::Unreadable {
        path: path.clone(),
        cause,
    };
    let (source, writable, target) = match classify(&path)? {
        ArgKind::NullDevice => {
            let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
            (Source::NullDevice, false, PathBuf::from(null))
        }
        ArgKind::Stream => {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .and_then(|file| file.take(MAX_DIFF_FILE_BYTES + 1).read_to_end(&mut bytes))
                .map_err(|e| unreadable(e.to_string()))?;
            if bytes.len() as u64 > MAX_DIFF_FILE_BYTES {
                return Err(unreadable(
                    crate::diff_view::TextRejection::TooLarge(None).to_string(),
                ));
            }
            (Source::Captured(bytes), false, path.clone())
        }
        ArgKind::File => {
            let writable = std::fs::OpenOptions::new().write(true).open(&path).is_ok();
            let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            (Source::Disk, writable, target)
        }
        ArgKind::Directory => return Err(StartupError::NotAFile(path)),
    };
    let side = FileSide {
        path: path.clone(),
        target,
        source,
        writable,
    };
    side.load().map_err(unreadable)?;
    Ok(side)
}

/// Turn the two command-line paths into what the session compares.
///
/// Both file sides are loaded once here, so content the built-in diff cannot
/// show is reported before the terminal is taken over.
pub fn resolve(left: &Path, right: &Path) -> Result<ComparisonTarget, StartupError> {
    let (left, right) = match (classify(left)?, classify(right)?) {
        (ArgKind::Directory, ArgKind::Directory) => {
            return Ok(ComparisonTarget::Directories {
                left: left.to_path_buf(),
                right: right.to_path_buf(),
            });
        }
        (ArgKind::Directory, ArgKind::File) => {
            (file_in_directory(left, right)?, right.to_path_buf())
        }
        (ArgKind::File, ArgKind::Directory) => {
            (left.to_path_buf(), file_in_directory(right, left)?)
        }
        (ArgKind::Directory, _) => {
            return Err(StartupError::DirectoryWithoutFile {
                directory: left.to_path_buf(),
                other: right.to_path_buf(),
            });
        }
        (_, ArgKind::Directory) => {
            return Err(StartupError::DirectoryWithoutFile {
                directory: right.to_path_buf(),
                other: left.to_path_buf(),
            });
        }
        _ => (left.to_path_buf(), right.to_path_buf()),
    };
    Ok(ComparisonTarget::Files(FilePair {
        left: side(left)?,
        right: side(right)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn files(target: ComparisonTarget) -> FilePair {
        match target {
            ComparisonTarget::Files(pair) => pair,
            other => panic!("expected a file pair, got {other:?}"),
        }
    }

    #[test]
    fn two_files_resolve_to_a_file_pair() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("a.txt");
        let right = dir.path().join("b.txt");
        fs::write(&left, "a\n").unwrap();
        fs::write(&right, "b\n").unwrap();

        let pair = files(resolve(&left, &right).unwrap());

        assert_eq!(pair.left.path(), left);
        assert_eq!(pair.right.path(), right);
    }

    #[test]
    fn two_directories_resolve_to_directories() {
        let left = tempdir().unwrap();
        let right = tempdir().unwrap();

        match resolve(left.path(), right.path()).unwrap() {
            ComparisonTarget::Directories { left: l, right: r } => {
                assert_eq!(l, left.path());
                assert_eq!(r, right.path());
            }
            other => panic!("expected directories, got {other:?}"),
        }
    }

    #[test]
    fn a_file_and_a_directory_compare_with_the_same_named_file_in_either_order() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        let other = dir.path().join("other");
        fs::create_dir(&other).unwrap();
        fs::write(&file, "a\n").unwrap();
        fs::write(other.join("notes.txt"), "b\n").unwrap();

        let pair = files(resolve(&file, &other).unwrap());
        assert_eq!(pair.left.path(), file);
        assert_eq!(pair.right.path(), other.join("notes.txt"));

        let pair = files(resolve(&other, &file).unwrap());
        assert_eq!(pair.left.path(), other.join("notes.txt"));
        assert_eq!(pair.right.path(), file);
    }

    #[test]
    fn a_directory_without_the_same_named_file_names_the_missing_path() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        let other = dir.path().join("other");
        fs::create_dir(&other).unwrap();
        fs::write(&file, "a\n").unwrap();

        let error = resolve(&file, &other).unwrap_err().to_string();

        assert_eq!(
            error,
            format!(
                "Error: Cannot open '{}'\nCause: the path does not exist",
                other.join("notes.txt").display()
            )
        );
    }

    #[test]
    fn a_same_named_directory_is_not_a_file_to_compare() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        let other = dir.path().join("other");
        fs::create_dir_all(other.join("notes.txt")).unwrap();
        fs::write(&file, "a\n").unwrap();

        let error = resolve(&file, &other).unwrap_err().to_string();

        assert!(
            error.starts_with(&format!(
                "Error: Cannot compare '{}'",
                other.join("notes.txt").display()
            )),
            "{error}"
        );
    }

    #[test]
    fn a_missing_argument_is_named_in_the_error() {
        let dir = tempdir().unwrap();
        let present = dir.path().join("a.txt");
        let missing = dir.path().join("nope.txt");
        fs::write(&present, "a\n").unwrap();

        let error = resolve(&present, &missing).unwrap_err().to_string();

        assert_eq!(
            error,
            format!(
                "Error: Cannot open '{}'\nCause: the path does not exist",
                missing.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_argument_is_followed_to_its_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("real.txt");
        let link = dir.path().join("link.txt");
        let right = dir.path().join("b.txt");
        fs::write(&target, "a\n").unwrap();
        fs::write(&right, "b\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let pair = files(resolve(&link, &right).unwrap());

        assert_eq!(pair.left.path(), link);
        assert_eq!(pair.left.load().unwrap().text, "a\n");
    }

    #[test]
    fn the_null_device_is_an_empty_read_only_side() {
        let dir = tempdir().unwrap();
        let right = dir.path().join("added.txt");
        fs::write(&right, "new\n").unwrap();

        let pair = files(resolve(Path::new("/dev/null"), &right).unwrap());

        assert_eq!(pair.left.path(), Path::new("/dev/null"));
        assert_eq!(pair.left.load().unwrap().text, "");
        assert!(!pair.left.is_writable());
        assert!(pair.left.can_reopen());
        assert!(pair.right.is_writable());
    }

    #[cfg(windows)]
    #[test]
    fn nul_is_the_null_device_on_windows() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("deleted.txt");
        fs::write(&left, "old\n").unwrap();

        let pair = files(resolve(&left, Path::new("NUL")).unwrap());

        assert_eq!(pair.right.load().unwrap().text, "");
        assert!(!pair.right.is_writable());
    }

    #[test]
    fn a_directory_paired_with_the_null_device_is_refused() {
        let dir = tempdir().unwrap();

        let error = resolve(dir.path(), Path::new("/dev/null"))
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            format!(
                "Error: Cannot compare '/dev/null' with '{}'\n\
                 Cause: a directory compares only with a regular file or another directory",
                dir.path().display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_is_read_once_into_a_read_only_side() {
        let dir = tempdir().unwrap();
        let left = crate::test_support::fifo_with(dir.path(), "left", "from a pipe\n");
        let right = dir.path().join("b.txt");
        fs::write(&right, "b\n").unwrap();

        let pair = files(resolve(&left, &right).unwrap());

        assert_eq!(pair.left.load().unwrap().text, "from a pipe\n");
        assert_eq!(pair.left.load().unwrap().text, "from a pipe\n");
        assert!(!pair.left.is_writable());
        assert!(!pair.left.can_reopen());
        assert_eq!(pair.left.captured_bytes(), Some(&b"from a pipe\n"[..]));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_paired_with_a_pipe_is_refused() {
        let dir = tempdir().unwrap();
        let other = dir.path().join("tree");
        fs::create_dir(&other).unwrap();
        let fifo = dir.path().join("63");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());

        let error = resolve(&other, &fifo).unwrap_err();

        assert!(matches!(error, StartupError::DirectoryWithoutFile { .. }));
    }

    #[test]
    fn binary_content_is_refused_naming_the_path() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("image.bin");
        let right = dir.path().join("b.txt");
        fs::write(&left, [0u8, 1, 2]).unwrap();
        fs::write(&right, "b\n").unwrap();

        let error = resolve(&left, &right).unwrap_err().to_string();

        assert_eq!(
            error,
            format!(
                "Error: Cannot open '{}'\nCause: binary file not supported",
                left.display()
            )
        );
    }

    #[test]
    fn non_utf8_content_is_refused_naming_the_path() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("a.txt");
        let right = dir.path().join("latin1.txt");
        fs::write(&left, "a\n").unwrap();
        fs::write(&right, [0xe9u8, b'\n']).unwrap();

        let error = resolve(&left, &right).unwrap_err().to_string();

        assert_eq!(
            error,
            format!(
                "Error: Cannot open '{}'\nCause: non-UTF-8 file not supported",
                right.display()
            )
        );
    }

    #[test]
    fn content_over_the_size_limit_is_refused_naming_the_path() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("big.txt");
        let right = dir.path().join("b.txt");
        let file = fs::File::create(&left).unwrap();
        file.set_len(MAX_DIFF_FILE_BYTES + 1).unwrap();
        fs::write(&right, "b\n").unwrap();

        let error = resolve(&left, &right).unwrap_err().to_string();

        assert!(
            error.starts_with(&format!(
                "Error: Cannot open '{}'\nCause: file too large",
                left.display()
            )),
            "{error}"
        );
    }

    #[test]
    fn a_file_the_user_cannot_write_is_read_only() {
        let dir = tempdir().unwrap();
        let left = dir.path().join("locked.txt");
        let right = dir.path().join("b.txt");
        fs::write(&left, "a\n").unwrap();
        fs::write(&right, "b\n").unwrap();
        let mut permissions = fs::metadata(&left).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&left, permissions).unwrap();

        let pair = files(resolve(&left, &right).unwrap());

        assert!(!pair.left.is_writable());
        assert!(pair.right.is_writable());
    }
}
