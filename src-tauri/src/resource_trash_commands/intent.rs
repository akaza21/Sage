use std::path::PathBuf;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{
    domain::{ResourceRef, TrashProjectResourceRequest, TrashResourceKind},
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    project_registry::RootRecord,
    workspace::{
        MAX_DIRECTORY_ENTRIES,
        resource_trash::{ResourceTrashIdentity, TRASH_NAME},
        root_identity_path,
    },
};

use super::{MAX_JAVASCRIPT_SAFE_INTEGER, layout, parent_resource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingTrashIntent {
    pub(super) project_id: String,
    pub(super) project_generation: u64,
    pub(super) transition_epoch: u64,
    pub(super) source: ResourceRef,
    pub(super) parent: ResourceRef,
    pub(super) kind: TrashResourceKind,
    root_identity: [u8; 24],
    parent_identity: [u8; 24],
    source_identity: [u8; 24],
    digest: String,
}

pub(crate) struct StartupTrashRecovery {
    pub(crate) transition_epoch: u64,
    pub(crate) notice: String,
}

impl PendingTrashIntent {
    pub(super) fn new(
        request: &TrashProjectResourceRequest,
        identity: ResourceTrashIdentity,
    ) -> Self {
        let parent = parent_resource(&request.source);
        let (root_identity, parent_identity, source_identity) = identity.component_bytes();
        let mut pending = Self {
            project_id: request.project_id.clone(),
            project_generation: request.project_generation,
            transition_epoch: request.expected_transition_epoch,
            source: request.source.clone(),
            parent,
            kind: request.kind,
            root_identity,
            parent_identity,
            source_identity,
            digest: String::new(),
        };
        pending.digest = pending.calculate_digest();
        pending
    }

    fn calculate_digest(&self) -> String {
        let mut hash = Sha256::new();
        for value in [
            self.project_id.as_bytes(),
            self.source.root_id.as_bytes(),
            self.source.relative_path.as_bytes(),
            self.parent.relative_path.as_bytes(),
            kind_text(self.kind).as_bytes(),
        ] {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value);
        }
        hash.update(self.project_generation.to_be_bytes());
        hash.update(self.transition_epoch.to_be_bytes());
        hash.update(self.root_identity);
        hash.update(self.parent_identity);
        hash.update(self.source_identity);
        format!("{:x}", hash.finalize())
    }

    fn validate(&self) -> AppResult<()> {
        if self.project_id.is_empty()
            || self.source.root_id.is_empty()
            || self.source.relative_path.is_empty()
            || self.parent.root_id != self.source.root_id
            || self.parent != parent_resource(&self.source)
            || self.project_generation > MAX_JAVASCRIPT_SAFE_INTEGER
            || self.transition_epoch >= MAX_JAVASCRIPT_SAFE_INTEGER
            || self.digest != self.calculate_digest()
        {
            return Err(AppError::InvalidRequest(format!(
                "pending {TRASH_NAME} intent is corrupt"
            )));
        }
        crate::workspace::validate_contract_path(&self.source.relative_path, false)?;
        crate::workspace::validate_contract_path(&self.parent.relative_path, true)
    }
}

pub(super) fn persist(database: &Database, pending: &PendingTrashIntent) -> AppResult<()> {
    pending.validate()?;
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM resource_trash_intents WHERE singleton=1)",
        [],
        |row| row.get(0),
    )?;
    if exists {
        return Err(AppError::InvalidRequest(format!(
            "another pending {TRASH_NAME} intent requires startup reconciliation"
        )));
    }
    transaction.execute(
        "INSERT INTO resource_trash_intents(
           singleton,project_id,project_generation,transition_epoch,root_id,
           source_relative_path,parent_relative_path,kind,root_identity,
           parent_identity,source_identity,identity_digest,created_at
         ) VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            pending.project_id,
            pending.project_generation as i64,
            pending.transition_epoch as i64,
            pending.source.root_id,
            pending.source.relative_path,
            pending.parent.relative_path,
            kind_text(pending.kind),
            pending.root_identity.as_slice(),
            pending.parent_identity.as_slice(),
            pending.source_identity.as_slice(),
            pending.digest,
            now_millis(),
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn delete_exact(
    transaction: &rusqlite::Transaction<'_>,
    pending: &PendingTrashIntent,
) -> AppResult<()> {
    let removed = transaction.execute(
        "DELETE FROM resource_trash_intents
         WHERE singleton=1 AND identity_digest=?1",
        [&pending.digest],
    )?;
    if removed != 1 {
        return Err(AppError::InvalidRequest(format!(
            "pending {TRASH_NAME} intent changed before settlement"
        )));
    }
    Ok(())
}

pub(super) fn clear(database: &Database, pending: &PendingTrashIntent) -> AppResult<()> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    delete_exact(&transaction, pending)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn recover_at_startup(database: &Database) -> AppResult<Option<StartupTrashRecovery>> {
    let Some(pending) = load(database)? else {
        return Ok(None);
    };
    let observation = load_root(database, &pending)
        .and_then(|root| observe_filesystem(&root.absolute_path, &pending));
    let confirmed_absent = observation
        .as_ref()
        .is_ok_and(|observed| observed.authority_matches && observed.source_absent);
    let unchanged = observation
        .as_ref()
        .is_ok_and(|observed| observed.authority_matches && observed.source_matches);

    let mut connection = database.write_connection()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if confirmed_absent {
        layout::migrate_selection(
            &transaction,
            &pending.project_id,
            &pending.source,
            &pending.parent,
        )?;
    }
    delete_exact(&transaction, &pending)?;
    transaction.commit()?;

    let detail = if confirmed_absent {
        "the source is absent and the durable Explorer selection was repaired"
    } else if unchanged {
        "the exact source remains present, so no Explorer resource was removed"
    } else {
        "filesystem identity could not be confirmed, so no replacement resource was modified"
    };
    Ok(Some(StartupTrashRecovery {
        transition_epoch: pending.transition_epoch + 1,
        notice: format!(
            "A pending {TRASH_NAME} operation was recovered as EffectUnknown after restart; {detail}."
        ),
    }))
}

