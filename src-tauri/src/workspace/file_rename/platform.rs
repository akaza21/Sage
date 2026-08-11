use std::{fs::File, path::Path};

use crate::{
    domain::DirectoryListing,
    error::{AppError, AppResult},
    workspace::{RootIdentity, root_identity_path},
};

pub(super) struct ParentAuthority<'a> {
    pub(super) root: &'a Path,
    pub(super) expected_root: RootIdentity,
    pub(super) relative_path: &'a str,
}

pub(super) struct RenameObservation {
    pub(super) warning: Option<String>,
    pub(super) listing: AppResult<DirectoryListing>,
    pub(super) target_identity_confirmed: bool,
}

#[derive(Debug, Clone, Copy)]
struct RenameIdentityProof {
    root: RootIdentity,
    parent: RootIdentity,
    source: RootIdentity,
}

#[derive(Debug)]
pub(super) struct FileRenameAuthority {
    proof: RenameIdentityProof,
    directories: Vec<File>,
    source: File,
}

impl FileRenameAuthority {
    fn parent(&self) -> &File {
        self.directories
            .last()
            .expect("file rename authority always retains its root/parent")
    }
}

fn verify_visible_authority(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
) -> AppResult<()> {
    let parent = if parent_relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_relative)
    };
    if root_identity_path(root)? != authority.proof.root
        || root_identity_path(&parent)? != authority.proof.parent
    {
        return Err(AppError::InvalidRequest(
            "file rename path authority changed during reconciliation".into(),
        ));
    }
    Ok(())
}

fn observe_listing<L>(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
    list: L,
) -> AppResult<DirectoryListing>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    verify_visible_authority(authority, root, parent_relative)?;
    let listing = list()?;
    verify_visible_authority(authority, root, parent_relative)?;
    Ok(listing)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::{observe_recovery, rename_file_noreplace};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{observe_recovery, rename_file_noreplace};

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn rename_file_noreplace<C, P, S, L>(
    _authority: ParentAuthority<'_>,
    _source_name: &str,
    _target_name: &str,
    _committed: C,
    _preflight: P,
    _sync_parent: S,
    _list_after: L,
) -> AppResult<RenameObservation>
where
    C: FnOnce(std::sync::Arc<FileRenameAuthority>),
    P: FnOnce() -> AppResult<()>,
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    Err(AppError::InvalidRequest(
        "safe file rename is unavailable on this platform".into(),
    ))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn observe_recovery<L>(
    _authority: std::sync::Arc<FileRenameAuthority>,
    _root: &Path,
    _parent_relative: &str,
    _source_name: &str,
    _target_name: &str,
    _list_after: L,
) -> AppResult<RenameObservation>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    Err(AppError::InvalidRequest(
        "safe file rename recovery is unavailable on this platform".into(),
    ))
}
