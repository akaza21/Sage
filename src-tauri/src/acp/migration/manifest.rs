use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::{
    fingerprint::{DatabaseFingerprint, FileFingerprint},
    limits::MANIFEST_BYTES,
    platform::{
        NativeFileIdentity, open_read_no_follow, reject_path_alias, write_bounded_create_new,
    },
};

const MANIFEST_SCHEMA: u32 = 1;
const APP_IDENTITY: &str = "sage-ade-0006a-schema-005-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SourceSet {
    pub(super) database: DatabaseFingerprint,
    pub(super) main: FileFingerprint,
    pub(super) wal: FileFingerprint,
    pub(super) shm: FileFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BackupSource {
    pub(super) database: DatabaseFingerprint,
    pub(super) main: FileFingerprint,
    pub(super) wal: FileFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BackupManifest {
    schema_version: u32,
    app_identity: String,
    pub(super) source: BackupSource,
    pub(super) backup: FileFingerprint,
    pub(super) backup_database: DatabaseFingerprint,
    created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReplacementBuildIntent {
    schema_version: u32,
    app_identity: String,
    pub(super) source_database: DatabaseFingerprint,
    pub(super) backup: FileFingerprint,
    pub(super) backup_database: DatabaseFingerprint,
    pub(super) replacement_stage_name: String,
    pub(super) replacement_manifest_name: String,
    pub(super) nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BackupBuildIntent {
    schema_version: u32,
    app_identity: String,
    pub(super) source_database: DatabaseFingerprint,
    pub(super) backup_stage_name: String,
    pub(super) backup_journal_name: String,
    pub(super) nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StageOwner {
    schema_version: u32,
    app_identity: String,
    pub(super) nonce: String,
    pub(super) stage_name: String,
    pub(super) journal_name: String,
    pub(super) stage_native_identity: NativeFileIdentity,
    pub(super) journal_native_identity: NativeFileIdentity,
}

impl ReplacementBuildIntent {
    pub(super) fn new(
        source_database: DatabaseFingerprint,
        backup: FileFingerprint,
        backup_database: DatabaseFingerprint,
        replacement_stage_name: String,
        replacement_manifest_name: String,
        nonce: String,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA,
            app_identity: APP_IDENTITY.into(),
            source_database,
            backup,
            backup_database,
            replacement_stage_name,
            replacement_manifest_name,
            nonce,
        }
    }

    pub(super) fn validate_identity(&self) -> AppResult<()> {
        if self.schema_version != MANIFEST_SCHEMA
            || self.app_identity != APP_IDENTITY
            || uuid::Uuid::parse_str(&self.nonce).is_err()
        {
            return invalid("replacement build intent has an unsupported application identity");
        }
        Ok(())
    }
}

impl BackupBuildIntent {
    pub(super) fn new(
        source_database: DatabaseFingerprint,
        backup_stage_name: String,
        backup_journal_name: String,
        nonce: String,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA,
            app_identity: APP_IDENTITY.into(),
            source_database,
            backup_stage_name,
            backup_journal_name,
            nonce,
        }
    }

    pub(super) fn validate_identity(&self) -> AppResult<()> {
        if self.schema_version != MANIFEST_SCHEMA
            || self.app_identity != APP_IDENTITY
            || uuid::Uuid::parse_str(&self.nonce).is_err()
        {
            return invalid("backup build intent has an unsupported application identity");
        }
        Ok(())
    }
}

impl StageOwner {
    pub(super) fn new(
        nonce: String,
        stage_name: String,
        journal_name: String,
        stage_native_identity: NativeFileIdentity,
        journal_native_identity: NativeFileIdentity,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA,
            app_identity: APP_IDENTITY.into(),
            nonce,
            stage_name,
            journal_name,
            stage_native_identity,
            journal_native_identity,
        }
    }

    pub(super) fn validate_identity(&self) -> AppResult<()> {
        if self.schema_version != MANIFEST_SCHEMA
            || self.app_identity != APP_IDENTITY
            || uuid::Uuid::parse_str(&self.nonce).is_err()
        {
            return invalid("stage owner has an unsupported application identity");
        }
        Ok(())
    }
}

impl BackupManifest {
    pub(super) fn new(
        source: BackupSource,
        backup: FileFingerprint,
        backup_database: DatabaseFingerprint,
        created_at: i64,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA,
            app_identity: APP_IDENTITY.into(),
            source,
            backup,
            backup_database,
            created_at,
        }
    }

    pub(super) fn validate_identity(&self) -> AppResult<()> {
        if self.schema_version != MANIFEST_SCHEMA || self.app_identity != APP_IDENTITY {
            return invalid("backup manifest has an unsupported application identity");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReplacementManifest {
    schema_version: u32,
    app_identity: String,
    pub(super) source_before_checkpoint: BackupSource,
    pub(super) source_after_checkpoint: SourceSet,
    pub(super) backup: FileFingerprint,
    pub(super) replacement: FileFingerprint,
    pub(super) replacement_database: DatabaseFingerprint,
}

impl ReplacementManifest {
    pub(super) fn new(
        source_before_checkpoint: BackupSource,
        source_after_checkpoint: SourceSet,
        backup: FileFingerprint,
        replacement: FileFingerprint,
        replacement_database: DatabaseFingerprint,
    ) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA,
            app_identity: APP_IDENTITY.into(),
            source_before_checkpoint,
            source_after_checkpoint,
            backup,
            replacement,
            replacement_database,
        }
    }

    pub(super) fn validate_identity(&self) -> AppResult<()> {
        if self.schema_version != MANIFEST_SCHEMA || self.app_identity != APP_IDENTITY {
            return invalid("replacement manifest has an unsupported application identity");
        }
        Ok(())
    }
}

pub(super) fn write_stage<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if path.exists() {
        return invalid("migration manifest stage already exists");
    }
    let mut bytes =
        serde_json::to_vec(value).map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MANIFEST_BYTES {
        return Err(AppError::ResourceLimit(
            "migration manifest exceeds 64 KiB".into(),
        ));
    }
    write_bounded_create_new(path, &bytes, MANIFEST_BYTES)
}

pub(super) fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> AppResult<T> {
    reject_path_alias(path)?;
    use std::io::Read;
    let file = open_read_no_follow(path)?;
    let mut bytes = Vec::with_capacity(MANIFEST_BYTES as usize + 1);
    file.take(MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_BYTES {
        return Err(AppError::ResourceLimit(
            "migration manifest exceeds 64 KiB".into(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| AppError::InvalidRequest(error.to_string()))
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
