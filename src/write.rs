//! Writing to either root on disk, safely: every copy stays inside the
//! destination root, a symlink is recreated rather than followed, a directory
//! copy takes exactly the entries the scan listed (Issue #235), and a save
//! lands all of its files or none.

/// Copy exactly the entries the scan listed under a directory, relative to it.
///
/// `entries` comes from the scan snapshot, so nothing hidden by an exclusion and
/// nothing created since the scan is copied. Existing destination entries that
/// the scan did not list are left in place — that is what makes it a merge.
pub(crate) fn copy_scanned_subtree(
    src_root: &std::path::Path,
    dst_root_dir: &std::path::Path,
    dst_root: &std::path::Path,
    entries: &[(std::path::PathBuf, bool)],
) -> std::io::Result<()> {
    if !path_is_under(dst_root_dir, dst_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "copy destination escapes the target root",
        ));
    }
    remove_destination_symlink(dst_root_dir)?;
    std::fs::create_dir_all(dst_root_dir)?;
    for (relative, is_dir) in entries {
        let src = src_root.join(relative);
        let dst = dst_root_dir.join(relative);
        if !path_is_under(&dst, dst_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "copy destination escapes the target root",
            ));
        }
        if *is_dir {
            remove_destination_symlink(&dst)?;
            std::fs::create_dir_all(&dst)?;
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let meta = std::fs::symlink_metadata(&src)?;
        if meta.file_type().is_symlink() {
            // Replace only this validated leaf; never follow or delete through it.
            remove_destination_symlink(&dst)?;
            recreate_symlink(&src, &dst)?;
        } else {
            remove_destination_symlink(&dst)?;
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub(crate) fn normalize_lexically(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

pub(crate) fn path_is_under(path: &std::path::Path, root: &std::path::Path) -> bool {
    let path = normalize_lexically(path);
    let root = normalize_lexically(root);
    path.starts_with(&root)
}

/// Write every `(path, new_contents, original_contents)` triple, all-or-nothing.
///
/// Each file is staged into a sibling temporary file first, so nothing visible
/// changes until every write is known to have succeeded. If a replacement fails
/// part-way, the already-replaced files are restored from their originals rather
/// than leaving one side written and the other not (Issue #235).
pub(crate) fn commit_all_or_nothing(
    writes: &[(std::path::PathBuf, String, String)],
) -> std::io::Result<()> {
    use std::path::PathBuf;

    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    let cleanup = |temps: &[(PathBuf, PathBuf)]| {
        for (temp, _) in temps {
            let _ = std::fs::remove_file(temp);
        }
    };

    for (path, contents, _) in writes {
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".to_string());
        let temp = parent.join(format!(".{file_name}.duodiff-tmp"));
        if let Err(e) =
            std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&temp, contents))
        {
            cleanup(&temps);
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
        temps.push((temp, path.clone()));
    }

    let mut replaced: Vec<usize> = Vec::new();
    for (i, (temp, path)) in temps.iter().enumerate() {
        if let Err(e) = std::fs::rename(temp, path) {
            // Put back whatever already moved, then drop the remaining temps.
            for &done in &replaced {
                let _ = std::fs::write(&temps[done].1, &writes[done].2);
            }
            cleanup(&temps[i..]);
            return Err(e);
        }
        replaced.push(i);
    }
    Ok(())
}

pub(crate) fn copy_entry_checked(
    src: &std::path::Path,
    dst: &std::path::Path,
    dst_root: &std::path::Path,
) -> std::io::Result<()> {
    if !path_is_under(dst, dst_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "copy destination escapes the target root",
        ));
    }

    let meta = std::fs::symlink_metadata(src)?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        remove_destination_symlink(dst)?;
        recreate_symlink(src, dst)
    } else if file_type.is_dir() {
        // The scan listed this entry as something else, so there is no
        // listing to copy from; walking the directory instead would copy
        // what the scan excludes (Issue #235).
        Err(std::io::Error::other(
            "it changed on disk after the last scan; rescan and copy again",
        ))
    } else if file_type.is_file() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        remove_destination_symlink(dst)?;
        std::fs::copy(src, dst).map(|_| ())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Source path not found on disk",
        ))
    }
}

fn recreate_symlink(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    let target = std::fs::read_link(src)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(windows)]
    {
        // Prefer recreating the link; Windows may require elevated privileges.
        let meta = std::fs::symlink_metadata(src)?;
        // `is_dir` on symlink metadata reports the *target* type on Windows.
        if meta.file_type().is_dir() {
            std::os::windows::fs::symlink_dir(target, dst)
        } else {
            std::os::windows::fs::symlink_file(target, dst)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (src, dst, target);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlink copy is not supported on this platform",
        ))
    }
}

