//! Moving a closed file or empty folder to the system trash on macOS and
//! Linux. As in file rename, every directory is opened component by component
//! without following links, and the source is held open so its inode cannot
//! be freed and its identity reused while it is re-proven. Linux moves the
//! item inside the held parent (freedesktop.rs); macOS hands its path to the
//! system Trash (macos.rs). Either way, the item that arrived in the trash is
//! checked against the held source afterwards and put back if it differs.

use std::{
    ffi::{CStr, CString},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::fs::MetadataExt,
    },
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use sage_core::error::{AppError, AppResult};

use crate::workspace::{
    RootIdentity, resource_trash::TRASH_NAME, root_identity_metadata, root_identity_path,
};
use sage_domain::{file_operations::TrashResourceKind, project::DirectoryListing};

use super::{ParentAuthority, ResourceTrashIdentity, TrashIdentityProof, TrashObservation};

#[cfg(target_os = "linux")]
mod freedesktop;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
use freedesktop::move_to_trash;
#[cfg(target_os = "macos")]
use macos::move_to_trash;

#[derive(Debug)]
pub(in crate::workspace::resource_trash) struct ResourceTrashAuthority {
    proof: TrashIdentityProof,
    source_path: PathBuf,
    parent: File,
    name: CString,
    _source: File,
}

impl ResourceTrashAuthority {
    fn identity(&self) -> ResourceTrashIdentity {
        ResourceTrashIdentity {
            root: self.proof.root,
            parent: self.proof.parent,
            source: self.proof.source,
        }
    }
}

fn c_name(name: &str) -> AppResult<CString> {
    if name.is_empty() || name.contains('/') {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} name is not one path component"
        )));
    }
    CString::new(name)
        .map_err(|_| AppError::InvalidRequest(format!("{TRASH_NAME} name contains NUL")))
}

/// Opens `name` (relative to `parent`, or absolute without one) as a
/// directory, refusing a final symbolic link.
fn open_directory(parent: Option<&File>, name: &CStr, flags: libc::c_int) -> std::io::Result<File> {
    let base = parent.map_or(libc::AT_FDCWD, AsRawFd::as_raw_fd);
    // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
    let fd = unsafe {
        libc::openat(
            base,
            name.as_ptr(),
            flags | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn stat_at(parent: &File, name: &CStr) -> std::io::Result<libc::stat> {
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

#[allow(
    clippy::unnecessary_cast,
    reason = "st_dev is a signed 32-bit value on macOS"
)]
fn stat_identity(stat: libc::stat) -> RootIdentity {
    let mut file_id = [0_u8; 16];
    file_id[8..].copy_from_slice(&(stat.st_ino as u64).to_be_bytes());
    RootIdentity {
        volume: stat.st_dev as u64,
        file_id,
    }
}

/// The identity of `name` in `parent` if it is still the kind requested,
/// without following a link.
fn identity_at(parent: &File, name: &CStr, kind: TrashResourceKind) -> AppResult<RootIdentity> {
    let stat = stat_at(parent, name)?;
    let format = stat.st_mode & libc::S_IFMT;
    let kind_matches = match kind {
        TrashResourceKind::File => format == libc::S_IFREG,
        TrashResourceKind::EmptyFolder => format == libc::S_IFDIR,
    };
    if !kind_matches {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source kind changed or is a symbolic link"
        )));
    }
    // A second name would leave the content in the Project after the move,
    // as the Windows adapter also refuses.
    if kind == TrashResourceKind::File && stat.st_nlink != 1 {
        return Err(AppError::InvalidRequest(format!(
            "files with hard-link aliases cannot be moved to the {TRASH_NAME}"
        )));
    }
    Ok(stat_identity(stat))
}

