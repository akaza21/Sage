use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    domain::{DirectoryListing, ResourceRef},
    error::{AppError, AppResult},
};

use super::{Workspace, file_create::validate_new_file_path};

#[derive(Debug)]
pub(crate) struct FileRenameCommit {
    pub(crate) source: ResourceRef,
    pub(crate) target: ResourceRef,
    pub(crate) resource_confirmed: bool,
    pub(crate) parent_listing: Option<DirectoryListing>,
    pub(crate) warning: Option<String>,
}

impl Workspace {
    pub(crate) fn rename_file_target(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<ResourceRef> {
        if source.root_id != self.root_id() {
            return Err(AppError::InvalidRequest(
                "resource root does not match the resolved project root".into(),
            ));
        }
        super::validate_contract_path(&source.relative_path, false)?;
        let parent_path = source
            .relative_path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let target_path = validate_new_file_path(parent_path, basename)?;
        if target_path == source.relative_path {
            return Err(AppError::InvalidRequest(
                "file rename target must differ from the source".into(),
            ));
        }
        Ok(ResourceRef {
            root_id: self.root_id().into(),
            relative_path: target_path,
        })
    }

    #[cfg(test)]
    pub(crate) fn rename_file(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileRenameCommit> {
        self.rename_file_with(source, basename, Arc::new(AtomicBool::new(false)))
    }

    pub(crate) fn rename_file_observed(
        &self,
        source: &ResourceRef,
        basename: &str,
        committed: Arc<AtomicBool>,
    ) -> AppResult<FileRenameCommit> {
        self.rename_file_with(source, basename, committed)
    }

    fn rename_file_with(
        &self,
        source: &ResourceRef,
        basename: &str,
        committed: Arc<AtomicBool>,
    ) -> AppResult<FileRenameCommit> {
        let source_path = self.validate_resource(source, false)?;
        let (parent_path, source_name) = source_path
            .rsplit_once('/')
            .map_or(("", source_path.as_str()), |(parent, name)| (parent, name));
        let target = self.rename_file_target(source, basename)?;
        let parent = ResourceRef {
            root_id: self.root_id().into(),
            relative_path: parent_path.into(),
        };
        let before = self.list_directory(&parent)?;
        if before.truncated {
            return Err(AppError::ResourceLimit(
                "file rename requires a complete parent listing".into(),
            ));
        }
        let source_is_regular_file = before.entries.iter().any(|entry| {
            entry.name == source_name && entry.resource == *source && entry.kind == "file"
        });
        if !source_is_regular_file {
            return Err(AppError::InvalidRequest(
                "file rename source must be one exact regular file".into(),
            ));
        }
        let observation = platform::rename_file_noreplace(
            platform::ParentAuthority {
                root: self.root(),
                expected_root: self.root_identity(),
                relative_path: parent_path,
            },
            source_name,
            basename,
            move || committed.store(true, Ordering::Release),
            sync_parent_directory,
            || self.list_directory(&parent),
        )?;
        let parent_listing = observation.listing.ok().filter(|listing| {
            !listing.truncated
                && listing.entries.iter().any(|entry| {
                    entry.resource == target && entry.kind == "file" && entry.byte_len.is_some()
                })
                && !listing
                    .entries
                    .iter()
                    .any(|entry| entry.resource == *source)
        });
        let resource_confirmed = observation.target_identity_confirmed && parent_listing.is_some();
        let refresh_warning = (!resource_confirmed).then(|| {
            "File rename completed under held authority, but the requested path identity could not be confirmed; Explorer is refreshing."
                .to_owned()
        });
        let warning = combine_warnings(observation.warning, refresh_warning);
        drop(observation.guards);
        Ok(FileRenameCommit {
            source: source.clone(),
            target,
            resource_confirmed,
            parent_listing,
            warning,
        })
    }

    pub(crate) fn confirm_renamed_file(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileRenameCommit> {
        let target = self.rename_file_target(source, basename)?;
        let parent_path = source
            .relative_path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let parent = ResourceRef {
            root_id: self.root_id().into(),
            relative_path: parent_path.into(),
        };
        let listing = self.list_directory(&parent)?;
        if listing.truncated {
            return Err(AppError::ResourceLimit(
                "file rename recovery requires a complete parent listing".into(),
            ));
        }
        let resource_confirmed = listing.entries.iter().any(|entry| {
            entry.resource == target && entry.kind == "file" && entry.byte_len.is_some()
        }) && !listing
            .entries
            .iter()
            .any(|entry| entry.resource == *source);
        Ok(FileRenameCommit {
            source: source.clone(),
            target,
            resource_confirmed,
            parent_listing: Some(listing),
            warning: Some(
                "File rename outcome was interrupted; Explorer and the durable layout were recovered from authoritative state."
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
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "directory durability is unavailable on this platform",
    ))
}

fn combine_warnings(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        (left, right) => left.or(right),
    }
}

mod platform;

#[cfg(test)]
mod tests;
