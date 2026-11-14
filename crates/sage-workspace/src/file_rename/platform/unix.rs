//! Closed-file rename on macOS and Linux. Unix cannot hold a path against
//! rename the way Windows withholds FILE_SHARE_DELETE, so every directory is
//! opened component by component without following links and the rename runs
//! inside the held parent descriptor. The source is held open, so its inode
//! cannot be freed and its identity reused while it is re-proven.

use std::{
    ffi::CString,
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path},
    sync::Arc,
};

use sage_core::error::{AppError, AppResult};

use crate::{RootIdentity, root_identity_metadata};

use super::{
    FileRenameAuthority, ParentAuthority, RenameIdentityProof, RenameObservation, observe_listing,
    verify_visible_authority,
};

fn c_name(name: &str) -> AppResult<CString> {
    if name.is_empty() || name.contains('/') {
        return Err(AppError::InvalidRequest(
            "file rename name is not one path component".into(),
        ));
    }
    CString::new(name).map_err(|_| AppError::InvalidRequest("file rename name contains NUL".into()))
}

/// Opens `name` (relative to `parent`, or absolute without one) as a
/// directory, refusing a final symbolic link.
fn open_directory(parent: Option<&File>, name: &CString) -> AppResult<File> {
    let base = parent.map_or(libc::AT_FDCWD, AsRawFd::as_raw_fd);
    // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
    let fd = unsafe {
        libc::openat(
            base,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(AppError::InvalidRequest(
            "file rename parent is not a plain directory".into(),
        ));
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_parent(authority: ParentAuthority<'_>) -> AppResult<Vec<File>> {
    let root_path = CString::new(authority.root.as_os_str().as_encoded_bytes())
        .map_err(|_| AppError::InvalidRequest("workspace root contains NUL".into()))?;
    let root = open_directory(None, &root_path)?;
    if root_identity_metadata(&root.metadata()?)? != authority.expected_root {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before file rename".into(),
        ));
    }
    let volume = root.metadata()?.dev();
    let mut directories = vec![root];
    for component in Path::new(authority.relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(
                "file rename parent is not canonical".into(),
            ));
        };
        let name =
            c_name(name.to_str().ok_or_else(|| {
                AppError::InvalidRequest("file rename parent is not UTF-8".into())
            })?)?;
        let next = open_directory(directories.last(), &name)?;
        // A mount point inside the Project fails, as a Windows reparse point does.
        if next.metadata()?.dev() != volume {
            return Err(AppError::InvalidRequest(
                "file rename parent crosses a mount point".into(),
            ));
        }
        directories.push(next);
    }
    Ok(directories)
}

/// Holds the source itself. Linux opens it as a path only, which needs no
/// read permission; macOS has no such mode and opens it for reading.
fn open_source(parent: &File, name: &CString) -> AppResult<File> {
    // Checked first, so a device node or FIFO is never opened.
    file_identity_at(parent, name)?;
    #[cfg(target_os = "linux")]
    let access = libc::O_PATH;
    #[cfg(target_os = "macos")]
    let access = libc::O_RDONLY | libc::O_NONBLOCK;
    // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            access | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    let source = unsafe { File::from_raw_fd(fd) };
    if !source.metadata()?.is_file() {
        return Err(AppError::InvalidRequest(
            "file rename source must be one regular non-link file".into(),
        ));
    }
    Ok(source)
}

/// The identity of the regular file `name` in `parent`, without following a
/// link.
fn file_identity_at(parent: &File, name: &CString) -> AppResult<RootIdentity> {
    let stat = stat_at(parent, name)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(AppError::InvalidRequest(
            "file rename source must be one regular non-link file".into(),
        ));
    }
    let mut file_id = [0_u8; 16];
    file_id[8..].copy_from_slice(&(stat.st_ino as u64).to_be_bytes());
    Ok(RootIdentity {
        volume: stat.st_dev as u64,
        file_id,
    })
}

fn stat_at(parent: &File, name: &CString) -> std::io::Result<libc::stat> {
    // SAFETY: an all-zero stat is a valid value for fstatat to overwrite.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstatat reads a NUL-terminated name and writes one stat.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stat)
}

fn rename_without_replacing(parent: &File, source: &CString, target: &CString) -> AppResult<()> {
    let fd = parent.as_raw_fd();
    // SAFETY: both names are NUL-terminated and both refer to the held parent.
    #[cfg(target_os = "linux")]
    let renamed = unsafe {
        libc::renameat2(
            fd,
            source.as_ptr(),
            fd,
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    // SAFETY: as above.
    #[cfg(target_os = "macos")]
    let renamed =
        unsafe { libc::renameatx_np(fd, source.as_ptr(), fd, target.as_ptr(), libc::RENAME_EXCL) };
    if renamed == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOTSUP)) {
        // Replacing would be the only fallback, so there is none.
        return Err(AppError::Unsupported(
            "this file system cannot rename a file without the risk of replacing another".into(),
        ));
    }
    Err(error.into())
}

