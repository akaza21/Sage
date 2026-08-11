use crate::{
    domain::{DirectoryListing, ResourceRef, WorkspaceEntry},
    error::{AppError, AppResult},
};

use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{Workspace, join_contract_path, validate_contract_path};

pub(crate) const MAX_NEW_FILE_NAME_BYTES: usize = 255;
pub(crate) const MAX_NEW_FILE_PATH_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub(crate) struct FileCreateCommit {
    pub(crate) resource: ResourceRef,
    pub(crate) resource_confirmed: bool,
    pub(crate) parent_listing: DirectoryListing,
    pub(crate) warning: Option<String>,
}

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
        let (parent_listing, refresh_warning) = match (resource_confirmed, after_listing) {
            (true, Ok(listing))
                if !listing.truncated
                    && listing.entries.iter().any(|entry| {
                        entry.resource == resource
                            && entry.kind == "file"
                            && entry.byte_len.is_some()
                    }) =>
            {
                (listing, None)
            }
            (true, _) => (
                reconciled_listing(before, basename, resource.clone()),
                Some(
                    "Explorer refresh could not be confirmed; the created file was inserted into the last authoritative parent listing."
                        .to_owned(),
                ),
            ),
            (false, Ok(listing)) if !listing.truncated => (
                listing,
                Some(
                    "File creation completed under the held parent, but the requested path identity is unknown; Explorer is refreshing."
                        .to_owned(),
                ),
            ),
            (false, _) => (
                before,
                Some(
                    "File creation completed under the held parent, but the requested path identity is unknown; Explorer is refreshing."
                        .to_owned(),
                ),
            ),
        };
        Ok(FileCreateCommit {
            resource,
            resource_confirmed,
            parent_listing,
            warning: combine_warnings(sync_warning, refresh_warning),
        })
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
}

fn reconciled_listing(
    mut before: DirectoryListing,
    basename: &str,
    resource: ResourceRef,
) -> DirectoryListing {
    before.entries.push(WorkspaceEntry {
        name: basename.into(),
        resource,
        kind: "file".into(),
        byte_len: Some(0),
    });
    before.entries.sort_unstable_by(|left, right| {
        let rank = |kind: &str| u8::from(kind != "directory");
        rank(&left.kind)
            .cmp(&rank(&right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
    before
}

fn combine_warnings(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        (left, right) => left.or(right),
    }
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

fn validate_new_file_path(parent: &str, basename: &str) -> AppResult<String> {
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

mod platform;

#[cfg(test)]
mod tests;
