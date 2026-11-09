use crate::{
    domain::{DirectoryListing, ResourceRef},
    error::{AppError, AppResult},
};

use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{Workspace, join_contract_path, validate_contract_path};
pub(crate) use result::FileCreateCommit;
use result::{CreatedKind, finish_create};

pub(crate) const MAX_NEW_FILE_NAME_BYTES: usize = 255;
pub(crate) const MAX_NEW_FILE_PATH_BYTES: usize = 16 * 1024;

impl Workspace {
    #[cfg(test)]
    pub(crate) fn create_empty_file(
        &self,
        parent: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileCreateCommit> {
        self.create_empty_file_with(
            parent,
            basename,
            |_| Ok(()),
            |file| file.sync_all(),
            |workspace, resource| workspace.list_directory(resource),
        )
    }

    pub(crate) fn create_empty_file_observed(
        &self,
        parent: &ResourceRef,
        basename: &str,
        committed: Arc<AtomicBool>,
    ) -> AppResult<FileCreateCommit> {
        self.create_empty_file_with(
            parent,
            basename,
            |_| Ok(()),
            move |file| {
                committed.store(true, Ordering::Release);
                file.sync_all()
            },
            |workspace, resource| workspace.list_directory(resource),
        )
    }

    fn create_empty_file_with<B, S, L>(
        &self,
        parent: &ResourceRef,
        basename: &str,
        before_create: B,
        sync_file: S,
        list_after: L,
    ) -> AppResult<FileCreateCommit>
    where
        B: FnOnce(&Path) -> std::io::Result<()>,
        S: FnOnce(&std::fs::File) -> std::io::Result<()>,
        L: FnOnce(&Workspace, &ResourceRef) -> AppResult<DirectoryListing>,
    {
        let relative_path = validate_new_file_path(&parent.relative_path, basename)?;
        let parent_path = self.validate_resource(parent, true)?;
        let before = self.list_directory(parent)?;
        if before.truncated || before.entries.len() >= super::MAX_DIRECTORY_ENTRIES {
            return Err(AppError::ResourceLimit(format!(
                "directory is at the {}-entry Explorer creation limit",
                super::MAX_DIRECTORY_ENTRIES
            )));
        }
        let resource = ResourceRef {
            root_id: self.root_id().into(),
            relative_path,
        };
        let (sync_warning, after_listing, resource_confirmed) = platform::create_new(
            self.root(),
            self.root_identity(),
            &parent_path,
            basename,
            before_create,
            sync_file,
            || list_after(self, parent),
        )?;
        Ok(finish_create(
            before,
            basename,
            resource,
            CreatedKind::File,
            sync_warning,
            after_listing,
            resource_confirmed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn create_directory(
        &self,
        parent: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileCreateCommit> {
        self.create_directory_with(
            parent,
            basename,
            |_| Ok(()),
            |_| Ok(()),
            |workspace, resource| workspace.list_directory(resource),
            || {},
        )
    }

    pub(crate) fn create_directory_observed(
        &self,
        parent: &ResourceRef,
        basename: &str,
        committed: Arc<AtomicBool>,
    ) -> AppResult<FileCreateCommit> {
        self.create_directory_with(
            parent,
            basename,
            |_| Ok(()),
            sync_parent_directory,
            |workspace, resource| workspace.list_directory(resource),
            move || committed.store(true, Ordering::Release),
        )
    }

    fn create_directory_with<B, S, L, C>(
        &self,
        parent: &ResourceRef,
        basename: &str,
        before_create: B,
        sync_parent: S,
        list_after: L,
        committed: C,
    ) -> AppResult<FileCreateCommit>
    where
        B: FnOnce(&Path) -> std::io::Result<()>,
        S: FnOnce(&std::fs::File) -> std::io::Result<()>,
        L: FnOnce(&Workspace, &ResourceRef) -> AppResult<DirectoryListing>,
        C: FnOnce(),
    {
        let relative_path = validate_new_file_path(&parent.relative_path, basename)?;
        let parent_path = self.validate_resource(parent, true)?;
        let before = self.list_directory(parent)?;
        if before.truncated || before.entries.len() >= super::MAX_DIRECTORY_ENTRIES {
            return Err(AppError::ResourceLimit(format!(
                "directory is at the {}-entry Explorer creation limit",
                super::MAX_DIRECTORY_ENTRIES
            )));
        }
        let resource = ResourceRef {
            root_id: self.root_id().into(),
            relative_path,
        };
        let mut observation = platform::create_new_directory(
            platform::ParentAuthority {
                root: self.root(),
                expected_root: self.root_identity(),
                relative_path: &parent_path,
            },
            basename,
            before_create,
            committed,
            sync_parent,
            || list_after(self, parent),
        )?;
        let commit = finish_create(
            before,
            basename,
            resource,
            CreatedKind::Directory,
            observation.warning.take(),
            observation
                .listing
                .take()
                .expect("directory creation always records a listing result"),
            observation.resource_confirmed,
        );
        drop(observation);
        Ok(commit)
    }

    pub(crate) fn confirm_created_file(
        &self,
        parent: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileCreateCommit> {
        let relative_path = validate_new_file_path(&parent.relative_path, basename)?;
        let resource = ResourceRef {
            root_id: self.root_id().into(),
            relative_path,
        };
        let parent_listing = self.list_directory(parent)?;
        let confirmed = !parent_listing.truncated
            && parent_listing.entries.iter().any(|entry| {
                entry.resource == resource && entry.kind == "file" && entry.byte_len.is_some()
            });
        if !confirmed {
            return Err(AppError::InvalidRequest(
                "file creation outcome could not be confirmed; refresh Explorer before retrying"
                    .into(),
            ));
        }
        Ok(FileCreateCommit {
            resource,
            resource_confirmed: true,
            parent_listing,
            warning: Some(
                "The file exists, but the interrupted operation left its durability status unknown."
                    .into(),
            ),
        })
    }

    pub(crate) fn confirm_created_directory(
        &self,
        parent: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileCreateCommit> {
        let relative_path = validate_new_file_path(&parent.relative_path, basename)?;
        let resource = ResourceRef {
            root_id: self.root_id().into(),
            relative_path,
        };
        let parent_listing = self.list_directory(parent)?;
        let confirmed = !parent_listing.truncated
            && parent_listing.entries.iter().any(|entry| {
                entry.resource == resource && entry.kind == "directory" && entry.byte_len.is_none()
            });
        if !confirmed {
            return Err(AppError::InvalidRequest(
                "folder creation outcome could not be confirmed; refresh Explorer before retrying"
                    .into(),
            ));
        }
        Ok(FileCreateCommit {
            resource,
            resource_confirmed: true,
            parent_listing,
            warning: Some(
                "The folder exists, but the interrupted operation left its durability status unknown."
                    .into(),
            ),
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sync_parent_directory(parent: &std::fs::File) -> std::io::Result<()> {
    parent.sync_all()
}

#[cfg(windows)]
fn sync_parent_directory(_parent: &std::fs::File) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows does not provide a supported directory durability flush through this handle",
    ))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn sync_parent_directory(_parent: &std::fs::File) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "directory durability is unavailable on this platform",
    ))
}

pub(crate) fn validate_new_file_basename(basename: &str) -> AppResult<()> {
    if basename.is_empty() || basename.len() > MAX_NEW_FILE_NAME_BYTES {
        return Err(AppError::InvalidRequest(format!(
            "file name must be 1-{MAX_NEW_FILE_NAME_BYTES} UTF-8 bytes"
        )));
    }
    if basename == "."
        || basename == ".."
        || basename.chars().any(|character| {
            character < ' ' || character == '\u{7f}' || r#"<>:"/\|?*"#.contains(character)
        })
    {
        return Err(AppError::InvalidRequest(
            "file name contains a non-portable character".into(),
        ));
    }
    if basename.ends_with(['.', ' ']) {
        return Err(AppError::InvalidRequest(
            "file name cannot end with a dot or space".into(),
        ));
    }
    let stem = basename.split('.').next().unwrap_or_default();
    let folded = stem.to_ascii_lowercase();
    let reserved = matches!(folded.as_str(), "con" | "prn" | "aux" | "nul")
        || folded
            .strip_prefix("com")
            .or_else(|| folded.strip_prefix("lpt"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            });
    if reserved {
        return Err(AppError::InvalidRequest(
            "file name is reserved on Windows".into(),
        ));
    }
    Ok(())
}

/// The rules for a path Sage creates on the user's behalf, folders and all:
/// every name passes `validate_new_file_path`, and none names protected
/// workspace metadata such as `.git` or `node_modules`.
pub(crate) fn admit_created_path(relative_path: &str) -> AppResult<()> {
    let mut parent = String::new();
    for name in relative_path.split('/') {
        if super::directory_skip::protected_workspace_name(name) {
            return Err(AppError::InvalidRequest(format!(
                "files cannot be created inside {name}"
            )));
        }
        parent = validate_new_file_path(&parent, name)?;
    }
    Ok(())
}

pub(crate) fn validate_new_file_path(parent: &str, basename: &str) -> AppResult<String> {
    validate_new_file_basename(basename)?;
    if parent.len() > MAX_NEW_FILE_PATH_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "file path exceeds {MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes"
        )));
    }
    validate_contract_path(parent, true)?;
    let separator_bytes = usize::from(!parent.is_empty());
    if parent
        .len()
        .checked_add(separator_bytes)
        .and_then(|length| length.checked_add(basename.len()))
        .is_none_or(|length| length > MAX_NEW_FILE_PATH_BYTES)
    {
        return Err(AppError::ResourceLimit(format!(
            "file path exceeds {MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes"
        )));
    }
    let relative_path = join_contract_path(parent, basename);
    validate_contract_path(&relative_path, false)?;
    Ok(relative_path)
}

#[cfg(unix)]
fn ensure_root_identity(
    metadata: &std::fs::Metadata,
    expected: super::RootIdentity,
) -> AppResult<()> {
    if super::root_identity_metadata(metadata)? != expected {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before file creation".into(),
        ));
    }
    Ok(())
}

pub(super) mod platform;
mod result;

#[cfg(test)]
mod tests;
