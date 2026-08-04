use std::path::Path;

use crate::error::{AppError, AppResult};

use super::{
    backup::verify_sqlite,
    fingerprint::{SchemaClass, database_fingerprint, file_fingerprint, open_read_only},
    manifest::{ReplacementManifest, SourceSet, read},
    paths::MigrationPaths,
    platform::{remove_synced, rename_synced},
};

pub(super) fn recover_or_publish(paths: &MigrationPaths) -> AppResult<bool> {
    if !paths.replacement_manifest.exists() {
        return Ok(false);
    }
    let manifest: ReplacementManifest = read(&paths.replacement_manifest)?;
    manifest.validate_identity()?;
    let live_class = super::probe::classify_live(paths)?;
    match live_class {
        SchemaClass::CurrentV5 => {
            verify_replacement(&paths.live, &manifest)?;
            if paths.replacement_stage.exists() {
                return invalid("published v5 coexists with a replacement stage");
            }
            cleanup_retired(paths, &manifest)?;
            Ok(true)
        }
        class if class.is_legacy() => {
            if !paths.replacement_stage.exists() && !paths.retired_live.exists() {
                finish_restored_source(paths, &manifest)?;
                return Ok(false);
            }
            verify_legacy_prefix(paths, &manifest)?;
            verify_replacement(&paths.replacement_stage, &manifest)?;
            retire_source(paths, &manifest)?;
            publish_stage(paths, &manifest)?;
            cleanup_retired(paths, &manifest)?;
            Ok(true)
        }
        SchemaClass::Fresh if !paths.live.exists() => {
            if paths.replacement_stage.exists() {
                verify_complete_retired(paths, &manifest)?;
                verify_replacement(&paths.replacement_stage, &manifest)?;
                publish_stage(paths, &manifest)?;
                cleanup_retired(paths, &manifest)?;
                Ok(true)
            } else {
                restore_retired_source(paths, &manifest)?;
                Ok(false)
            }
        }
        _ => invalid("unlisted replacement recovery state"),
    }
}

fn verify_legacy_prefix(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    verify_file(&paths.live, &manifest.source_after_checkpoint.main)?;
    verify_database(&paths.live, &manifest.source_after_checkpoint)?;
    verify_sidecar_prefix(
        &paths.live_wal,
        &paths.retired_wal,
        &manifest.source_after_checkpoint.wal,
        true,
    )?;
    let wal_retired = paths.retired_wal.exists() || !manifest.source_after_checkpoint.wal.present;
    verify_sidecar_prefix(
        &paths.live_shm,
        &paths.retired_shm,
        &manifest.source_after_checkpoint.shm,
        wal_retired,
    )?;
    if paths.retired_live.exists() {
        return invalid("legacy live database coexists with a retired main file");
    }
    Ok(())
}

fn verify_sidecar_prefix(
    live: &Path,
    retired: &Path,
    expected: &super::fingerprint::FileFingerprint,
    retirement_allowed: bool,
) -> AppResult<()> {
    match (expected.present, live.exists(), retired.exists()) {
        (false, false, false) => Ok(()),
        (true, true, false) => verify_file(live, expected),
        (true, false, true) if retirement_allowed => verify_file(retired, expected),
        _ => invalid("sidecar retirement state is not a valid manifested prefix"),
    }
}

fn retire_source(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    retire_one(
        &paths.live_wal,
        &paths.retired_wal,
        &manifest.source_after_checkpoint.wal,
    )?;
    #[cfg(test)]
    super::test_failpoint::hit("retire-wal-rename-or-parent-fsync")?;
    retire_one(
        &paths.live_shm,
        &paths.retired_shm,
        &manifest.source_after_checkpoint.shm,
    )?;
    #[cfg(test)]
    super::test_failpoint::hit("retire-shm-rename-or-parent-fsync")?;
    if !paths.retired_live.exists() {
        verify_file(&paths.live, &manifest.source_after_checkpoint.main)?;
        rename_synced(&paths.live, &paths.retired_live)?;
    }
    #[cfg(test)]
    super::test_failpoint::hit("retire-live-rename-or-parent-fsync")?;
    verify_complete_retired(paths, manifest)
}

