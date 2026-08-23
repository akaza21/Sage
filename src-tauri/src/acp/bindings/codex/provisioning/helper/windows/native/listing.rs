use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use super::super::super::ElevatedHelperError;
use super::FILE_ATTRIBUTE_REPARSE_POINT;

pub(crate) fn remove_exact_directory_tree(path: &Path) -> Result<(), ElevatedHelperError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ElevatedHelperError::Cleanup)?;
    if !metadata.file_type().is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Cleanup);
    }
    let mut entries = 0;
    remove_directory_contents(path, 0, &mut entries)?;
    fs::remove_dir(path).map_err(|_| ElevatedHelperError::Cleanup)
}

fn remove_directory_contents(
    path: &Path,
    depth: usize,
    entries: &mut usize,
) -> Result<(), ElevatedHelperError> {
    const DELETE_DEPTH_MAX: usize = 8;
    const DELETE_ENTRIES_MAX: usize = 32;
    if depth > DELETE_DEPTH_MAX {
        return Err(ElevatedHelperError::Cleanup);
    }
    let available = DELETE_ENTRIES_MAX
        .checked_sub(*entries)
        .ok_or(ElevatedHelperError::Cleanup)?;
    let children = bounded_directory_entries(
        fs::read_dir(path).map_err(|_| ElevatedHelperError::Cleanup)?,
        available,
        ElevatedHelperError::Cleanup,
    )?;
    *entries = entries
        .checked_add(children.len())
        .ok_or(ElevatedHelperError::Cleanup)?;
    for entry in children {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ElevatedHelperError::Cleanup)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ElevatedHelperError::Cleanup);
        }
        if metadata.file_type().is_dir() {
            remove_directory_contents(&path, depth + 1, entries)?;
            fs::remove_dir(path).map_err(|_| ElevatedHelperError::Cleanup)?;
        } else if metadata.file_type().is_file() {
            fs::remove_file(path).map_err(|_| ElevatedHelperError::Cleanup)?;
        } else {
            return Err(ElevatedHelperError::Cleanup);
        }
    }
    Ok(())
}

pub(super) fn bounded_directory_entries<T, E>(
    entries: impl IntoIterator<Item = Result<T, E>>,
    maximum: usize,
    failure: ElevatedHelperError,
) -> Result<Vec<T>, ElevatedHelperError> {
    let mut bounded = Vec::with_capacity(maximum);
    for entry in entries {
        if bounded.len() == maximum {
            return Err(failure);
        }
        bounded.push(entry.map_err(|_| failure)?);
    }
    Ok(bounded)
}

pub(crate) fn bounded_names(
    root: &Path,
    maximum: usize,
    failure: ElevatedHelperError,
) -> Result<BTreeSet<OsString>, ElevatedHelperError> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|_| failure)? {
        let name = entry.map_err(|_| failure)?.file_name();
        if !names.insert(name) || names.len() > maximum {
            return Err(failure);
        }
    }
    Ok(names)
}
