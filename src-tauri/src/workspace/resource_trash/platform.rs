#[cfg(not(windows))]
use std::sync::Arc;
use std::{fs::File, path::Path};

#[cfg(not(windows))]
use crate::error::AppError;
use crate::{
    domain::{DirectoryListing, TrashResourceKind},
    error::AppResult,
    workspace::RootIdentity,
};

pub(super) struct ParentAuthority<'a> {
    pub(super) root: &'a Path,
    pub(super) expected_root: RootIdentity,
    pub(super) relative_path: &'a str,
}

pub(super) struct TrashObservation {
    pub(super) warning: Option<String>,
    pub(super) listing: AppResult<DirectoryListing>,
    pub(super) source_absent_confirmed: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct TrashIdentityProof {
    root: RootIdentity,
    parent: RootIdentity,
    source: RootIdentity,
    kind: TrashResourceKind,
}

#[cfg(windows)]
#[derive(Debug)]
pub(super) struct ResourceTrashAuthority {
    proof: TrashIdentityProof,
    _directories: Vec<File>,
    _source: File,
    source_path: std::path::PathBuf,
}

#[cfg(not(windows))]
#[derive(Debug)]
pub(super) struct ResourceTrashAuthority;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{observe_recovery, recycle_one};

#[cfg(windows)]
pub(crate) const SUPPORTED: bool = true;
#[cfg(not(windows))]
pub(crate) const SUPPORTED: bool = false;

#[cfg(not(windows))]
pub(super) fn recycle_one<C, P, L>(
    authority: ParentAuthority<'_>,
    _source_name: &str,
    _kind: TrashResourceKind,
    _mutation_started: C,
    _preflight: P,
    _list_after: L,
) -> AppResult<TrashObservation>
where
    C: FnOnce(Arc<ResourceTrashAuthority>) + Send + 'static,
    P: FnOnce() -> AppResult<()>,
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    let _ = (
        authority.root,
        authority.expected_root,
        authority.relative_path,
    );
    Err(AppError::Unsupported(
        "recoverable resource trash currently requires the Windows Recycle Bin adapter".into(),
    ))
}

#[cfg(not(windows))]
pub(super) fn observe_recovery<L>(
    _authority: Arc<ResourceTrashAuthority>,
    _root: &Path,
    _parent_relative: &str,
    _list_after: L,
) -> AppResult<TrashObservation>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    Err(AppError::Unsupported(
        "recoverable resource trash reconciliation currently requires Windows".into(),
    ))
}