/// How the rename runs. Normally nothing may exist at the target. A rename
/// that only changes letter case on a case-insensitive macOS volume finds the
/// source itself there, which a no-replace rename refuses; a plain rename is
/// then the only way, and it cannot replace another file because both
/// spellings name one directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameMode {
    NoReplace,
    CaseOnly,
}

fn rename_mode(
    parent: &File,
    names: (&str, &str),
    target: &CString,
    source: RootIdentity,
) -> AppResult<RenameMode> {
    let existing = match stat_at(parent, target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RenameMode::NoReplace);
        }
        Err(error) => return Err(error.into()),
        Ok(existing) => existing,
    };
    // One link proves the volume folds both spellings to the source's single
    // entry; a second link could be a distinct entry that Rust's lowercasing
    // happens to equate.
    if names.0.to_lowercase() == names.1.to_lowercase()
        && case_insensitive(parent)
        && existing.st_nlink == 1
        && file_identity_at(parent, target).is_ok_and(|target| target == source)
    {
        return Ok(RenameMode::CaseOnly);
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into())
}

#[cfg(target_os = "macos")]
fn case_insensitive(parent: &File) -> bool {
    // SAFETY: fpathconf only queries the volume of a held descriptor.
    unsafe { libc::fpathconf(parent.as_raw_fd(), libc::_PC_CASE_SENSITIVE) == 0 }
}

/// Linux case-folding directories are rare and opt-in; a case-only rename
/// there fails safely as a collision.
#[cfg(target_os = "linux")]
fn case_insensitive(_parent: &File) -> bool {
    false
}

fn rename_once(parent: &File, from: &CString, to: &CString, mode: RenameMode) -> AppResult<()> {
    if mode == RenameMode::NoReplace {
        return rename_without_replacing(parent, from, to);
    }
    let fd = parent.as_raw_fd();
    // SAFETY: both names are NUL-terminated and both refer to the held parent.
    if unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Renames and then checks that the file now at the target is `source`. The
/// rename is by name, so a file swapped in at the source name in the instant
/// after the last check would have moved instead. It is moved back the same
/// way, which replaces nothing. Something did change on disk meanwhile, so
/// that outcome is reported as an unconfirmed rename with a warning, never as
/// a failure before mutation.
fn rename_bound(
    parent: &File,
    names: (&CString, &CString),
    source: RootIdentity,
    mode: RenameMode,
) -> AppResult<Option<String>> {
    let (from, to) = names;
    rename_once(parent, from, to, mode)?;
    if file_identity_at(parent, to).is_ok_and(|target| target == source) {
        return Ok(None);
    }
    Ok(Some(if rename_once(parent, to, from, mode).is_ok() {
        "A different file was at the source name during the rename and was moved back; Explorer is refreshing."
            .to_owned()
    } else {
        "A different file was at the source name during the rename and could not be moved back; Explorer is refreshing."
            .to_owned()
    }))
}

fn parent_path(root: &Path, parent_relative: &str) -> std::path::PathBuf {
    if parent_relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_relative)
    }
}

/// The visible path, not the held parent, must show the retained source
/// under the target name for the rename to be confirmed.
fn target_is_retained_source(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
    target_name: &str,
) -> bool {
    verify_visible_authority(authority, root, parent_relative).is_ok()
        && std::fs::symlink_metadata(parent_path(root, parent_relative).join(target_name))
            .is_ok_and(|target| {
                target.is_file()
                    && root_identity_metadata(&target)
                        .is_ok_and(|target| target == authority.proof.source)
            })
}

