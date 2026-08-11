use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    domain::{DirectoryListing, ResourceRef, TrashResourceKind},
    error::{AppError, AppResult},
};

use super::Workspace;

pub(crate) use platform::ResourceTrashIdentity;

pub(crate) fn supported() -> bool {
    platform::SUPPORTED
}

#[derive(Debug)]
pub(crate) struct ResourceTrashCommit {
    pub(crate) source: ResourceRef,
    pub(crate) parent: ResourceRef,
    pub(crate) resource_confirmed: bool,
    pub(crate) parent_listing: Option<DirectoryListing>,
    pub(crate) warning: Option<String>,
}

impl ResourceTrashCommit {
    pub(crate) fn unconfirmed(
        source: ResourceRef,
        parent: ResourceRef,
        warning: impl Into<String>,
    ) -> Self {
        Self {
            source,
            parent,
            resource_confirmed: false,
            parent_listing: None,
            warning: Some(warning.into()),
        }
    }
}

#[derive(Default)]
pub(crate) struct ResourceTrashMarker {
    mutation_started: AtomicBool,
    authority: Mutex<Option<Arc<platform::ResourceTrashAuthority>>>,
}

impl ResourceTrashMarker {
    fn retain(&self, authority: Arc<platform::ResourceTrashAuthority>) {
        *self
            .authority
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(authority);
    }

    pub(crate) fn record_mutation_started(&self) {
        self.mutation_started.store(true, Ordering::Release);
    }

    pub(crate) fn mutation_started(&self) -> bool {
        self.mutation_started.load(Ordering::Acquire)
    }

    fn authority(&self) -> Option<Arc<platform::ResourceTrashAuthority>> {
        self.authority
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Workspace {
    pub(crate) fn resource_trash_identity(
        &self,
        source: &ResourceRef,
        kind: TrashResourceKind,
    ) -> AppResult<ResourceTrashIdentity> {
        let parent = self.preflight_resource_trash(source, kind)?;
        let source_name = source
            .relative_path
            .rsplit_once('/')
            .map_or(source.relative_path.as_str(), |(_, name)| name);
        platform::observe_identity(
            platform::ParentAuthority {
                root: self.root(),
                expected_root: self.root_identity(),
                relative_path: &parent.relative_path,
            },
            source_name,
            kind,
        )
    }

    pub(crate) fn preflight_resource_trash(
        &self,
        source: &ResourceRef,
        kind: TrashResourceKind,
    ) -> AppResult<ResourceRef> {
        let source_path = self.validate_resource(source, false)?;
        let (parent_path, source_name) = source_path
            .rsplit_once('/')
            .map_or(("", source_path.as_str()), |(parent, name)| (parent, name));
        let parent = ResourceRef {
            root_id: self.root_id().into(),
            relative_path: parent_path.into(),
        };
        let parent_listing = self.list_directory(&parent)?;
        if parent_listing.truncated {
            return Err(AppError::ResourceLimit(
                "moving a resource to the Recycle Bin requires a complete parent listing".into(),
            ));
        }
        let expected_kind = match kind {
            TrashResourceKind::File => "file",
            TrashResourceKind::EmptyFolder => "directory",
        };
        if !parent_listing.entries.iter().any(|entry| {
            entry.name == source_name && entry.resource == *source && entry.kind == expected_kind
        }) {
            return Err(AppError::InvalidRequest(format!(
                "Recycle Bin source must be one exact {expected_kind}"
            )));
        }
        if kind == TrashResourceKind::EmptyFolder {
            let source_listing = self.list_directory(source)?;
            if source_listing.truncated {
                return Err(AppError::ResourceLimit(
                    "moving a folder to the Recycle Bin requires a complete source listing".into(),
                ));
            }
            if !source_listing.entries.is_empty() {
                return Err(AppError::InvalidRequest(
                    "only empty folders can be moved to the Recycle Bin".into(),
                ));
            }
            if std::fs::read_dir(self.root().join(&source_path))?
                .next()
                .transpose()?
                .is_some()
            {
                return Err(AppError::InvalidRequest(
                    "only physically empty folders can be moved to the Recycle Bin".into(),
                ));
            }
        }
        Ok(parent)
    }

    pub(crate) fn trash_resource_observed(
        &self,
        source: &ResourceRef,
        kind: TrashResourceKind,
        expected_identity: ResourceTrashIdentity,
        marker: Arc<ResourceTrashMarker>,
    ) -> AppResult<ResourceTrashCommit> {
        let source_path = super::validate_contract_path(&source.relative_path, false)
            .map(|()| source.relative_path.clone())?;
        if source.root_id != self.root_id() {
            return Err(AppError::InvalidRequest(
                "Recycle Bin source root does not match the resolved Project root".into(),
            ));
        }
        let (parent_path, source_name) = source_path
            .rsplit_once('/')
            .map_or(("", source_path.as_str()), |(parent, name)| (parent, name));
        let parent = ResourceRef {
            root_id: self.root_id().into(),
            relative_path: parent_path.into(),
        };
        let observed_marker = Arc::clone(&marker);
        let observation = platform::recycle_one(
            platform::ParentAuthority {
                root: self.root(),
                expected_root: self.root_identity(),
                relative_path: parent_path,
            },
            source_name,
            kind,
            expected_identity,
            move |authority| {
                observed_marker.retain(authority);
                observed_marker.record_mutation_started();
            },
            || self.preflight_resource_trash(source, kind).map(drop),
            || self.list_directory(&parent),
        )?;
        Ok(commit_from_observation(source, parent, observation))
    }

    pub(crate) fn confirm_trashed_resource(
        &self,
        source: &ResourceRef,
        marker: &ResourceTrashMarker,
    ) -> AppResult<ResourceTrashCommit> {
        let parent_path = source
            .relative_path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let parent = ResourceRef {
            root_id: source.root_id.clone(),
            relative_path: parent_path.into(),
        };
        let authority = marker.authority().ok_or_else(|| {
            AppError::InvalidRequest("Recycle Bin recovery authority is unavailable".into())
        })?;
        let observation = platform::observe_recovery(authority, self.root(), parent_path, || {
            self.list_directory(&parent)
        })?;
        let mut commit = commit_from_observation(source, parent, observation);
        commit.warning = Some(
            "Recycle Bin outcome was interrupted; Explorer and the durable layout were reconciled from retained filesystem authority."
                .into(),
        );
        Ok(commit)
    }
}

fn commit_from_observation(
    source: &ResourceRef,
    parent: ResourceRef,
    observation: platform::TrashObservation,
) -> ResourceTrashCommit {
    let parent_listing = observation.listing.ok().filter(|listing| {
        !listing.truncated
            && !listing
                .entries
                .iter()
                .any(|entry| entry.resource == *source)
    });
    let resource_confirmed = observation.source_absent_confirmed && parent_listing.is_some();
    let warning = observation.warning.or_else(|| {
        (!resource_confirmed).then(|| {
            "Recycle Bin operation may have changed the filesystem, but source absence could not be confirmed; Explorer is refreshing."
                .to_owned()
        })
    });
    ResourceTrashCommit {
        source: source.clone(),
        parent,
        resource_confirmed,
        parent_listing,
        warning,
    }
}

mod platform;

#[cfg(test)]
mod tests;
