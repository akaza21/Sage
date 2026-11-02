use std::{
    fs::{File, Metadata},
    path::PathBuf,
};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    workspace::{Workspace, validate_contract_path},
};

pub(super) struct ContainedFile {
    pub(super) file: File,
    pub(super) byte_len: u64,
}

pub(super) fn open_contained_file(
    workspace: &Workspace,
    resource: &ResourceRef,
) -> AppResult<ContainedFile> {
    if resource.root_id != workspace.root_id() {
        return Err(AppError::InvalidRequest(
            "search file root does not match its Workspace".into(),
        ));
    }
    validate_contract_path(&resource.relative_path, false)?;
    let path = workspace.root().join(&resource.relative_path);
    let file = File::open(&path)?;
    let byte_len = validate_opened_file(workspace, resource, &file)?;
    Ok(ContainedFile { file, byte_len })
}

fn validate_opened_file(
    workspace: &Workspace,
    resource: &ResourceRef,
    file: &File,
) -> AppResult<u64> {
    let handle_metadata = file.metadata()?;
    if !handle_metadata.is_file() {
        return Err(AppError::InvalidRequest(
            "search resource is not a regular file".into(),
        ));
    }
    if !handle_has_links(file, &handle_metadata)? {
        return Err(AppError::InvalidRequest(
            "search resource handle is unlinked".into(),
        ));
    }
    let handle_relative = handle_contract_relative(workspace, file)?;
    if handle_relative != resource.relative_path {
        return Err(AppError::InvalidRequest(
            "search resource handle does not match its root-relative identity".into(),
        ));
    }
    Ok(handle_metadata.len())
}

#[cfg(unix)]
fn handle_has_links(_file: &File, metadata: &Metadata) -> AppResult<bool> {
    use std::os::unix::fs::MetadataExt;

    Ok(metadata.nlink() > 0)
}

#[cfg(windows)]
fn handle_has_links(file: &File, _metadata: &Metadata) -> AppResult<bool> {
    use std::{mem::zeroed, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let succeeded = unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), std::ptr::addr_of_mut!(information))
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(information.nNumberOfLinks > 0)
}

#[cfg(not(any(unix, windows)))]
fn handle_has_links(_file: &File, _metadata: &Metadata) -> AppResult<bool> {
    Ok(false)
}

fn handle_contract_relative(workspace: &Workspace, file: &File) -> AppResult<String> {
    let path = handle_path(file)?;
    let relative = path.strip_prefix(workspace.root()).map_err(|_| {
        AppError::InvalidRequest("search file handle is outside the Workspace root".into())
    })?;
    let parts = relative
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => {
                value.to_str().map(str::to_owned).ok_or_else(|| {
                    AppError::InvalidRequest("search file handle path is not valid Unicode".into())
                })
            }
            _ => Err(AppError::InvalidRequest(
                "search file handle path is not root-relative".into(),
            )),
        })
        .collect::<AppResult<Vec<_>>>()?;
    let relative = parts.join("/");
    validate_contract_path(&relative, false)?;
    Ok(relative)
}