fn retire_one(
    live: &Path,
    retired: &Path,
    expected: &super::fingerprint::FileFingerprint,
) -> AppResult<()> {
    if !expected.present {
        if live.exists() || retired.exists() {
            return invalid("unexpected source sidecar blocks replacement");
        }
        return Ok(());
    }
    if retired.exists() {
        if live.exists() {
            return invalid("source and retired sidecar both exist");
        }
        return verify_file(retired, expected);
    }
    verify_file(live, expected)?;
    rename_synced(live, retired)
}

fn verify_complete_retired(
    paths: &MigrationPaths,
    manifest: &ReplacementManifest,
) -> AppResult<()> {
    verify_file(&paths.retired_live, &manifest.source_after_checkpoint.main)?;
    verify_retired_optional(&paths.retired_wal, &manifest.source_after_checkpoint.wal)?;
    verify_retired_optional(&paths.retired_shm, &manifest.source_after_checkpoint.shm)
}

fn verify_retired_optional(
    path: &Path,
    expected: &super::fingerprint::FileFingerprint,
) -> AppResult<()> {
    if expected.present {
        verify_file(path, expected)
    } else if path.exists() {
        invalid("unexpected retired sidecar blocks replacement")
    } else {
        Ok(())
    }
}

fn publish_stage(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    if paths.live_wal.exists() || paths.live_shm.exists() {
        return invalid("old SQLite sidecars remain at the live name");
    }
    verify_replacement(&paths.replacement_stage, manifest)?;
    rename_synced(&paths.replacement_stage, &paths.live)?;
    verify_replacement(&paths.live, manifest)?;
    #[cfg(test)]
    super::test_failpoint::hit("replacement-publish")?;
    Ok(())
}

fn cleanup_retired(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    if (!manifest.source_after_checkpoint.shm.present && paths.retired_shm.exists())
        || (!manifest.source_after_checkpoint.wal.present && paths.retired_wal.exists())
    {
        return invalid("unexpected retired sidecar blocks cleanup");
    }
    if manifest.source_after_checkpoint.shm.present && paths.retired_shm.exists() {
        verify_file(&paths.retired_shm, &manifest.source_after_checkpoint.shm)?;
        remove_synced(&paths.retired_shm)?;
    }
    #[cfg(test)]
    super::test_failpoint::hit("cleanup-retired-shm-remove-or-parent-fsync")?;
    if manifest.source_after_checkpoint.wal.present && paths.retired_wal.exists() {
        verify_file(&paths.retired_wal, &manifest.source_after_checkpoint.wal)?;
        remove_synced(&paths.retired_wal)?;
    }
    #[cfg(test)]
    super::test_failpoint::hit("cleanup-retired-wal-remove-or-parent-fsync")?;
    if paths.retired_live.exists() {
        verify_file(&paths.retired_live, &manifest.source_after_checkpoint.main)?;
        remove_synced(&paths.retired_live)?;
    }
    #[cfg(test)]
    super::test_failpoint::hit("cleanup-retired-live-remove-or-parent-fsync")?;
    remove_synced(&paths.replacement_manifest)?;
    #[cfg(test)]
    super::test_failpoint::hit("cleanup-replacement-manifest-remove-or-parent-fsync")?;
    Ok(())
}

fn restore_retired_source(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    verify_restore_prefix(paths, manifest)?;
    restore_optional(
        &paths.retired_shm,
        &paths.live_shm,
        &manifest.source_after_checkpoint.shm,
    )?;
    restore_optional(
        &paths.retired_wal,
        &paths.live_wal,
        &manifest.source_after_checkpoint.wal,
    )?;
    if paths.retired_live.exists() {
        verify_file(&paths.retired_live, &manifest.source_after_checkpoint.main)?;
        rename_synced(&paths.retired_live, &paths.live)?;
    }
    finish_restored_source(paths, manifest)
}

