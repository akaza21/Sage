use std::path::Path;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use std::sync::Arc;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use sage_core::error::AppError;
use sage_core::error::AppResult;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use super::TRASH_NAME;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use crate::workspace::root_identity_path;
use crate::{
    domain::{DirectoryListing, TrashResourceKind},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceTrashIdentity {
    root: RootIdentity,
    parent: RootIdentity,
    source: RootIdentity,
}

impl ResourceTrashIdentity {
    pub(crate) fn component_bytes(self) -> ([u8; 24], [u8; 24], [u8; 24]) {
        (
            self.root.stable_bytes(),
            self.parent.stable_bytes(),
            self.source.stable_bytes(),
        )
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
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
    source_path: std::path::PathBuf,
}

#[cfg(windows)]
impl ResourceTrashAuthority {
    fn identity(&self) -> ResourceTrashIdentity {
        ResourceTrashIdentity {
            root: self.proof.root,
            parent: self.proof.parent,
            source: self.proof.source,
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[derive(Debug)]
pub(super) struct ResourceTrashAuthority;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::{observe_identity, observe_recovery, recycle_one};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::{ResourceTrashAuthority, observe_identity, observe_recovery, recycle_one};

pub(crate) const SUPPORTED: bool = cfg!(any(windows, target_os = "linux", target_os = "macos"));

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn recycle_one<C, P, L>(
    authority: ParentAuthority<'_>,
    _source_name: &str,
    _kind: TrashResourceKind,
    _expected_identity: ResourceTrashIdentity,
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

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn observe_identity(
    authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
) -> AppResult<ResourceTrashIdentity> {
    let parent = if authority.relative_path.is_empty() {
        authority.root.to_owned()
    } else {
        authority.root.join(authority.relative_path)
    };
    let source = parent.join(source_name);
    let metadata = std::fs::symlink_metadata(&source)?;
    let kind_matches = match kind {
        TrashResourceKind::File => metadata.is_file(),
        TrashResourceKind::EmptyFolder => metadata.is_dir(),
    };
    if metadata.file_type().is_symlink() || !kind_matches {
        return Err(AppError::InvalidRequest(format!(
            "{TRASH_NAME} source kind changed or is a symbolic link"
        )));
    }
    let identity = ResourceTrashIdentity {
        root: root_identity_path(authority.root)?,
        parent: root_identity_path(&parent)?,
        source: root_identity_path(&source)?,
    };
    if identity.root != authority.expected_root {
        return Err(AppError::InvalidRequest(format!(
            "workspace root identity changed before {TRASH_NAME} admission"
        )));
    }
    Ok(identity)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
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
