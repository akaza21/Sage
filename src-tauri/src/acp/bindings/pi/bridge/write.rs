use super::files::{Edit, read};
use crate::workspace::Workspace;

pub(crate) fn commit(workspace: &Workspace, edit: &Edit) -> Result<(), ()> {
    super::path::admit_resolved(workspace, &edit.relative)?;
    if !unchanged(workspace, edit) {
        return Err(());
    }
    platform_write(workspace, edit).map_err(|_| ())
}

/// The target still holds exactly what the user approved replacing (or is
/// still absent for a create). Every platform re-checks this after the new
/// bytes are durable and immediately before they become visible.
fn unchanged(workspace: &Workspace, edit: &Edit) -> bool {
    read(workspace, &edit.relative)
        .is_ok_and(|current| current.as_deref() == (!edit.created).then_some(edit.old.as_str()))
}

#[cfg(windows)]
fn platform_write(workspace: &Workspace, edit: &Edit) -> std::io::Result<()> {
    use crate::workspace::atomic_write_windows::{Existing, atomic_write_checked};
    atomic_write_checked(
        workspace.root(),
        workspace.root_identity().stable_bytes(),
        &workspace.root().join(&edit.relative),
        edit.new.as_bytes(),
        if edit.created {
            Existing::Refuse
        } else {
            Existing::Replace
        },
        || {
            unchanged(workspace, edit)
                .then_some(())
                .ok_or_else(|| std::io::ErrorKind::PermissionDenied.into())
        },
    )
}

#[cfg(unix)]
fn platform_write(workspace: &Workspace, edit: &Edit) -> std::io::Result<()> {
    use std::{
        ffi::CString,
        fs::File,
        io::Write,
        os::fd::{AsRawFd, FromRawFd},
        path::Path,
    };
    let refused = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let path = Path::new(&edit.relative);
    let parent = path.parent().and_then(Path::to_str).ok_or_else(refused)?;
    let held =
        crate::workspace::open_held_parent(workspace.root(), workspace.root_identity(), parent)
            .map_err(|_| refused())?;
    let target = CString::new(path.file_name().ok_or_else(refused)?.as_encoded_bytes())
        .map_err(|_| refused())?;
    let temporary =
        CString::new(format!(".sage-pi-{}.tmp", uuid::Uuid::new_v4())).map_err(|_| refused())?;
    let fd = held.as_raw_fd();
    // SAFETY: the held directory and terminated basename outlive openat.
    let descriptor = unsafe {
        libc::openat(
            fd,
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    let result = (|| {
        if !edit.created {
            let original = crate::workspace::open_workspace_text_file(
                workspace.root(),
                workspace.root_identity(),
                &edit.relative,
                None,
            )
            .map_err(|_| refused())?;
            file.set_permissions(original.metadata()?.permissions())?;
        }
        file.write_all(edit.new.as_bytes())?;
        file.sync_all()?;
        if !unchanged(workspace, edit) {
            return Err(refused());
        }
        let named =
            crate::workspace::open_held_parent(workspace.root(), workspace.root_identity(), parent)
                .map_err(|_| refused())?;
        if crate::workspace::root_identity_metadata(&named.metadata()?).map_err(|_| refused())?
            != crate::workspace::root_identity_metadata(&held.metadata()?).map_err(|_| refused())?
        {
            return Err(refused());
        }
        // SAFETY: both basenames and the held parent remain live; no path traversal occurs.
        let result = unsafe {
            if edit.created {
                // linkat is create-only, so a concurrently created file is never replaced.
                libc::linkat(fd, temporary.as_ptr(), fd, target.as_ptr(), 0)
            } else {
                libc::renameat(fd, temporary.as_ptr(), fd, target.as_ptr())
            }
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        held.sync_all()
    })();
    // SAFETY: cleanup addresses only the unique temporary basename in the held parent.
    unsafe {
        libc::unlinkat(fd, temporary.as_ptr(), 0);
    }
    result
}
