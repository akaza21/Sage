use std::{fs::File, path::Path};

use crate::{
    domain::DirectoryListing,
    error::{AppError, AppResult},
    workspace::RootIdentity,
};

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use crate::workspace::root_identity_path;

pub(in crate::workspace) struct ParentAuthority<'a> {
    pub(in crate::workspace) root: &'a Path,
    pub(in crate::workspace) expected_root: RootIdentity,
    pub(in crate::workspace) relative_path: &'a str,
}

pub(super) struct RenameObservation {
    pub(super) warning: Option<String>,
    pub(super) listing: AppResult<DirectoryListing>,
    pub(super) target_identity_confirmed: bool,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct RenameIdentityProof {
    root: RootIdentity,
    parent: RootIdentity,
    source: RootIdentity,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
pub(super) struct FileRenameAuthority {
    proof: RenameIdentityProof,
    directories: Vec<File>,
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "held open so its identity cannot be reused")
    )]
    source: File,
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[derive(Debug)]
pub(super) struct FileRenameAuthority;

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
impl FileRenameAuthority {
    fn parent(&self) -> &File {
        self.directories
            .last()
            .expect("file rename authority always retains its root/parent")
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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
pub(in crate::workspace) use windows::open_parent;
#[cfg(windows)]
pub(super) use windows::{observe_recovery, rename_file_noreplace};
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::{observe_recovery, rename_file_noreplace};

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
    let _ = (
        _authority.root,
        _authority.expected_root,
        _authority.relative_path,
    );
    Err(AppError::Unsupported(
        "safe closed-file rename currently requires Windows held-source authority".into(),
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
    Err(AppError::Unsupported(
        "safe closed-file rename recovery currently requires Windows held-source authority".into(),
    ))
}
