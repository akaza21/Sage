use super::files::{Edit, read};
use crate::workspace::Workspace;

#[cfg(windows)]
#[path = "write_windows.rs"]
mod windows;

pub(crate) fn commit(workspace: &Workspace, edit: &Edit) -> Result<(), ()> {
    let current = read(workspace, &edit.relative)?;
    if current.as_deref() != (!edit.created).then_some(edit.old.as_str()) {
        return Err(());
    }
    platform_write(workspace, edit).map_err(|_| ())
}

#[cfg(windows)]
fn platform_write(workspace: &Workspace, edit: &Edit) -> std::io::Result<()> {
    windows::atomic_write(
        workspace.root(),
        workspace.root_identity().stable_bytes(),
        &workspace.root().join(&edit.relative),
        edit.new.as_bytes(),
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
        let current = read(workspace, &edit.relative).map_err(|_| refused())?;
        if current.as_deref() != (!edit.created).then_some(edit.old.as_str()) {
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