fn open_parent(authority: &ParentAuthority<'_>) -> AppResult<File> {
    let root_path = CString::new(authority.root.as_os_str().as_encoded_bytes())
        .map_err(|_| AppError::InvalidRequest("workspace root contains NUL".into()))?;
    let mut directory = open_directory(None, &root_path, libc::O_RDONLY)?;
    if root_identity_metadata(&directory.metadata()?)? != authority.expected_root {
        return Err(AppError::InvalidRequest(format!(
            "workspace root identity changed before {TRASH_NAME} operation"
        )));
    }
    let volume = directory.metadata()?.dev();
    for component in Path::new(authority.relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(format!(
                "{TRASH_NAME} parent is not canonical"
            )));
        };
        let name = c_name(name.to_str().ok_or_else(|| {
            AppError::InvalidRequest(format!("{TRASH_NAME} parent is not UTF-8"))
        })?)?;
        let next = open_directory(Some(&directory), &name, libc::O_RDONLY)?;
        // A mount point inside the Project fails, as a Windows reparse point does.
        if next.metadata()?.dev() != volume {
            return Err(AppError::InvalidRequest(format!(
                "{TRASH_NAME} parent crosses a mount point"
            )));
        }
        directory = next;
    }
    Ok(directory)
}

/// Holds the source itself. Linux opens it as a path only, which needs no
/// read permission; macOS opens it for event notification only, which needs
/// none either.
fn open_source(parent: &File, name: &CStr, kind: TrashResourceKind) -> AppResult<File> {
    // Checked first, so a device node or FIFO is never opened.
    let expected = identity_at(parent, name, kind)?;
    // A mount point would leave its volume's Trash to be made inside the
    // Project, and would not move with its name.
    if expected.volume != parent.metadata()?.dev() {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source is on another volume than its folder"
        )));
    }
    #[cfg(target_os = "linux")]
    let access = libc::O_PATH;
    #[cfg(target_os = "macos")]
    let access = libc::O_EVTONLY | libc::O_NONBLOCK;
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
    if root_identity_metadata(&source.metadata()?)? != expected {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source identity changed while it was opened"
        )));
    }
    Ok(source)
}

fn parent_path(root: &Path, parent_relative: &str) -> PathBuf {
    if parent_relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_relative)
    }
}

fn open_authority(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
) -> AppResult<Arc<ResourceTrashAuthority>> {
    let name = c_name(source_name)?;
    let parent = open_parent(&parent_authority)?;
    let source = open_source(&parent, &name, kind)?;
    let authority = Arc::new(ResourceTrashAuthority {
        proof: TrashIdentityProof {
            root: parent_authority.expected_root,
            parent: root_identity_metadata(&parent.metadata()?)?,
            source: root_identity_metadata(&source.metadata()?)?,
            kind,
        },
        source_path: parent_path(parent_authority.root, parent_authority.relative_path)
            .join(source_name),
        parent,
        name,
        _source: source,
    });
    verify_visible_source(&authority)?;
    Ok(authority)
}

pub(in crate::workspace::resource_trash) fn observe_identity(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
) -> AppResult<ResourceTrashIdentity> {
    Ok(open_authority(parent_authority, source_name, kind)?.identity())
}

fn verify_visible_authority(
    authority: &ResourceTrashAuthority,
    root: &Path,
    parent_relative: &str,
) -> AppResult<()> {
    if root_identity_path(root)? != authority.proof.root
        || root_identity_path(&parent_path(root, parent_relative))? != authority.proof.parent
    {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} path authority changed during reconciliation"
        )));
    }
    Ok(())
}

/// The visible path and the held parent both still name the held source.
fn verify_visible_source(authority: &ResourceTrashAuthority) -> AppResult<()> {
    let held = identity_at(&authority.parent, &authority.name, authority.proof.kind)?;
    let visible = std::fs::symlink_metadata(&authority.source_path)?;
    let visible_kind = match authority.proof.kind {
        TrashResourceKind::File => visible.is_file(),
        TrashResourceKind::EmptyFolder => visible.is_dir(),
    };
    if held != authority.proof.source
        || !visible_kind
        || root_identity_metadata(&visible)? != authority.proof.source
    {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source identity changed before mutation"
        )));
    }
    Ok(())
}

