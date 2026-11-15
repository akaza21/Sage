use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use sage_core::error::{AppError, AppResult};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
};

use crate::workspace::{root_identity_file, root_identity_path};
use sage_domain::{file_operations::TrashResourceKind, project::DirectoryListing};

use super::{
    ParentAuthority, ResourceTrashAuthority, ResourceTrashIdentity, TrashIdentityProof,
    TrashObservation,
};

mod progress_sink;

fn open_directory(path: &Path, allow_delete: bool) -> AppResult<File> {
    let share =
        FILE_SHARE_READ | FILE_SHARE_WRITE | if allow_delete { FILE_SHARE_DELETE } else { 0 };
    let access = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | if allow_delete { DELETE } else { 0 };
    let directory = OpenOptions::new()
        .access_mode(access)
        .share_mode(share)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "Recycle Bin path traverses a directory reparse point".into(),
        ));
    }
    Ok(directory)
}

fn open_parent(authority: ParentAuthority<'_>) -> AppResult<Vec<File>> {
    let root = open_directory(authority.root, false)?;
    if root_identity_file(&root)? != authority.expected_root {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before Recycle Bin operation".into(),
        ));
    }
    let mut directories = vec![root];
    let mut path = authority.root.to_path_buf();
    for component in Path::new(authority.relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(
                "Recycle Bin parent is not canonical".into(),
            ));
        };
        path.push(name);
        directories.push(open_directory(&path, false)?);
    }
    Ok(directories)
}

fn file_information(file: &File) -> AppResult<BY_HANDLE_FILE_INFORMATION> {
    let mut information = std::mem::MaybeUninit::uninit();
    // SAFETY: the file handle is live and the output points to writable storage.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: GetFileInformationByHandle initialized the output on success.
    Ok(unsafe { information.assume_init() })
}

fn open_source(path: &Path, kind: TrashResourceKind) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options
        .access_mode(FILE_READ_ATTRIBUTES | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if kind == TrashResourceKind::EmptyFolder {
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    let source = options.open(path)?;
    let metadata = source.metadata()?;
    let kind_matches = match kind {
        TrashResourceKind::File => metadata.is_file(),
        TrashResourceKind::EmptyFolder => metadata.is_dir(),
    };
    if !kind_matches || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "Recycle Bin source kind changed or is a reparse point".into(),
        ));
    }
    if kind == TrashResourceKind::File && file_information(&source)?.nNumberOfLinks != 1 {
        return Err(AppError::InvalidRequest(
            "files with hard-link aliases cannot be moved to the Recycle Bin".into(),
        ));
    }
    Ok(source)
}

fn parent_path(root: &Path, parent_relative: &str) -> PathBuf {
    if parent_relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_relative)
    }
}

fn open_authority(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
) -> AppResult<Arc<ResourceTrashAuthority>> {
    let root = parent_authority.root;
    let expected_root = parent_authority.expected_root;
    let parent_relative = parent_authority.relative_path;
    let directories = open_parent(parent_authority)?;
    let source_path = parent_path(root, parent_relative).join(source_name);
    let source = open_source(&source_path, kind)?;
    let authority = Arc::new(ResourceTrashAuthority {
        proof: TrashIdentityProof {
            root: expected_root,
            parent: root_identity_file(
                directories
                    .last()
                    .expect("Recycle Bin root/parent authority exists"),
            )?,
            source: root_identity_file(&source)?,
            kind,
        },
        source_path,
    });
    verify_visible_source(&authority)?;
    Ok(authority)
}

pub(in crate::workspace::resource_trash) fn observe_identity(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
) -> AppResult<ResourceTrashIdentity> {
    Ok(open_authority(parent_authority, source_name, kind)?.identity())
}

pub(super) fn verify_visible_authority(
    authority: &ResourceTrashAuthority,
    root: &Path,
    parent_relative: &str,
) -> AppResult<()> {
    let parent = parent_path(root, parent_relative);
    if root_identity_path(root)? != authority.proof.root
        || root_identity_path(&parent)? != authority.proof.parent
    {
        return Err(AppError::InvalidRequest(
            "Recycle Bin path authority changed during reconciliation".into(),
        ));
    }
    Ok(())
}

pub(super) fn verify_visible_source(authority: &ResourceTrashAuthority) -> AppResult<()> {
    let visible = open_source(&authority.source_path, authority.proof.kind)?;
    if root_identity_file(&visible)? != authority.proof.source {
        return Err(AppError::InvalidRequest(
            "Recycle Bin source identity changed before mutation".into(),
        ));
    }
    Ok(())
}

fn observe_listing<L>(
    authority: &ResourceTrashAuthority,
    root: &Path,
    parent_relative: &str,
    list_after: L,
) -> AppResult<DirectoryListing>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    verify_visible_authority(authority, root, parent_relative)?;
    let listing = list_after()?;
    verify_visible_authority(authority, root, parent_relative)?;
    Ok(listing)
}

fn source_absent(authority: &ResourceTrashAuthority) -> bool {
    matches!(
        open_source(&authority.source_path, authority.proof.kind),
        Err(AppError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
    )
}

pub(in crate::workspace::resource_trash) fn recycle_one<C, P, L>(
    parent_authority: ParentAuthority<'_>,
    source_name: &str,
    kind: TrashResourceKind,
    expected_identity: ResourceTrashIdentity,
    mutation_started: C,
    preflight: P,
    list_after: L,
) -> AppResult<TrashObservation>
where
    C: FnOnce(Arc<ResourceTrashAuthority>) + Send + 'static,
    P: FnOnce() -> AppResult<()>,
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    let root = parent_authority.root;
    let expected_root = parent_authority.expected_root;
    let parent_relative = parent_authority.relative_path;
    preflight()?;
    let authority = open_authority(
        ParentAuthority {
            root,
            expected_root,
            relative_path: parent_relative,
        },
        source_name,
        kind,
    )?;
    if authority.identity() != expected_identity {
        return Err(AppError::InvalidRequest(
            "Recycle Bin source identity changed after durable intent admission".into(),
        ));
    }
    verify_visible_source(&authority)?;
    progress_sink::recycle_on_sta(Arc::clone(&authority), mutation_started)?;
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(TrashObservation {
        warning: None,
        listing,
        source_absent_confirmed: source_absent(&authority),
    })
}

pub(in crate::workspace::resource_trash) fn observe_recovery<L>(
    authority: Arc<ResourceTrashAuthority>,
    root: &Path,
    parent_relative: &str,
    list_after: L,
) -> AppResult<TrashObservation>
where
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    verify_visible_authority(&authority, root, parent_relative)?;
    let source_absent_confirmed = source_absent(&authority);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(TrashObservation {
        warning: None,
        listing,
        source_absent_confirmed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn hard_link_alias_is_rejected_before_shell_mutation() {
        let root = std::env::temp_dir().join(format!("sage-trash-hardlink-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.txt");
        std::fs::write(&source, "preserve\n").unwrap();
        std::fs::hard_link(&source, root.join("alias.txt")).unwrap();

        let error = open_source(&source, TrashResourceKind::File).unwrap_err();

        assert!(error.to_string().contains("hard-link aliases"));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "preserve\n");
        std::fs::remove_dir_all(root).unwrap();
    }
}