pub(in crate::file_rename) fn rename_file_noreplace<C, P, S, L>(
    authority: ParentAuthority<'_>,
    source_name: &str,
    target_name: &str,
    committed: C,
    preflight: P,
    sync_parent: S,
    list_after: L,
) -> AppResult<RenameObservation>
where
    C: FnOnce(Arc<FileRenameAuthority>),
    P: FnOnce() -> AppResult<()>,
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<sage_domain::project::DirectoryListing>,
{
    let root = authority.root;
    let expected_root = authority.expected_root;
    let parent_relative = authority.relative_path;
    let (source_c, target_c) = (c_name(source_name)?, c_name(target_name)?);
    let directories = open_parent(authority)?;
    let parent = directories
        .last()
        .expect("file rename root/parent authority exists");
    let source = open_source(parent, &source_c)?;
    let proof = RenameIdentityProof {
        root: expected_root,
        parent: root_identity_metadata(&parent.metadata()?)?,
        source: root_identity_metadata(&source.metadata()?)?,
    };
    let authority = Arc::new(FileRenameAuthority {
        proof,
        directories,
        source,
    });
    let source_is_current = || -> AppResult<()> {
        verify_visible_authority(&authority, root, parent_relative)?;
        if file_identity_at(authority.parent(), &source_c)? != authority.proof.source {
            return Err(AppError::InvalidRequest(
                "file rename source identity changed before mutation".into(),
            ));
        }
        Ok(())
    };
    source_is_current()?;
    preflight()?;
    source_is_current()?;
    // Checking the target first also keeps a file system that ignores the
    // no-replace flag from overwriting a file that already exists.
    let mode = rename_mode(
        authority.parent(),
        (source_name, target_name),
        &target_c,
        authority.proof.source,
    )?;
    let warning = rename_bound(
        authority.parent(),
        (&source_c, &target_c),
        authority.proof.source,
        mode,
    )?;
    committed(Arc::clone(&authority));
    let durability = sync_parent(authority.parent())
        .err()
        .map(|_| "File was renamed, but its durability could not be confirmed.".to_owned());
    let warning = warning.or(durability);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    let target_identity_confirmed =
        target_is_retained_source(&authority, root, parent_relative, target_name);
    Ok(RenameObservation {
        warning,
        listing,
        target_identity_confirmed,
    })
}

pub(in crate::file_rename) fn observe_recovery<L>(
    authority: Arc<FileRenameAuthority>,
    root: &Path,
    parent_relative: &str,
    _source_name: &str,
    target_name: &str,
    list_after: L,
) -> AppResult<RenameObservation>
where
    L: FnOnce() -> AppResult<sage_domain::project::DirectoryListing>,
{
    verify_visible_authority(&authority, root, parent_relative)?;
    let target_identity_confirmed =
        target_is_retained_source(&authority, root, parent_relative, target_name);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(RenameObservation {
        warning: None,
        listing,
        target_identity_confirmed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(std::path::PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn identity(path: &std::path::Path) -> RootIdentity {
        root_identity_metadata(&std::fs::metadata(path).unwrap()).unwrap()
    }

    #[test]
    fn a_file_swapped_in_during_the_rename_is_moved_back() {
        let dir =
            Dir(std::env::temp_dir().join(format!("sage-rename-bound-{}", uuid::Uuid::new_v4())));
        let root = &dir.0;
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("swapped.rs"), "swapped\n").unwrap();
        std::fs::write(root.join("held.rs"), "held\n").unwrap();
        let parent = File::open(root).unwrap();
        let names = (c_name("swapped.rs").unwrap(), c_name("new.rs").unwrap());

        // The held source is elsewhere, as if swapped out just before.
        let warning = rename_bound(
            &parent,
            (&names.0, &names.1),
            identity(&root.join("held.rs")),
            RenameMode::NoReplace,
        )
        .unwrap()
        .unwrap();

        assert!(warning.contains("moved back"));
        assert_eq!(
            std::fs::read_to_string(root.join("swapped.rs")).unwrap(),
            "swapped\n"
        );
        assert!(!root.join("new.rs").exists());
        let names = (c_name("held.rs").unwrap(), c_name("new.rs").unwrap());
        let held = identity(&root.join("held.rs"));
        assert_eq!(
            rename_bound(&parent, (&names.0, &names.1), held, RenameMode::NoReplace).unwrap(),
            None
        );
        assert_eq!(
            std::fs::read_to_string(root.join("new.rs")).unwrap(),
            "held\n"
        );
    }

    /// On a case-sensitive file system a hard link under a case variant of
    /// the name is a collision, never a plain rename.
    #[test]
    fn a_hard_link_under_a_case_variant_is_a_collision() {
        let dir =
            Dir(std::env::temp_dir().join(format!("sage-rename-case-{}", uuid::Uuid::new_v4())));
        let root = &dir.0;
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("a.rs"), "held\n").unwrap();
        let parent = File::open(root).unwrap();
        let held = identity(&root.join("a.rs"));
        let target = c_name("A.rs").unwrap();

        // A case-insensitive volume cannot hold both spellings.
        if !case_insensitive(&parent) {
            std::fs::hard_link(root.join("a.rs"), root.join("A.rs")).unwrap();
            let error = rename_mode(&parent, ("a.rs", "A.rs"), &target, held).unwrap_err();
            assert!(
                matches!(error, AppError::Io(error) if error.kind() == std::io::ErrorKind::AlreadyExists)
            );
        }
        let missing = c_name("b.rs").unwrap();
        assert_eq!(
            rename_mode(&parent, ("a.rs", "b.rs"), &missing, held).unwrap(),
            RenameMode::NoReplace
        );
    }
}
