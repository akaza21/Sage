use super::{
    backup::{VerifiedBackup, ensure, recover_existing},
    fingerprint::SchemaClass,
    invalid,
    paths::MigrationPaths,
    reject_partial_backup_publication, reject_unexpected_backup_artifacts,
};
use crate::error::AppResult;

pub(super) fn paths_for_v18(paths: &MigrationPaths) -> MigrationPaths {
    let mut versioned = paths.clone();
    let parent = paths.parent();
    versioned.backup_stage = parent.join("sage.pre-v18.db.stage");
    versioned.backup_journal = parent.join("sage.pre-v18.db.stage-journal");
    versioned.backup_build_intent = parent.join("sage.pre-v18.build-intent.json");
    versioned.backup_stage_owner = parent.join("sage.pre-v18.stage-owner.json");
    versioned.backup_final = parent.join("sage.pre-v18.db");
    versioned.manifest_stage = parent.join("sage.pre-v18.manifest.json.stage");
    versioned.manifest_final = parent.join("sage.pre-v18.manifest.json");
    versioned
}

pub(super) fn ensure_for_v18_source(
    paths: &MigrationPaths,
    schema: SchemaClass,
) -> AppResult<VerifiedBackup> {
    let versioned = paths_for_v18(paths);
    versioned.validate_containment()?;
    if schema == SchemaClass::CurrentV16 {
        if let Some(backup) = recover_existing(paths, schema)?
            && !versioned.has_backup_artifact()
        {
            return Ok(backup);
        }
    } else {
        let _ = reject_partial_backup_publication(paths)?;
    }
    ensure(&versioned)
}

pub(super) fn admit_recovery(paths: &MigrationPaths) -> AppResult<()> {
    let versioned = paths_for_v18(paths);
    versioned.validate_containment()?;
    if paths.has_replacement_artifact() {
        reject_unexpected_backup_artifacts(&versioned)?;
    }
    Ok(())
}

pub(super) fn admit_source(paths: &MigrationPaths, schema: SchemaClass) -> AppResult<()> {
    let versioned = paths_for_v18(paths);
    versioned.validate_containment()?;
    match schema {
        SchemaClass::CurrentV16 | SchemaClass::CurrentV17 => Ok(()),
        SchemaClass::CurrentV18
        | SchemaClass::CurrentV19
        | SchemaClass::CurrentV20
        | SchemaClass::CurrentV21
        | SchemaClass::CurrentV22
        | SchemaClass::CurrentV23
        | SchemaClass::CurrentResumeLink
        | SchemaClass::CurrentTitleSource
        | SchemaClass::CurrentSessionFactsTurnIndex => {
            if let Some(backup) = reject_partial_backup_publication(&versioned)?
                && !matches!(
                    backup.source.database.schema,
                    SchemaClass::CurrentV16 | SchemaClass::CurrentV17
                )
            {
                return invalid("v18 backup does not retain an exact v16 or v17 source schema");
            }
            Ok(())
        }
        _ => reject_unexpected_backup_artifacts(&versioned),
    }
}
