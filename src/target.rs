//! What a session compares — two directory trees or one file pair — resolved
//! from the command-line arguments before the terminal is touched.

use crate::side::Pair;
use crate::text::{LoadedText, MAX_DIFF_FILE_BYTES};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The two things a session compares.
///
/// Built once at startup and taken apart straight away, so the size gap
/// between the variants costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum ComparisonTarget {
    Directories {
        left: PathBuf,
        right: PathBuf,
    },
    /// The pair, and each side as it was loaded to check it can be shown —
    /// what File Diff opens on, so startup reads each side once.
    Files(FilePair, Pair<LoadedText>),
}

/// Both sides of a direct file comparison.
pub type FilePair = Pair<FileSide>;

/// One side of a direct file comparison.
#[derive(Debug)]
pub struct FileSide {
    path: PathBuf,
    /// How the side is read, and where writes, external tools, and the editor
    /// go. See [`FileSide::target_path`].
    source: SideSource,
    writable: bool,
}

/// Where one side of the Compared pair is read from, and whether a save
/// writes it: the one reader File Diff and startup share, so both refuse the
/// same content the same way. Cheap to clone and safe to send to a worker.
///
/// A file pair's side keeps one for its whole session, its path being where
/// saves, copies, and external tools go; see [`FileSide::target_path`].
#[derive(Clone, Debug)]
pub(crate) struct SideSource {
    path: PathBuf,
    origin: Origin,
}

#[derive(Clone, Debug)]
enum Origin {
    /// A Directory Tree row's side: missing, or a directory, loads as empty.
    Entry,
    /// A file named on the command line, which must still be there.
    File,
    /// The null device: always empty.
    NullDevice,
    /// A pipe read once at startup, because it cannot be read again.
    Captured(Arc<[u8]>),
}

impl SideSource {
    /// A Directory Tree row's side at `path`.
    pub(crate) fn entry(path: PathBuf) -> Self {
        Self {
            path,
            origin: Origin::Entry,
        }
    }

    /// A regular file named on the command line.
    pub(crate) fn file(path: PathBuf) -> Self {
        Self {
            path,
            origin: Origin::File,
        }
    }

    /// The null device, under this platform's name.
    pub(crate) fn null_device(path: PathBuf) -> Self {
        Self {
            path,
            origin: Origin::NullDevice,
        }
    }

    /// Bytes captured from a pipe at `path`.
    pub(crate) fn captured(path: PathBuf, bytes: Arc<[u8]>) -> Self {
        Self {
            path,
            origin: Origin::Captured(bytes),
        }
    }

    /// Where the side is read and written.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a save writes this side to a file it can hash again; the null
    /// device and a pipe are never written.
    pub(crate) fn is_written_to_disk(&self) -> bool {
        matches!(self.origin, Origin::Entry | Origin::File)
    }

    /// Size and modification time of a file named on the command line, for
    /// File Diff's panes; `None` for any other side.
    pub(crate) fn info(&self) -> Option<crate::diff::FileInfo> {
        if !matches!(self.origin, Origin::File) {
            return None;
        }
        let meta = std::fs::metadata(&self.path).ok()?;
        Some(crate::diff::FileInfo {
            is_dir: false,
            size: meta.len(),
            modified: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        })
    }