fn restore_optional(
    retired: &Path,
    live: &Path,
    expected: &super::fingerprint::FileFingerprint,
) -> AppResult<()> {
    if !expected.present {
        return if retired.exists() || live.exists() {
            invalid("unexpected sidecar blocks source restoration")
        } else {
            Ok(())
        };
    }
    match (retired.exists(), live.exists()) {
        (true, false) => {
            verify_file(retired, expected)?;
            rename_synced(retired, live)
        }
        (false, true) => verify_file(live, expected),
        _ => invalid("sidecar restoration prefix is invalid"),
    }
}

fn verify_restore_prefix(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    if paths.live.exists() {
        return invalid("restore prefix published main before all sidecars");
    }
    verify_file(&paths.retired_live, &manifest.source_after_checkpoint.main)?;
    verify_restore_optional(
        &paths.retired_shm,
        &paths.live_shm,
        &manifest.source_after_checkpoint.shm,
        true,
    )?;
    let shm_done = !manifest.source_after_checkpoint.shm.present || paths.live_shm.exists();
    verify_restore_optional(
        &paths.retired_wal,
        &paths.live_wal,
        &manifest.source_after_checkpoint.wal,
        shm_done,
    )
}

fn verify_restore_optional(
    retired: &Path,
    live: &Path,
    expected: &super::fingerprint::FileFingerprint,
    live_allowed: bool,
) -> AppResult<()> {
    match (expected.present, retired.exists(), live.exists()) {
        (false, false, false) => Ok(()),
        (true, true, false) => verify_file(retired, expected),
        (true, false, true) if live_allowed => verify_file(live, expected),
        _ => invalid("sidecar restoration state is not a valid manifested prefix"),
    }
}

fn finish_restored_source(paths: &MigrationPaths, manifest: &ReplacementManifest) -> AppResult<()> {
    verify_file(&paths.live, &manifest.source_after_checkpoint.main)?;
    verify_database(&paths.live, &manifest.source_after_checkpoint)?;
    verify_restored_optional(&paths.live_shm, &manifest.source_after_checkpoint.shm)?;
    verify_restored_optional(&paths.live_wal, &manifest.source_after_checkpoint.wal)?;
    if paths.retired_live.exists() || paths.retired_shm.exists() || paths.retired_wal.exists() {
        return invalid("restored source coexists with retired artifacts");
    }
    remove_synced(&paths.replacement_manifest)
}

fn verify_restored_optional(
    path: &Path,
    expected: &super::fingerprint::FileFingerprint,
) -> AppResult<()> {
    if expected.present {
        verify_file(path, expected)
    } else if path.exists() {
        invalid("restored source has an unexpected sidecar")
    } else {
        Ok(())
    }
}

fn verify_replacement(path: &Path, manifest: &ReplacementManifest) -> AppResult<()> {
    verify_file(path, &manifest.replacement)?;
    let connection = open_read_only(path)?;
    verify_sqlite(&connection)?;
    let database = database_fingerprint(&connection)?;
    if database != manifest.replacement_database || database.schema != SchemaClass::CurrentV5 {
        return invalid("replacement database fingerprint mismatch");
    }
    Ok(())
}

fn verify_database(path: &Path, expected: &SourceSet) -> AppResult<()> {
    let connection = open_read_only(path)?;
    verify_sqlite(&connection)?;
    if database_fingerprint(&connection)? != expected.database {
        return invalid("legacy live logical fingerprint changed during migration");
    }
    Ok(())
}

fn verify_file(path: &Path, expected: &super::fingerprint::FileFingerprint) -> AppResult<()> {
    if file_fingerprint(path)? != *expected {
        return invalid("migration artifact bytes do not match the manifest");
    }
    Ok(())
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
