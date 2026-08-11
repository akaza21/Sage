use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
};

#[cfg(unix)]
use std::path::Path;

use super::{Workspace, join_contract_path, validate_contract_path};

pub(crate) const MAX_NEW_FILE_NAME_BYTES: usize = 255;
pub(crate) const MAX_NEW_FILE_PATH_BYTES: usize = 16 * 1024;

impl Workspace {
    pub(crate) fn create_empty_file(
        &self,
        parent: &ResourceRef,
        basename: &str,
    ) -> AppResult<ResourceRef> {
        let relative_path = validate_new_file_path(&parent.relative_path, basename)?;
        let parent_path = self.validate_resource(parent, true)?;
        let before = self.list_directory(parent)?;
        if before.truncated || before.entries.len() >= super::MAX_DIRECTORY_ENTRIES {
            return Err(AppError::ResourceLimit(format!(
                "directory is at the {}-entry Explorer creation limit",
                super::MAX_DIRECTORY_ENTRIES
            )));
        }
        platform::create_new(self.root(), self.root_identity(), &parent_path, basename)?;
        Ok(ResourceRef {
            root_id: self.root_id().into(),
            relative_path,
        })
    }
}

pub(crate) fn validate_new_file_basename(basename: &str) -> AppResult<()> {
    if basename.is_empty() || basename.len() > MAX_NEW_FILE_NAME_BYTES {
        return Err(AppError::InvalidRequest(format!(
            "file name must be 1-{MAX_NEW_FILE_NAME_BYTES} UTF-8 bytes"
        )));
    }
    if basename == "."
        || basename == ".."
        || basename.chars().any(|character| {
            character < ' ' || character == '\u{7f}' || r#"<>:"/\|?*"#.contains(character)
        })
    {
        return Err(AppError::InvalidRequest(
            "file name contains a non-portable character".into(),
        ));
    }
    if basename.ends_with(['.', ' ']) {
        return Err(AppError::InvalidRequest(
            "file name cannot end with a dot or space".into(),
        ));
    }
    let stem = basename.split('.').next().unwrap_or_default();
    let folded = stem.to_ascii_lowercase();
    let reserved = matches!(folded.as_str(), "con" | "prn" | "aux" | "nul")
        || folded
            .strip_prefix("com")
            .or_else(|| folded.strip_prefix("lpt"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if reserved {
        return Err(AppError::InvalidRequest(
            "file name is reserved on Windows".into(),
        ));
    }
    Ok(())
}

fn validate_new_file_path(parent: &str, basename: &str) -> AppResult<String> {
    validate_new_file_basename(basename)?;
    if parent.len() > MAX_NEW_FILE_PATH_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "file path exceeds {MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes"
        )));
    }
    validate_contract_path(parent, true)?;
    let separator_bytes = usize::from(!parent.is_empty());
    if parent
        .len()
        .checked_add(separator_bytes)
        .and_then(|length| length.checked_add(basename.len()))
        .is_none_or(|length| length > MAX_NEW_FILE_PATH_BYTES)
    {
        return Err(AppError::ResourceLimit(format!(
            "file path exceeds {MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes"
        )));
    }
    let relative_path = join_contract_path(parent, basename);
    validate_contract_path(&relative_path, false)?;
    Ok(relative_path)
}

#[cfg(unix)]
fn ensure_root_identity(
    metadata: &std::fs::Metadata,
    expected: super::RootIdentity,
) -> AppResult<()> {
    if super::root_identity_metadata(metadata)? != expected {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before file creation".into(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
mod platform {
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

    use crate::error::{AppError, AppResult};

    use super::ensure_root_identity;

    #[cfg(target_os = "macos")]
    const O_DIRECTORY: c_int = 0x10_0000;
    #[cfg(not(target_os = "macos"))]
    const O_DIRECTORY: c_int = 0x1_0000;
    #[cfg(target_os = "macos")]
    const O_NOFOLLOW: c_int = 0x100;
    #[cfg(not(target_os = "macos"))]
    const O_NOFOLLOW: c_int = 0x2_0000;
    #[cfg(target_os = "macos")]
    const O_CLOEXEC: c_int = 0x100_0000;
    #[cfg(not(target_os = "macos"))]
    const O_CLOEXEC: c_int = 0x8_0000;
    #[cfg(target_os = "macos")]
    const O_CREAT: c_int = 0x200;
    #[cfg(not(target_os = "macos"))]
    const O_CREAT: c_int = 0x40;
    #[cfg(target_os = "macos")]
    const O_EXCL: c_int = 0x800;
    #[cfg(not(target_os = "macos"))]
    const O_EXCL: c_int = 0x80;
    const O_RDONLY: c_int = 0;
    const O_WRONLY: c_int = 1;

    #[cfg(target_os = "macos")]
    type Mode = u16;
    #[cfg(not(target_os = "macos"))]
    type Mode = u32;

    unsafe extern "C" {
        fn openat(dirfd: c_int, path: *const c_char, flags: c_int, mode: Mode) -> c_int;
    }

    pub(super) fn create_new(
        root: &Path,
        expected_root: super::super::RootIdentity,
        parent: &str,
        basename: &str,
    ) -> AppResult<()> {
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(root)?;
        ensure_root_identity(&directory.metadata()?, expected_root)?;
        for component in Path::new(parent).components() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file parent is not canonical".into(),
                ));
            };
            let name = CString::new(name.as_encoded_bytes())
                .map_err(|_| AppError::InvalidRequest("file parent contains a null byte".into()))?;
            // SAFETY: the directory descriptor and NUL-terminated name remain valid for the call.
            let descriptor = unsafe {
                openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                    0,
                )
            };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: openat returned a new owned descriptor on success.
            directory = unsafe { File::from_raw_fd(descriptor) };
        }
        let basename = CString::new(basename)
            .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
        // SAFETY: the directory descriptor and NUL-terminated basename remain valid for the call.
        let descriptor = unsafe {
            openat(
                directory.as_raw_fd(),
                basename.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o666 as Mode,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: openat returned a new owned descriptor on success.
        unsafe { File::from_raw_fd(descriptor) }.sync_all()?;
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        os::windows::fs::{MetadataExt, OpenOptionsExt},
        path::{Component, Path},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    use crate::error::{AppError, AppResult};

    fn open_directory(path: &Path) -> AppResult<File> {
        let directory = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(AppError::InvalidRequest(
                "file parent traverses a reparse point".into(),
            ));
        }
        Ok(directory)
    }

    pub(super) fn create_new(
        root: &Path,
        expected_root: super::super::RootIdentity,
        parent: &str,
        basename: &str,
    ) -> AppResult<()> {
        let root_guard = open_directory(root)?;
        if super::super::root_identity_file(&root_guard)? != expected_root {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before file creation".into(),
            ));
        }
        let mut guards = vec![root_guard];
        let mut directory = root.to_path_buf();
        for component in Path::new(parent).components() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file parent is not canonical".into(),
                ));
            };
            directory.push(name);
            guards.push(open_directory(&directory)?);
        }
        let target = directory.join(basename);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(target)?;
        file.sync_all()?;
        drop(guards);
        Ok(())
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
mod platform {
    use std::path::Path;

    use crate::error::{AppError, AppResult};

    pub(super) fn create_new(
        _root: &Path,
        _expected_root: super::super::RootIdentity,
        _parent: &str,
        _basename: &str,
    ) -> AppResult<()> {
        Err(AppError::InvalidRequest(
            "safe file creation is unavailable on this platform".into(),
        ))
    }
}

#[cfg(test)]
mod tests;