    /// Read the side for the built-in diff, once. The error is the cause alone;
    /// the caller names the path.
    pub(crate) fn load(&self) -> Result<crate::text::LoadedText, String> {
        use crate::text::{LoadedText, TextRejection, MAX_DIFF_FILE_BYTES};
        let bytes = match &self.origin {
            Origin::NullDevice => return Ok(LoadedText::default()),
            Origin::Captured(bytes) => bytes.to_vec(),
            Origin::Entry if !self.path.is_file() => return Ok(LoadedText::default()),
            Origin::Entry | Origin::File => {
                let len = std::fs::metadata(&self.path)
                    .map_err(|e| e.to_string())?
                    .len();
                if len > MAX_DIFF_FILE_BYTES {
                    return Err(TextRejection::TooLarge(Some(len)).to_string());
                }
                std::fs::read(&self.path).map_err(|e| e.to_string())?
            }
        };
        LoadedText::from_bytes(bytes).map_err(|rejection| rejection.to_string())
    }
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
        self.source.path()
    }

    /// Whether an external tool can open this side again by
    /// [`FileSide::target_path`]; a pipe was consumed at startup.
    pub fn can_reopen(&self) -> bool {
        !matches!(self.source.origin, Origin::Captured(_))
    }

    /// Whether this side is the null device, which has nothing to copy.
    pub fn is_null_device(&self) -> bool {
        matches!(self.source.origin, Origin::NullDevice)
    }

    /// Whether this side is a regular file an editor can open.
    pub fn is_regular_file(&self) -> bool {
        matches!(self.source.origin, Origin::File)
    }

    /// The bytes a copy from this side writes, when they cannot be re-read
    /// from [`FileSide::path`].
    pub fn captured_bytes(&self) -> Option<&[u8]> {
        match &self.source.origin {
            Origin::Entry | Origin::File => None,
            Origin::NullDevice => Some(&[]),
            Origin::Captured(bytes) => Some(bytes),
        }
    }

    /// Size and modification time for the pane title and info bar; `None` for
    /// a side that is not a regular file.
    pub fn info(&self) -> Option<crate::diff::FileInfo> {
        self.source.info()
    }

    /// How File Diff reads this side.
    pub(crate) fn source(&self) -> SideSource {
        self.source.clone()
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

fn side(path: PathBuf) -> Result<(FileSide, LoadedText), StartupError> {
    let unreadable = |cause: String| StartupError::Unreadable {
        path: path.clone(),
        cause,
    };
    let (source, writable) = match classify(&path)? {
        ArgKind::NullDevice => {
            let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
            (SideSource::null_device(PathBuf::from(null)), false)
        }
        ArgKind::Stream => {
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .and_then(|file| file.take(MAX_DIFF_FILE_BYTES + 1).read_to_end(&mut bytes))
                .map_err(|e| unreadable(e.to_string()))?;
            if bytes.len() as u64 > MAX_DIFF_FILE_BYTES {
                return Err(unreadable(
                    crate::text::TextRejection::TooLarge(None).to_string(),
                ));
            }
            (SideSource::captured(path.clone(), bytes.into()), false)
        }
        ArgKind::File => {
            let writable = std::fs::OpenOptions::new().write(true).open(&path).is_ok();
            let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            (SideSource::file(target), writable)
        }
        ArgKind::Directory => return Err(StartupError::NotAFile(path)),
    };
    let side = FileSide {
        path: path.clone(),
        source,
        writable,
    };
    let loaded = side.source().load().map_err(unreadable)?;
    Ok((side, loaded))
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
    let (left, left_loaded) = side(left)?;
    let (right, right_loaded) = side(right)?;
    Ok(ComparisonTarget::Files(
        Pair::new(left, right),
        Pair::new(left_loaded, right_loaded),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn files(target: ComparisonTarget) -> FilePair {
        match target {
            ComparisonTarget::Files(pair, _) => pair,
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
        assert_eq!(pair.left.source().load().unwrap().text, "a\n");
    }

    #[test]
    fn the_null_device_is_an_empty_read_only_side() {
        let dir = tempdir().unwrap();
        let right = dir.path().join("added.txt");
        fs::write(&right, "new\n").unwrap();

        let pair = files(resolve(Path::new("/dev/null"), &right).unwrap());

        assert_eq!(pair.left.path(), Path::new("/dev/null"));
        assert_eq!(pair.left.source().load().unwrap().text, "");
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

        assert_eq!(pair.right.source().load().unwrap().text, "");
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

        assert_eq!(pair.left.source().load().unwrap().text, "from a pipe\n");
        assert_eq!(pair.left.source().load().unwrap().text, "from a pipe\n");
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
        let fifo = crate::test_support::fifo(dir.path(), "63");

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
