use super::{
    backup::{VerifiedBackup, ensure, load_retained_pair},
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

pub(super) fn ensure_for_v17(paths: &MigrationPaths) -> AppResult<VerifiedBackup> {
    reject_partial_backup_publication(paths)?;
    let versioned = paths_for_v18(paths);
    versioned.validate_containment()?;
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
        SchemaClass::CurrentV17 => Ok(()),
        SchemaClass::CurrentV18 => {
            reject_partial_backup_publication(&versioned)?;
            if versioned.backup_final.exists()
                && load_retained_pair(&versioned)?.source.database.schema != SchemaClass::CurrentV17
            {
                return invalid("v18 backup does not retain the exact v17 source schema");
            }
            Ok(())
        }
        _ => reject_unexpected_backup_artifacts(&versioned),
    }
}
