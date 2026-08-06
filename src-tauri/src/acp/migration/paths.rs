use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub(super) struct MigrationPaths {
    pub(super) live: PathBuf,
    pub(super) live_wal: PathBuf,
    pub(super) live_shm: PathBuf,
    pub(super) backup_stage: PathBuf,
    pub(super) backup_journal: PathBuf,
    pub(super) backup_build_intent: PathBuf,
    pub(super) backup_stage_owner: PathBuf,
    pub(super) backup_final: PathBuf,
    pub(super) manifest_stage: PathBuf,
    pub(super) manifest_final: PathBuf,
    pub(super) replacement_stage: PathBuf,
    pub(super) replacement_journal: PathBuf,
    pub(super) replacement_build_intent: PathBuf,
    pub(super) replacement_stage_owner: PathBuf,
    pub(super) replacement_manifest: PathBuf,
    pub(super) retired_live: PathBuf,
    pub(super) retired_wal: PathBuf,
    pub(super) retired_shm: PathBuf,
    pub(super) probe_main: PathBuf,
    pub(super) probe_wal: PathBuf,
    pub(super) probe_shm: PathBuf,
    pub(super) probe_manifest: PathBuf,
}

impl MigrationPaths {
    pub(super) fn new(live: &Path) -> AppResult<Self> {
        let parent = live.parent().ok_or_else(|| {
            AppError::InvalidRequest("Sage database path has no parent directory".into())
        })?;
        if live.file_name().and_then(|name| name.to_str()) != Some("sage.db") {
            return Err(AppError::InvalidRequest(
                "schema migration is admitted only for the canonical sage.db filename".into(),
            ));
        }
        if !live.is_absolute() {
            return Err(AppError::InvalidRequest(
                "schema migration requires an absolute Sage database path".into(),
            ));
        }
        let paths = Self {
            live: live.to_path_buf(),
            live_wal: parent.join("sage.db-wal"),
            live_shm: parent.join("sage.db-shm"),
            backup_stage: parent.join("sage.pre-v5.db.stage"),
            backup_journal: parent.join("sage.pre-v5.db.stage-journal"),
            backup_build_intent: parent.join("sage.pre-v5.build-intent.json"),
            backup_stage_owner: parent.join("sage.pre-v5.stage-owner.json"),
            backup_final: parent.join("sage.pre-v5.db"),
            manifest_stage: parent.join("sage.pre-v5.manifest.json.stage"),
            manifest_final: parent.join("sage.pre-v5.manifest.json"),
            replacement_stage: parent.join("sage.v5.db.stage"),
            replacement_journal: parent.join("sage.v5.db.stage-journal"),
            replacement_build_intent: parent.join("sage.v5.build-intent.json"),
            replacement_stage_owner: parent.join("sage.v5.stage-owner.json"),
            replacement_manifest: parent.join("sage.v5.manifest.json.stage"),
            retired_live: parent.join("sage.v4.db.retired"),
            retired_wal: parent.join("sage.v4.db-wal.retired"),
            retired_shm: parent.join("sage.v4.db-shm.retired"),
            probe_main: parent.join("sage.migration-probe.db"),
            probe_wal: parent.join("sage.migration-probe.db-wal"),
            probe_shm: parent.join("sage.migration-probe.db-shm"),
            probe_manifest: parent.join("sage.migration-probe.manifest.json"),
        };
        paths.validate_containment()?;
        Ok(paths)
    }

    pub(super) fn parent(&self) -> &Path {
        self.live
            .parent()
            .expect("validated migration path has a parent")
    }

    pub(super) fn has_replacement_artifact(&self) -> bool {
        [
            &self.replacement_stage,
            &self.replacement_journal,
            &self.replacement_build_intent,
            &self.replacement_stage_owner,
            &self.replacement_manifest,
            &self.retired_live,
            &self.retired_wal,
            &self.retired_shm,
        ]
        .iter()
        .any(|path| path.exists())
    }

    pub(super) fn validate_containment(&self) -> AppResult<()> {
        let parent = self.parent();
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir() {
            return Err(AppError::InvalidRequest(
                "Sage migration parent is not a directory".into(),
            ));
        }
        super::platform::reject_path_alias(parent)?;
        for path in [
            &self.live,
            &self.live_wal,
            &self.live_shm,
            &self.backup_stage,
            &self.backup_journal,
            &self.backup_build_intent,
            &self.backup_stage_owner,
            &self.backup_final,
            &self.manifest_stage,
            &self.manifest_final,
            &self.replacement_stage,
            &self.replacement_journal,
            &self.replacement_build_intent,
            &self.replacement_stage_owner,
            &self.replacement_manifest,
            &self.retired_live,
            &self.retired_wal,
            &self.retired_shm,
            &self.probe_main,
            &self.probe_wal,
            &self.probe_shm,
            &self.probe_manifest,
        ] {
            if path.parent() != Some(parent) {
                return Err(AppError::InvalidRequest(
                    "migration artifact escaped the live database directory".into(),
                ));
            }
            super::platform::reject_path_alias(path)?;
        }
        Ok(())
    }
}
