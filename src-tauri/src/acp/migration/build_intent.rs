use std::path::Path;

use crate::error::{AppError, AppResult};

use super::{
    backup::{VerifiedBackup, load_retained_pair},
    fingerprint::{database_fingerprint, file_fingerprint, open_read_only},
    manifest::{ReplacementBuildIntent, ReplacementManifest, read, write_stage},
    paths::MigrationPaths,
    platform::remove_synced,
    stage_owner,
};

pub(super) fn begin(paths: &MigrationPaths, backup: &VerifiedBackup) -> AppResult<()> {
    if paths.replacement_build_intent.exists()
        || paths.replacement_stage.exists()
        || paths.replacement_journal.exists()
        || paths.replacement_stage_owner.exists()
        || paths.replacement_manifest.exists()
        || has_retired(paths)
    {
        return invalid("replacement build cannot begin over existing migration artifacts");
    }
    let intent = ReplacementBuildIntent::new(
        backup.source.database.clone(),
        backup.backup_file.clone(),
        backup.backup_database.clone(),
        file_name(&paths.replacement_stage)?.into(),
        file_name(&paths.replacement_manifest)?.into(),
        uuid::Uuid::new_v4().to_string(),
    );
    write_stage(&paths.replacement_build_intent, &intent)
}

pub(super) fn resume(paths: &MigrationPaths) -> AppResult<()> {
    let intent = read_intent(paths)?;
    if paths.replacement_manifest.exists() {
        return finalize_published_manifest(paths);
    }
    if has_retired(paths) {
        return invalid("replacement build intent coexists with retired source artifacts");
    }
    let source = super::probe::inspect_live(paths)?
        .ok_or_else(|| AppError::InvalidRequest("replacement build source is absent".into()))?;
    if !source.schema.is_legacy() || source != intent.source_database {
        return invalid("replacement build source no longer matches its intent");
    }
    let backup = load_retained_pair(paths)?;
    validate_backup(&intent, &backup)?;

    remove_authorized_partial(paths)?;
    super::build_replacement(paths, &backup, &intent.nonce)?;
    let source_after_checkpoint = super::checkpoint_source(paths, &backup)?;
    let replacement = file_fingerprint(&paths.replacement_stage)?;
    let replacement_connection = open_read_only(&paths.replacement_stage)?;
    let replacement_database = database_fingerprint(&replacement_connection)?;
    super::verify_current(&replacement_connection)?;
    drop(replacement_connection);
    let manifest = ReplacementManifest::new(
        backup.source,
        source_after_checkpoint,
        backup.backup_file,
        replacement,
        replacement_database,
    );
    write_stage(&paths.replacement_manifest, &manifest)?;
    #[cfg(test)]
    super::test_failpoint::hit("replacement-manifest-fsync")?;
    finalize_published_manifest(paths)
}

pub(super) fn finalize_published_manifest(paths: &MigrationPaths) -> AppResult<()> {
    let intent = read_intent(paths)?;
    let manifest: ReplacementManifest = read(&paths.replacement_manifest)?;
    manifest.validate_identity()?;
    let backup = load_retained_pair(paths)?;
    validate_backup(&intent, &backup)?;
    if manifest.source_before_checkpoint.database != intent.source_database
        || manifest.source_after_checkpoint.database != intent.source_database
        || manifest.source_before_checkpoint != backup.source
        || manifest.backup != intent.backup
    {
        return invalid("replacement manifest does not match its build intent");
    }
    let mut anchor = stage_owner::verify_bound_stage(
        &paths.replacement_stage,
        &paths.replacement_journal,
        &paths.replacement_stage_owner,
        &intent.nonce,
        false,
    )?;
    if file_fingerprint(&paths.replacement_stage)? != manifest.replacement {
        return invalid("replacement stage bytes do not match both manifests");
    }
    anchor.verify_path(&paths.replacement_stage)?;
    let replacement = open_read_only(&paths.replacement_stage)?;
    super::verify_current(&replacement)?;
    if database_fingerprint(&replacement)? != manifest.replacement_database {
        return invalid("replacement stage logical fingerprint does not match its manifest");
    }
    drop(replacement);
    anchor.retire_journal_if_present(&paths.replacement_journal)?;
    drop(anchor);
    stage_owner::retire_owner(&paths.replacement_stage_owner)?;
    remove_synced(&paths.replacement_build_intent)?;
    #[cfg(test)]
    super::test_failpoint::hit("replacement-build-intent-remove-or-parent-fsync")?;
    Ok(())
}

pub(super) fn read_intent(paths: &MigrationPaths) -> AppResult<ReplacementBuildIntent> {
    let intent: ReplacementBuildIntent = read(&paths.replacement_build_intent)?;
    intent.validate_identity()?;
    if intent.replacement_stage_name != file_name(&paths.replacement_stage)?
        || intent.replacement_manifest_name != file_name(&paths.replacement_manifest)?
        || !intent.source_database.schema.is_legacy()
    {
        return invalid("replacement build intent names or schema are invalid");
    }
    Ok(intent)
}

fn validate_backup(intent: &ReplacementBuildIntent, backup: &VerifiedBackup) -> AppResult<()> {
    if backup.source.database != intent.source_database
        || backup.backup_file != intent.backup
        || backup.backup_database != intent.backup_database
        || intent.backup_database != intent.source_database
    {
        return invalid("verified backup no longer matches replacement build intent");
    }
    Ok(())
}

fn remove_authorized_partial(paths: &MigrationPaths) -> AppResult<()> {
    let intent = read_intent(paths)?;
    stage_owner::remove_owned_partial(
        &paths.replacement_stage,
        &paths.replacement_journal,
        &paths.replacement_stage_owner,
        &intent.nonce,
    )
}

fn has_retired(paths: &MigrationPaths) -> bool {
    paths.retired_live.exists() || paths.retired_wal.exists() || paths.retired_shm.exists()
}

fn file_name(path: &Path) -> AppResult<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("migration artifact filename is not UTF-8".into()))
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
