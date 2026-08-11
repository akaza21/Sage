use std::{fs::File, path::Path};

use crate::{
    domain::DirectoryListing,
    error::{AppError, AppResult},
    workspace::RootIdentity,
};

#[cfg(windows)]
use crate::workspace::root_identity_path;

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

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct RenameIdentityProof {
    root: RootIdentity,
    parent: RootIdentity,
    source: RootIdentity,
}

#[cfg(windows)]
#[derive(Debug)]
pub(super) struct FileRenameAuthority {
    proof: RenameIdentityProof,
    directories: Vec<File>,
    source: File,
}

#[cfg(not(windows))]
#[derive(Debug)]
pub(super) struct FileRenameAuthority;

#[cfg(windows)]
impl FileRenameAuthority {
    fn parent(&self) -> &File {
        self.directories
            .last()
            .expect("file rename authority always retains its root/parent")
    }
}

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{observe_recovery, rename_file_noreplace};

#[cfg(not(windows))]
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
    let _ = (
        _authority.root,
        _authority.expected_root,
        _authority.relative_path,
    );
    Err(AppError::Unsupported(
        "safe closed-file rename currently requires Windows held-source authority".into(),
    ))
}

#[cfg(not(windows))]
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
    Err(AppError::Unsupported(
        "safe closed-file rename recovery currently requires Windows held-source authority".into(),
    ))
}