/// Replace a destination link itself, never the file or directory it points to.
fn remove_destination_symlink(dst: &std::path::Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(dst) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(dst),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_scanned_directory_copy_skips_entries_absent_from_the_snapshot() {
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        let source = left.path().join("project");
        std::fs::create_dir_all(source.join(".git")).unwrap();
        std::fs::write(source.join("visible.txt"), "copy me").unwrap();
        std::fs::write(source.join(".git/config"), "do not copy").unwrap();

        copy_scanned_subtree(
            &source,
            &right.path().join("project"),
            right.path(),
            &[(PathBuf::from("visible.txt"), false)],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(right.path().join("project/visible.txt")).unwrap(),
            "copy me"
        );
        assert!(
            !right.path().join("project/.git").exists(),
            "excluded .git must not be copied merely because it exists on disk"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_replaces_destination_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("safe.txt");
        let destination = destination_dir.path().join("safe.txt");
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&source, "replacement").unwrap();
        std::fs::write(&outside_file, "must survive").unwrap();
        symlink(&outside_file, &destination).unwrap();

        copy_entry_checked(&source, &destination, destination_dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "replacement"
        );
        assert!(destination
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_file());
        assert_eq!(
            std::fs::read_to_string(outside_file).unwrap(),
            "must survive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_scanned_copy_replaces_destination_root_symlink_without_walking_it() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("project");
        let destination = destination_dir.path().join("project");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("visible.txt"), "safe copy").unwrap();
        symlink(outside.path(), &destination).unwrap();

        copy_scanned_subtree(
            &source,
            &destination,
            destination_dir.path(),
            &[(PathBuf::from("visible.txt"), false)],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("visible.txt")).unwrap(),
            "safe copy"
        );
        assert!(
            !outside.path().join("visible.txt").exists(),
            "a destination root symlink must be replaced, not followed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_scanned_copy_replaces_destination_directory_symlink_without_walking_it() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("project");
        let destination = destination_dir.path().join("project");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("nested/visible.txt"), "safe copy").unwrap();
        symlink(outside.path(), destination.join("nested")).unwrap();

        copy_scanned_subtree(
            &source,
            &destination,
            destination_dir.path(),
            &[
                (PathBuf::from("nested"), true),
                (PathBuf::from("nested/visible.txt"), false),
            ],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("nested/visible.txt")).unwrap(),
            "safe copy"
        );
        assert!(
            !outside.path().join("visible.txt").exists(),
            "a destination directory symlink must be replaced, not followed"
        );
    }

    #[test]
    fn test_path_is_under_lexical() {
        let root = std::path::Path::new("/tmp/root");
        assert!(path_is_under(std::path::Path::new("/tmp/root"), root));
        assert!(path_is_under(std::path::Path::new("/tmp/root/a/b"), root));
        assert!(!path_is_under(
            std::path::Path::new("/tmp/root/../escape"),
            root
        ));
        assert!(!path_is_under(std::path::Path::new("/tmp/other"), root));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_recreates_symlink_not_target_tree() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        // Symlink inside left pointing at outside dir
        symlink(outside.path(), left.path().join("link_out")).unwrap();

        // Copying the symlink should recreate the link, not walk outside.
        copy_entry_checked(
            &left.path().join("link_out"),
            &right.path().join("link_out"),
            right.path(),
        )
        .unwrap();
        assert!(right
            .path()
            .join("link_out")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        // Destination must not materialize secret.txt as a regular copied tree.
        assert!(
            !right.path().join("link_out").join("secret.txt").is_file()
                || std::fs::symlink_metadata(right.path().join("link_out"))
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
        );
    }

    /// An entry the scan saw as a file but that is now a directory is not
    /// copied: there is no scan listing to copy it from, and walking it would
    /// take what the scan excludes (Issue #235).
    #[test]
    fn a_directory_the_scan_did_not_list_is_not_copied() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let source = left.path().join("was_a_file");
        std::fs::create_dir_all(source.join(".git")).unwrap();
        std::fs::write(source.join(".git/config"), "secret").unwrap();

        let error = copy_entry_checked(&source, &right.path().join("was_a_file"), right.path())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "it changed on disk after the last scan; rescan and copy again"
        );
        assert!(!right.path().join("was_a_file").exists());
    }

    /// A copy whose destination leaves the target root is refused.
    #[test]
    fn a_copy_outside_the_target_root_is_refused() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let source = left.path().join("a.txt");
        std::fs::write(&source, "a").unwrap();

        let error =
            copy_entry_checked(&source, &right.path().join("../a.txt"), right.path()).unwrap_err();

        assert!(error.to_string().contains("escapes"), "{error}");
    }
}
