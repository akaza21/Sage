use std::{fs::File, io::Read, path::Path};

#[cfg(test)]
pub(crate) mod test_hook;

use crate::{
    error::{AppError, AppResult},
    workspace::{RootIdentity, leaf_identity},
};

/// P-02's complete-source ceiling. Line selection happens only after admission.
pub(crate) const MAX_TEXT_FILE_BYTES: u64 =
    crate::acp::limits::preview::P02_READ_TEXT_SOURCE_BYTES as u64;
pub(crate) const TEXT_FILE_READ_PROBE_BYTES: u64 =
    crate::acp::limits::preview::P02_READ_TEXT_PROBE_BYTES as u64;

/// Opens a source exactly once beneath a held root authority. Metadata and the
/// bounded read must use this handle, never reopen a path validated earlier.
pub(crate) fn open_workspace_text_file(
    root: &Path,
    expected_root: RootIdentity,
    relative_path: &str,
    expected_leaf: Option<RootIdentity>,
) -> AppResult<File> {
    let file = platform::open_workspace_file(root, expected_root, relative_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::InvalidRequest(
            "resource is not a regular file".into(),
        ));
    }
    if let Some(expected_leaf) = expected_leaf
        && leaf_identity(&file, &metadata)? != expected_leaf
    {
        return Err(AppError::InvalidRequest(
            "workspace source identity changed before source read".into(),
        ));
    }
    Ok(file)
}

pub(crate) fn read_text_bytes_bounded(file: &mut File) -> AppResult<Vec<u8>> {
    #[cfg(test)]
    let guard = test_hook::lock();
    read_text_bytes_bounded_inner(
        file,
        #[cfg(test)]
        guard.as_deref(),
    )
}

fn read_text_bytes_bounded_inner(
    file: &mut File,
    #[cfg(test)] barrier: Option<&std::sync::Barrier>,
) -> AppResult<Vec<u8>> {
    // This is only a fast preflight; the same held handle is still probed at
    // cap + 1 below, so a file that grows after metadata cannot bypass bounds.
    if file.metadata()?.len() > MAX_TEXT_FILE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "file exceeds the {} MiB text-view limit",
            MAX_TEXT_FILE_BYTES / 1024 / 1024
        )));
    }
    #[cfg(test)]
    if let Some(barrier) = barrier {
        barrier.wait();
        // The writer completes its mutation before the second rendezvous.
        barrier.wait();
    }
    let mut bytes = Vec::new();
    file.take(TEXT_FILE_READ_PROBE_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "file exceeds the {} MiB text-view limit",
            MAX_TEXT_FILE_BYTES / 1024 / 1024
        )));
    }
    Ok(bytes)
}

pub(crate) fn decode_text_bytes(bytes: Vec<u8>) -> AppResult<String> {
    if bytes.contains(&0) {
        return Err(AppError::InvalidRequest(
            "binary files cannot be opened in the text view".into(),
        ));
    }
    String::from_utf8(bytes).map_err(|_| AppError::InvalidRequest("file is not valid UTF-8".into()))
}