#[cfg(windows)]
fn handle_path(file: &File) -> AppResult<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let mut buffer = vec![0_u16; 32 * 1024];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| {
                AppError::InvalidRequest("search file handle path is too long".into())
            })?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| {
        AppError::InvalidRequest("search file handle path length is invalid".into())
    })?;
    if length == 0 || length >= buffer.len() {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

#[cfg(target_os = "linux")]
fn handle_path(file: &File) -> AppResult<PathBuf> {
    use std::os::fd::AsRawFd;

    Ok(std::fs::read_link(format!(
        "/proc/self/fd/{}",
        file.as_raw_fd()
    ))?)
}

#[cfg(target_os = "macos")]
fn handle_path(file: &File) -> AppResult<PathBuf> {
    use std::{ffi::OsString, os::fd::AsRawFd, os::unix::ffi::OsStringExt};

    const F_GETPATH: i32 = 50;
    const MAX_PATH_BYTES: usize = 1024;
    unsafe extern "C" {
        fn fcntl(file_descriptor: i32, command: i32, ...) -> i32;
    }

    let mut buffer = vec![0_u8; MAX_PATH_BYTES];
    let result = unsafe { fcntl(file.as_raw_fd(), F_GETPATH, buffer.as_mut_ptr()) };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    let length = buffer.iter().position(|byte| *byte == 0).ok_or_else(|| {
        AppError::InvalidRequest("search file handle path is not terminated".into())
    })?;
    if length == 0 {
        return Err(AppError::InvalidRequest(
            "search file handle path is empty".into(),
        ));
    }
    buffer.truncate(length);
    Ok(PathBuf::from(OsString::from_vec(buffer)))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn handle_path(_file: &File) -> AppResult<PathBuf> {
    Err(AppError::InvalidRequest(
        "search file handle containment is unsupported on this platform".into(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn handle_path(_file: &File) -> AppResult<PathBuf> {
    Err(AppError::InvalidRequest(
        "search file handle containment is unsupported on this platform".into(),
    ))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use uuid::Uuid;

    use super::*;

    fn fixture() -> (PathBuf, Workspace, ResourceRef) {
        let root = std::env::temp_dir().join(format!("sage-contained-file-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("inside.txt"), "contained\n").unwrap();
        let workspace = Workspace::open("project", "root", &root).unwrap();
        let resource = ResourceRef {
            root_id: "root".into(),
            relative_path: "inside.txt".into(),
        };
        (root, workspace, resource)
    }

    #[test]
    fn opens_the_validated_contained_handle() {
        let (root, workspace, resource) = fixture();
        let contained = open_contained_file(&workspace, &resource).unwrap();
        assert_eq!(contained.byte_len, 10);
        drop(contained);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_a_regular_file_replacement_after_opening() {
        let (root, workspace, resource) = fixture();
        let path = root.join("inside.txt");
        let file = File::open(&path).unwrap();
        fs::rename(&path, root.join("original.txt")).unwrap();
        fs::write(&path, "replacement\n").unwrap();

        assert!(validate_opened_file(&workspace, &resource, &file).is_err());
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_path_replaced_after_its_handle_was_opened() {
        use std::os::unix::fs::symlink;

        let (root, workspace, resource) = fixture();
        let outside = root.with_extension("outside.txt");
        fs::write(&outside, "outside secret\n").unwrap();
        let path = root.join("inside.txt");
        let file = File::open(&path).unwrap();
        fs::rename(&path, root.join("original.txt")).unwrap();
        symlink(&outside, &path).unwrap();

        assert!(validate_opened_file(&workspace, &resource, &file).is_err());
        drop(file);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn rejects_an_outside_handle_even_when_the_resource_path_is_inside() {
        let (root, workspace, resource) = fixture();
        let outside = root.with_extension("outside-handle.txt");
        fs::write(&outside, "outside secret\n").unwrap();
        let file = File::open(&outside).unwrap();

        assert!(validate_opened_file(&workspace, &resource, &file).is_err());
        drop(file);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn rejects_an_unlinked_open_handle() {
        let (root, workspace, resource) = fixture();
        let path = root.join("inside.txt");
        let file = File::open(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(validate_opened_file(&workspace, &resource, &file).is_err());
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_an_outside_link_handle_after_the_path_swaps_back_inside() {
        use std::os::unix::fs::symlink;

        let (root, workspace, resource) = fixture();
        let outside = root.with_extension("outside-swap.txt");
        fs::write(&outside, "outside secret\n").unwrap();
        let path = root.join("inside.txt");
        fs::remove_file(&path).unwrap();
        symlink(&outside, &path).unwrap();
        let file = File::open(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, "contained again\n").unwrap();

        assert!(validate_opened_file(&workspace, &resource, &file).is_err());
        drop(file);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_a_linked_file_when_windows_allows_test_symlinks() {
        use std::os::windows::fs::symlink_file;

        let (root, workspace, resource) = fixture();
        let outside = root.with_extension("outside.txt");
        fs::write(&outside, "outside secret\n").unwrap();
        fs::remove_file(root.join("inside.txt")).unwrap();
        if symlink_file(&outside, root.join("inside.txt")).is_ok() {
            assert!(open_contained_file(&workspace, &resource).is_err());
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }
}
