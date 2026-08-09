use std::{fs, fs::File, path::Path};

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
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(AppError::InvalidRequest(
            "search does not follow linked files".into(),
        ));
    }
    let file = File::open(&path)?;
    let byte_len = validate_opened_file(workspace, resource, &path, &file)?;
    Ok(ContainedFile { file, byte_len })
}

fn validate_opened_file(
    workspace: &Workspace,
    resource: &ResourceRef,
    path: &Path,
    file: &File,
) -> AppResult<u64> {
    let handle_metadata = file.metadata()?;
    if !handle_metadata.is_file() {
        return Err(AppError::InvalidRequest(
            "search resource is not a regular file".into(),
        ));
    }
    let canonical = workspace.canonical_resource_identity(resource, false)?;
    if &canonical != resource {
        return Err(AppError::InvalidRequest(
            "search resource identity changed while opening".into(),
        ));
    }
    if !same_file_identity(file, path)? {
        return Err(AppError::InvalidRequest(
            "search resource was replaced while opening".into(),
        ));
    }
    Ok(handle_metadata.len())
}

#[cfg(unix)]
fn same_file_identity(file: &File, path: &Path) -> AppResult<bool> {
    use std::os::unix::fs::MetadataExt;

    let left = file.metadata()?;
    let right = fs::metadata(path)?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(windows)]
fn same_file_identity(file: &File, path: &Path) -> AppResult<bool> {
    let current = File::open(path)?;
    Ok(windows_file_identity(file)? == windows_file_identity(&current)?)
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> AppResult<(u32, u64)> {
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
    let index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok((information.dwVolumeSerialNumber, index))
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_file: &File, _path: &Path) -> AppResult<bool> {
    Ok(false)
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

        assert!(validate_opened_file(&workspace, &resource, &path, &file).is_err());
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

        assert!(validate_opened_file(&workspace, &resource, &path, &file).is_err());
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