pub(super) fn load(database: &Database) -> AppResult<Option<PendingTrashIntent>> {
    let connection = database.connection();
    let row = connection
        .query_row(
            "SELECT project_id,project_generation,transition_epoch,root_id,
                    source_relative_path,parent_relative_path,kind,root_identity,
                    parent_identity,source_identity,identity_digest
             FROM resource_trash_intents WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, Vec<u8>>(8)?,
                    row.get::<_, Vec<u8>>(9)?,
                    row.get::<_, String>(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        project_id,
        generation,
        epoch,
        root_id,
        source,
        parent,
        kind,
        root_identity,
        parent_identity,
        source_identity,
        digest,
    )) = row
    else {
        return Ok(None);
    };
    let pending = PendingTrashIntent {
        project_id,
        project_generation: nonnegative(generation)?,
        transition_epoch: nonnegative(epoch)?,
        source: ResourceRef {
            root_id: root_id.clone(),
            relative_path: source,
        },
        parent: ResourceRef {
            root_id,
            relative_path: parent,
        },
        kind: parse_kind(&kind)?,
        root_identity: identity_array(root_identity)?,
        parent_identity: identity_array(parent_identity)?,
        source_identity: identity_array(source_identity)?,
        digest,
    };
    pending.validate()?;
    Ok(Some(pending))
}

fn load_root(database: &Database, pending: &PendingTrashIntent) -> AppResult<RootRecord> {
    database
        .connection()
        .query_row(
            "SELECT id,project_id,absolute_path,ordinal FROM project_roots
             WHERE id=?1 AND project_id=?2",
            params![pending.source.root_id, pending.project_id],
            |row| {
                Ok(RootRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    absolute_path: row.get::<_, String>(2)?.into(),
                    ordinal: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::InvalidRequest(format!("pending {TRASH_NAME} root is unavailable"))
        })
}

struct FilesystemObservation {
    authority_matches: bool,
    source_absent: bool,
    source_matches: bool,
}

fn observe_filesystem(
    root: &std::path::Path,
    pending: &PendingTrashIntent,
) -> AppResult<FilesystemObservation> {
    let parent = join_relative(root, &pending.parent.relative_path);
    let source = join_relative(root, &pending.source.relative_path);
    let root_matches = root_identity_path(root)?.stable_bytes() == pending.root_identity;
    let parent_before = root_identity_path(&parent)?.stable_bytes();
    match std::fs::symlink_metadata(&source) {
        Ok(metadata) => {
            let source_matches = if metadata_is_alias(&metadata) {
                false
            } else {
                let kind_matches = match pending.kind {
                    TrashResourceKind::File => metadata.is_file(),
                    TrashResourceKind::EmptyFolder => metadata.is_dir(),
                };
                kind_matches
                    && root_identity_path(&source)?.stable_bytes() == pending.source_identity
            };
            let parent_after = root_identity_path(&parent)?.stable_bytes();
            Ok(FilesystemObservation {
                authority_matches: root_matches
                    && parent_before == pending.parent_identity
                    && parent_after == pending.parent_identity,
                source_absent: false,
                source_matches,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let source_name = source.file_name().ok_or_else(|| {
                AppError::InvalidRequest(format!("pending {TRASH_NAME} source has no filename"))
            })?;
            let mut count = 0_usize;
            let mut source_seen = false;
            for entry in std::fs::read_dir(&parent)? {
                let entry = entry?;
                count += 1;
                if count > MAX_DIRECTORY_ENTRIES {
                    return Err(AppError::ResourceLimit(format!(
                        "pending {TRASH_NAME} parent exceeds the bounded recovery scan"
                    )));
                }
                if entry.file_name() == source_name {
                    source_seen = true;
                }
            }
            let still_absent = match std::fs::symlink_metadata(&source) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                Ok(_) => false,
                Err(error) => return Err(error.into()),
            };
            let parent_after = root_identity_path(&parent)?.stable_bytes();
            Ok(FilesystemObservation {
                authority_matches: root_matches
                    && parent_before == pending.parent_identity
                    && parent_after == pending.parent_identity,
                source_absent: !source_seen && still_absent,
                source_matches: false,
            })
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
fn metadata_is_alias(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_alias(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn join_relative(root: &std::path::Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        root.to_owned()
    } else {
        root.join(relative)
    }
}

fn identity_array(bytes: Vec<u8>) -> AppResult<[u8; 24]> {
    bytes.try_into().map_err(|_| {
        AppError::InvalidRequest(format!("pending {TRASH_NAME} identity length is corrupt"))
    })
}

fn nonnegative(value: i64) -> AppResult<u64> {
    u64::try_from(value)
        .map_err(|_| AppError::InvalidRequest(format!("pending {TRASH_NAME} counter is corrupt")))
}

fn kind_text(kind: TrashResourceKind) -> &'static str {
    match kind {
        TrashResourceKind::File => "file",
        TrashResourceKind::EmptyFolder => "emptyFolder",
    }
}

fn parse_kind(value: &str) -> AppResult<TrashResourceKind> {
    match value {
        "file" => Ok(TrashResourceKind::File),
        "emptyFolder" => Ok(TrashResourceKind::EmptyFolder),
        _ => Err(AppError::InvalidRequest(format!(
            "pending {TRASH_NAME} kind is corrupt"
        ))),
    }
}