fn clear_errno() {
    // SAFETY: errno is this thread's own variable.
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location() = 0;
    }
    // SAFETY: as above.
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error() = 0;
    }
}

/// An empty folder is re-read through a held descriptor just before it
/// moves, so one filled since admission is refused. Something written in the
/// instant before the move would still go with it, recoverably.
fn verify_still_empty(authority: &ResourceTrashAuthority) -> AppResult<()> {
    if authority.proof.kind != TrashResourceKind::EmptyFolder {
        return Ok(());
    }
    let folder = open_directory(Some(&authority.parent), &authority.name, libc::O_RDONLY)?;
    if root_identity_metadata(&folder.metadata()?)? != authority.proof.source {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source identity changed before mutation"
        )));
    }
    let fd = folder.into_raw_fd();
    // SAFETY: fdopendir takes ownership of the descriptor on success only.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let error = std::io::Error::last_os_error();
        // SAFETY: the descriptor is still ours after a failed fdopendir.
        unsafe { libc::close(fd) };
        return Err(error.into());
    }
    let mut empty = true;
    let mut read_error = None;
    loop {
        clear_errno();
        // SAFETY: the stream is open; readdir returns null at the end or on
        // an error, which only errno tells apart.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                read_error = Some(error);
            }
            break;
        }
        // SAFETY: d_name is NUL-terminated inside the entry readdir returned.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            empty = false;
            break;
        }
    }
    // SAFETY: the stream is open and closed exactly once, with its descriptor.
    unsafe { libc::closedir(stream) };
    if let Some(error) = read_error {
        return Err(error.into());
    }
    if !empty {
        return Err(AppError::InvalidRequest(format!(
            "only empty folders can be moved to the {TRASH_NAME}"
        )));
    }
    Ok(())
}

fn observe_listing<L>(
    authority: &ResourceTrashAuthority,
    root: &Path,
    parent_relative: &str,
    list_after: L,
) -> AppResult<DirectoryListing>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    verify_visible_authority(authority, root, parent_relative)?;
    let listing = list_after()?;
    verify_visible_authority(authority, root, parent_relative)?;
    Ok(listing)
}

fn source_absent(authority: &ResourceTrashAuthority) -> bool {
    let missing = |result: std::io::Result<()>| matches!(result, Err(error) if error.kind() == std::io::ErrorKind::NotFound);
    missing(stat_at(&authority.parent, &authority.name).map(drop))
        && missing(std::fs::symlink_metadata(&authority.source_path).map(drop))
}

pub(in crate::workspace::resource_trash) fn recycle_one<C, P, L>(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
    expected_identity: ResourceTrashIdentity,
    mutation_started: C,
    preflight: P,
    list_after: L,
) -> AppResult<TrashObservation>
where
    C: FnOnce(Arc<ResourceTrashAuthority>) + Send + 'static,
    P: FnOnce() -> AppResult<()>,
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    let root = parent_authority.root;
    let parent_relative = parent_authority.relative_path;
    preflight()?;
    let authority = open_authority(parent_authority, source_name, kind)?;
    if authority.identity() != expected_identity {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source identity changed after durable intent admission"
        )));
    }
    verify_visible_source(&authority)?;
    verify_still_empty(&authority)?;
    let warning = move_to_trash(&authority, mutation_started)?;
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(TrashObservation {
        warning,
        listing,
        source_absent_confirmed: source_absent(&authority),
    })
}

pub(in crate::workspace::resource_trash) fn observe_recovery<L>(
    authority: Arc<ResourceTrashAuthority>,
    root: &Path,
    parent_relative: &str,
    list_after: L,
) -> AppResult<TrashObservation>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    verify_visible_authority(&authority, root, parent_relative)?;
    let source_absent_confirmed = source_absent(&authority);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(TrashObservation {
        warning: None,
        listing,
        source_absent_confirmed,
    })
}

#[cfg(test)]
mod tests;