/// Selects complete source lines using ACP's 1-based `line` and positive
/// `limit` semantics, retaining the original bytes (including CRLF and a final newline).
pub(crate) fn select_text_lines(
    content: &str,
    line: Option<u64>,
    limit: Option<u64>,
) -> AppResult<String> {
    let line = line.unwrap_or(1);
    if line == 0 || limit == Some(0) {
        return Err(AppError::InvalidRequest(
            "line and limit must be positive ACP integers".into(),
        ));
    }
    let start_line = usize::try_from(line - 1).map_err(|_| {
        AppError::InvalidRequest("line cannot be represented by this platform".into())
    })?;
    let mut starts = Vec::with_capacity(content.lines().count().saturating_add(1));
    starts.push(0);
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' && index + 1 < content.len() {
            starts.push(index + 1);
        }
    }
    let end_line = match limit {
        Some(value) => start_line
            .checked_add(usize::try_from(value).map_err(|_| {
                AppError::InvalidRequest("limit cannot be represented by this platform".into())
            })?)
            .ok_or_else(|| AppError::InvalidRequest("line and limit overflow ACP range".into()))?
            .min(starts.len()),
        None => starts.len(),
    };
    if start_line >= starts.len() {
        return Ok(String::new());
    }
    let end = starts.get(end_line).copied().unwrap_or(content.len());
    Ok(content[starts[start_line]..end].to_owned())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::*;
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            raw::{c_char, c_int},
            unix::fs::OpenOptionsExt,
        },
        path::{Component, Path},
    };

    #[cfg(target_os = "macos")]
    const O_DIRECTORY: c_int = 0x10_0000;
    #[cfg(target_os = "linux")]
    const O_DIRECTORY: c_int = 0x1_0000;
    #[cfg(target_os = "macos")]
    const O_NOFOLLOW: c_int = 0x100;
    #[cfg(target_os = "linux")]
    const O_NOFOLLOW: c_int = 0x2_0000;
    #[cfg(target_os = "macos")]
    const O_CLOEXEC: c_int = 0x100_0000;
    #[cfg(target_os = "linux")]
    const O_CLOEXEC: c_int = 0x8_0000;
    #[cfg(target_os = "macos")]
    const O_NONBLOCK: c_int = 0x4;
    #[cfg(target_os = "linux")]
    const O_NONBLOCK: c_int = 0x800;
    const O_RDONLY: c_int = 0;
    unsafe extern "C" {
        fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    }

    pub(super) fn open_workspace_file(
        root: &Path,
        expected_root: RootIdentity,
        relative_path: &str,
    ) -> AppResult<File> {
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(root)?;
        if crate::workspace::root_identity_metadata(&directory.metadata()?)? != expected_root {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before source read".into(),
            ));
        }
        let mut components = Path::new(relative_path).components().peekable();
        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file path is not canonical".into(),
                ));
            };
            let name = CString::new(name.as_encoded_bytes())
                .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
            let flags = if components.peek().is_some() {
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC
            } else {
                // A FIFO/device must not block before its same-handle file
                // type rejection below. Regular files retain ordinary reads.
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK
            };
            // SAFETY: directory is held, and name is an owned NUL-terminated component.
            let descriptor = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), flags) };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: openat returned a new owned descriptor.
            let opened = unsafe { File::from_raw_fd(descriptor) };
            if components.peek().is_some() {
                directory = opened;
            } else {
                return Ok(opened);
            }
        }
        Err(AppError::InvalidRequest("file path is empty".into()))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::workspace::root_identity_file;
    use std::{
        fs::{File, OpenOptions},
        os::windows::fs::{MetadataExt, OpenOptionsExt},
        path::{Component, Path},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fn open_no_reparse(path: &Path, directory: bool) -> AppResult<File> {
        let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
        if directory {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
        }
        let file = OpenOptions::new()
            .access_mode(FILE_READ_DATA | FILE_READ_ATTRIBUTES)
            // Deliberately omit FILE_SHARE_DELETE. While this handle is held,
            // Windows rejects rename/delete replacement of this component.
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(flags)
            .open(path)?;
        let metadata = file.metadata()?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(AppError::InvalidRequest(
                "workspace source traverses a reparse point".into(),
            ));
        }
        Ok(file)
    }

    pub(super) fn open_workspace_file(
        root: &Path,
        expected_root: RootIdentity,
        relative_path: &str,
    ) -> AppResult<File> {
        let root_guard = open_no_reparse(root, true)?;
        if root_identity_file(&root_guard)? != expected_root {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before source read".into(),
            ));
        }
        // Keep the root and every intermediate directory handle alive through
        // the final open. Each handle denies FILE_SHARE_DELETE, so Windows
        // cannot rename/delete-and-replace any traversed component while a
        // later pathname resolution is in progress. `OPEN_REPARSE_POINT` plus
        // the handle metadata check rejects links before descent. This is the
        // Windows equivalent of the Unix retained-parent openat walk: every
        // pathname prefix is pinned by a no-delete handle, rather than merely
        // validating a root and reopening an unprotected path.
        let mut path = root.to_path_buf();
        let mut guards = vec![root_guard];
        let mut parts = Path::new(relative_path).components().peekable();
        while let Some(component) = parts.next() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file path is not canonical".into(),
                ));
            };
            path.push(name);
            let directory = parts.peek().is_some();
            let opened = open_no_reparse(&path, directory)?;
            if directory {
                guards.push(opened);
            } else {
                return Ok(opened);
            }
        }
        Err(AppError::InvalidRequest("file path is empty".into()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use uuid::Uuid;

        #[test]
        fn held_no_delete_root_and_intermediate_handles_reject_real_replacement() {
            let root = std::env::temp_dir().join(format!("sage-p02-win-pin-{}", Uuid::new_v4()));
            let child = root.join("child");
            std::fs::create_dir_all(&child).unwrap();
            let root_guard = open_no_reparse(&root, true).unwrap();
            let child_guard = open_no_reparse(&child, true).unwrap();
            // Actual replacement attempts while held handles deny delete.
            assert!(std::fs::rename(&root, root.with_extension("replacement")).is_err());
            assert!(std::fs::rename(&child, root.join("child-replacement")).is_err());
            drop(child_guard);
            drop(root_guard);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;
    pub(super) fn open_workspace_file(
        _root: &Path,
        _expected_root: RootIdentity,
        _relative_path: &str,
    ) -> AppResult<File> {
        Err(AppError::InvalidRequest(
            "safe source reads are unavailable on this platform".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nul_anywhere_in_the_bounded_source() {
        let mut bytes = vec![b'x'; 8_193];
        bytes[8_192] = 0;
        assert!(matches!(
            decode_text_bytes(bytes),
            Err(AppError::InvalidRequest(_))
        ));
    }
    #[test]
    fn rejects_unrepresentable_or_overflowing_ranges_instead_of_eof() {
        assert!(matches!(
            select_text_lines("one\n", Some(u64::MAX), Some(u64::MAX)),
            Err(AppError::InvalidRequest(_))
        ));
    }
}

#[cfg(test)]
#[test]
fn decode_text_bytes_rejects_invalid_utf8() {
    assert!(decode_text_bytes(vec![0xff, 0xfe]).is_err());
}
