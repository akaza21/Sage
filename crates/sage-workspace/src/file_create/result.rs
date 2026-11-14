use sage_core::error::AppResult;

use sage_domain::{
    project::{DirectoryListing, WorkspaceEntry},
    resource::ResourceRef,
};

#[derive(Debug)]
pub struct FileCreateCommit {
    pub resource: ResourceRef,
    pub resource_confirmed: bool,
    pub parent_listing: DirectoryListing,
    pub warning: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum CreatedKind {
    File,
    Directory,
}

impl CreatedKind {
    fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }

    fn byte_len(self) -> Option<u64> {
        match self {
            Self::File => Some(0),
            Self::Directory => None,
        }
    }
}

pub(super) fn finish_create(
    before: DirectoryListing,
    basename: &str,
    resource: ResourceRef,
    kind: CreatedKind,
    sync_warning: Option<String>,
    after_listing: AppResult<DirectoryListing>,
    resource_confirmed: bool,
) -> FileCreateCommit {
    let label = match kind {
        CreatedKind::File => "File",
        CreatedKind::Directory => "Folder",
    };
    let valid_entry = |listing: &DirectoryListing| {
        !listing.truncated
            && listing.entries.iter().any(|entry| {
                entry.resource == resource
                    && entry.kind == kind.label()
                    && entry.byte_len == kind.byte_len()
            })
    };
    let (parent_listing, refresh_warning) = match (resource_confirmed, after_listing) {
        (true, Ok(listing)) if valid_entry(&listing) => (listing, None),
        (true, _) => (
            reconciled_listing(before, basename, resource.clone(), kind),
            Some(format!(
                "Explorer refresh could not be confirmed; the created {} was inserted into the last authoritative parent listing.",
                kind.label()
            )),
        ),
        (false, Ok(listing)) if !listing.truncated => (
            listing,
            Some(format!(
                "{label} creation completed under the held parent, but the requested path identity is unknown; Explorer is refreshing."
            )),
        ),
        (false, _) => (
            before,
            Some(format!(
                "{label} creation completed under the held parent, but the requested path identity is unknown; Explorer is refreshing."
            )),
        ),
    };
    FileCreateCommit {
        resource,
        resource_confirmed,
        parent_listing,
        warning: combine_warnings(sync_warning, refresh_warning),
    }
}

fn reconciled_listing(
    mut before: DirectoryListing,
    basename: &str,
    resource: ResourceRef,
    kind: CreatedKind,
) -> DirectoryListing {
    before.entries.push(WorkspaceEntry {
        name: basename.into(),
        resource,
        kind: kind.label().into(),
        byte_len: kind.byte_len(),
        // Used only when the fresh listing failed; the next listing brings
        // the ignore state.
        ignored: false,
    });
    before.entries.sort_unstable_by(|left, right| {
        use crate::entry_rank;
        entry_rank(&left.kind)
            .cmp(&entry_rank(&right.kind))
            .then_with(|| crate::name_order::explorer_name_cmp(&left.name, &right.name))
    });
    before
}

fn combine_warnings(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        (left, right) => left.or(right),
    }
}
