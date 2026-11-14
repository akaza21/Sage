//! The macOS system Trash through `NSFileManager`, the call Finder-aware apps
//! (and VS Code, through Electron) use. It records where the item came from,
//! so Finder can offer Put Back, and moves the item within its volume, so it
//! keeps its inode. The call takes a path, so the visible path is re-proven
//! just before it and the item that arrived in the Trash is checked against
//! the held source just after it. A different item is reported, not moved:
//! Finder's Put Back returns it to where it really came from.

use std::{
    ffi::{CStr, CString},
    os::unix::ffi::OsStrExt,
    sync::Arc,
};

use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{NSFileManager, NSString, NSURL};
use sage_core::error::{AppError, AppResult};

use crate::workspace::root_identity_metadata;

use super::{ResourceTrashAuthority, verify_visible_source};

/// Trashes `path` and returns where the item went, or the system's reason.
fn trash_path(path: &str) -> Result<Option<CString>, String> {
    autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let mut resulting: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
            .map_err(|error| error.localizedDescription().to_string())?;
        Ok(resulting.map(|url| {
            // SAFETY: the representation is a NUL-terminated buffer that lives
            // as long as the URL, and it is copied before the URL is released.
            unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }.to_owned()
        }))
    })
}

pub(super) fn move_to_trash<C>(
    authority: &Arc<ResourceTrashAuthority>,
    mutation_started: C,
) -> AppResult<Option<String>>
where
    C: FnOnce(Arc<ResourceTrashAuthority>),
{
    let path = authority
        .source_path
        .to_str()
        .ok_or_else(|| AppError::InvalidRequest("Trash source path is not UTF-8".into()))?;
    verify_visible_source(authority)?;
    // The system call is not atomic, so the filesystem may change from here.
    mutation_started(Arc::clone(authority));
    let trashed = match trash_path(path) {
        Ok(trashed) => trashed,
        // The system may have refused before or after changing anything, so
        // its reason travels as the unconfirmed outcome's warning.
        Err(reason) => {
            return Ok(Some(format!(
                "macOS could not move the item to the Trash: {}. Explorer is refreshing.",
                reason.trim_end_matches('.')
            )));
        }
    };
    let Some(trashed) = trashed else {
        return Ok(Some(
            "macOS did not report where the item went in the Trash; Explorer is refreshing.".into(),
        ));
    };
    match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(trashed.as_bytes()))
        .map_err(AppError::from)
        .and_then(|metadata| root_identity_metadata(&metadata))
    {
        Ok(arrived) if arrived == authority.proof.source => Ok(None),
        // Finder's Put Back knows where a different item came from, so it is
        // left in the Trash rather than moved into the Project.
        Ok(_) => Ok(Some(
            "The item that reached the Trash is not the one you chose, so Sage left it there; Put Back in Finder returns it. Explorer is refreshing."
                .into(),
        )),
        Err(_) => Ok(Some(
            "The item in the Trash could not be checked; Explorer is refreshing.".into(),
        )),
    }
}
