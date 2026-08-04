use std::path::Path;

use crate::error::{AppError, AppResult};

use super::{
    manifest::{StageOwner, read, write_stage},
    platform::{
        SqlitePathAnchor, anchor_existing_sqlite, create_sqlite_stage, delete_file_with_identity,
        remove_synced,
    },
};

pub(super) struct BoundStage {
    stage: SqlitePathAnchor,
    journal: Option<SqlitePathAnchor>,
}

impl BoundStage {
    pub(super) fn verify_path(&self, stage: &Path) -> AppResult<()> {
        self.stage.verify_path(stage)
    }

    pub(super) fn sync(&self) -> AppResult<()> {
        self.stage.sync()
    }

    pub(super) fn retire_journal_if_present(&mut self, journal: &Path) -> AppResult<()> {
        let Some(anchor) = self.journal.take() else {
            return Ok(());
        };
        anchor.verify_path(journal)?;
        let identity = anchor.identity().clone();
        drop(anchor);
        delete_file_with_identity(journal, &identity)
    }
}

pub(super) fn create_bound_stage(
    stage: &Path,
    journal: &Path,
    owner_path: &Path,
    nonce: &str,
) -> AppResult<BoundStage> {
    if owner_path.exists() || stage.exists() || journal.exists() {
        return invalid("stage ownership cannot begin over existing artifacts");
    }
    let stage_anchor = create_sqlite_stage(stage)?;
    let journal_anchor = create_sqlite_stage(journal)?;
    let owner = StageOwner::new(
        nonce.into(),
        file_name(stage)?.into(),
        file_name(journal)?.into(),
        stage_anchor.identity().clone(),
        journal_anchor.identity().clone(),
    );
    write_stage(owner_path, &owner)?;
    stage_anchor.verify_path(stage)?;
    journal_anchor.verify_path(journal)?;
    Ok(BoundStage {
        stage: stage_anchor,
        journal: Some(journal_anchor),
    })
}

pub(super) fn verify_bound_stage(
    stage: &Path,
    journal: &Path,
    owner_path: &Path,
    nonce: &str,
    writable: bool,
) -> AppResult<BoundStage> {
    let owner: StageOwner = read(owner_path)?;
    owner.validate_identity()?;
    if owner.nonce != nonce
        || owner.stage_name != file_name(stage)?
        || owner.journal_name != file_name(journal)?
    {
        return invalid("stage owner does not match its build intent");
    }
    let stage_anchor = anchor_existing_sqlite(stage, writable)?;
    if stage_anchor.identity() != &owner.stage_native_identity {
        return invalid("stage native file identity does not match its owner");
    }
    stage_anchor.verify_path(stage)?;
    let journal_anchor = if journal.exists() {
        let anchor = anchor_existing_sqlite(journal, true)?;
        if anchor.identity() != &owner.journal_native_identity {
            return invalid("journal native file identity does not match its owner");
        }
        anchor.verify_path(journal)?;
        Some(anchor)
    } else {
        None
    };
    Ok(BoundStage {
        stage: stage_anchor,
        journal: journal_anchor,
    })
}

pub(super) fn remove_owned_partial(
    stage: &Path,
    journal: &Path,
    owner_path: &Path,
    nonce: &str,
) -> AppResult<()> {
    if owner_path.exists() {
        if !journal.exists() {
            return invalid("an owned partial SQLite journal is absent");
        }
        let owner: StageOwner = read(owner_path)?;
        owner.validate_identity()?;
        let anchor = verify_bound_stage(stage, journal, owner_path, nonce, true)?;
        anchor.verify_path(stage)?;
        drop(anchor);
        if journal.exists() {
            delete_file_with_identity(journal, &owner.journal_native_identity)?;
        }
        delete_file_with_identity(stage, &owner.stage_native_identity)?;
        return remove_synced(owner_path);
    }
    if journal.exists() {
        return invalid("an unowned SQLite stage journal is preserved");
    }
    if stage.exists() {
        return invalid("an unowned SQLite stage is preserved");
    }
    Ok(())
}

pub(super) fn retire_owner(owner_path: &Path) -> AppResult<()> {
    remove_synced(owner_path)
}

pub(super) fn retire_orphan_owner(
    stage: &Path,
    journal: &Path,
    owner_path: &Path,
    nonce: &str,
) -> AppResult<()> {
    if !owner_path.exists() {
        return Ok(());
    }
    if stage.exists() || journal.exists() {
        return invalid("stage owner is not orphaned");
    }
    let owner: StageOwner = read(owner_path)?;
    owner.validate_identity()?;
    if owner.nonce != nonce
        || owner.stage_name != file_name(stage)?
        || owner.journal_name != file_name(journal)?
    {
        return invalid("orphan stage owner does not match its build intent");
    }
    remove_synced(owner_path)
}

fn file_name(path: &Path) -> AppResult<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::InvalidRequest("stage filename is not UTF-8".into()))
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
