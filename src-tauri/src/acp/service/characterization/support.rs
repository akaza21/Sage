use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::acp::{
    contract::SageRootId,
    intent::IntentSnapshot,
    limits::LIMITS,
    process::{
        ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, ProcessDeadlines,
        RequiredContainment, TreeControl, TreeExit, TreeLimits,
    },
    resource_mapper::ConfiguredResourceRoot,
    runtime::{HostError, LiveRunnerExit, LiveRunnerHandle, ProjectIntentAuthority},
    scheduler::RuntimeOwner,
    store::PrepareTurnCommand,
};
use crate::project_registry::ProjectRootAuthority;

pub(super) struct DatabaseAuthority(pub(super) crate::persistence::Database);

impl ProjectIntentAuthority for DatabaseAuthority {
    fn current_snapshot(&self, command: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        super::super::load_intent_snapshot(
            &self.0,
            command.project_id.clone(),
            command.project_generation,
            command.chat_id.clone(),
        )
        .ok()
    }
}

use super::{
    super::{RuntimeSlot, runtime_slot::RuntimeSlotError},
    types::{
        BINDING_DIGEST_HEX, BINDING_ID, CharacterizationArtifact, CharacterizationCase,
        CharacterizationError,
    },
};

pub(super) fn validate_session_binding(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
) -> Result<(), CharacterizationError> {
    let binding = database
        .connection()
        .query_row(
            "SELECT binding_id,binding_digest FROM agent_sessions
             WHERE project_id=?1 AND chat_id=?2 AND id=?3",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str()
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|_| CharacterizationError::Session)?
        .ok_or(CharacterizationError::Intent)?;
    (binding.0 == BINDING_ID && binding.1 == BINDING_DIGEST_HEX)
        .then_some(())
        .ok_or(CharacterizationError::Intent)
}

pub(super) fn load_roots(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
) -> Result<Vec<ConfiguredResourceRoot>, CharacterizationError> {
    let connection = database.connection();
    let mut statement = connection
        .prepare(
            "SELECT id, absolute_path FROM project_roots
             WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9",
        )
        .map_err(|_| CharacterizationError::Session)?;
    let rows = statement
        .query_map(params![owner.project_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| CharacterizationError::Session)?;
    let mut roots = Vec::new();
    for row in rows {
        if roots.len() == 8 {
            return Err(CharacterizationError::Capacity);
        }
        let (id, path) = row.map_err(|_| CharacterizationError::Session)?;
        roots.push(ConfiguredResourceRoot {
            root_id: SageRootId::parse(id).map_err(|_| CharacterizationError::Intent)?,
            canonical_native_root: PathBuf::from(path),
        });
    }
    Ok(roots)
}

pub(super) fn launch_spec(
    artifact: &CharacterizationArtifact,
    case: CharacterizationCase,
    owner: &RuntimeOwner,
    project_root: ProjectRootAuthority,
    cleanup_deadline: Instant,
) -> Result<LaunchSpec, CharacterizationError> {
    Ok(LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: owner.runtime_epoch.get(),
        artifact: ArtifactExpectation::new(
            Uuid::nil(),
            artifact.path.clone(),
            artifact.sha256,
            artifact.bytes,
        )
        .map_err(|_| CharacterizationError::Artifact)?,
        fixed_args: BoundedOsArgs::new(vec![
            OsString::from("--case"),
            OsString::from(case.argument()),
            OsString::from("--seed"),
            OsString::from("0"),
        ])
        .map_err(|_| CharacterizationError::Artifact)?,
        environment: BoundedEnvironment::new(Vec::new())
            .map_err(|_| CharacterizationError::Artifact)?,
        project_root,
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: Some(cleanup_deadline),
        #[cfg(test)]
        test_after_artifact_verified: None,
    })
}

pub(super) fn observe_runner(
    slot: &Arc<RuntimeSlot>,
    owner: &RuntimeOwner,
    runner: &Arc<LiveRunnerHandle>,
) -> Result<(), CharacterizationError> {
    let slot: Weak<RuntimeSlot> = Arc::downgrade(slot);
    let owner = owner.clone();
    runner
        .observe_exit(move |exit| {
            if exit == LiveRunnerExit::Failed
                && let Some(slot) = slot.upgrade()
            {
                let _ = slot.mark_recovery(&owner);
            }
        })
        .map_err(|_| CharacterizationError::Recovery)
}

pub(super) fn map_host_error(
    slot: &RuntimeSlot,
    owner: &RuntimeOwner,
    error: HostError,
) -> CharacterizationError {
    match classify_host_error(error) {
        CharacterizationError::Recovery => {
            let _ = slot.mark_recovery(owner);
            CharacterizationError::Recovery
        }
        error => error,
    }
}

fn classify_host_error(error: HostError) -> CharacterizationError {
    match error {
        HostError::Intent | HostError::ConfigMutationRejected => CharacterizationError::Intent,
        HostError::Capacity => CharacterizationError::Capacity,
        HostError::Unavailable
        | HostError::State
        | HostError::DeliveryUnknown
        | HostError::RetryExhausted
        | HostError::RecoveryRequired
        | HostError::Deadline
        | HostError::Closed => CharacterizationError::Recovery,
    }
}

pub(super) fn converge_failed_runtime(
    slot: &RuntimeSlot,
    owner: &RuntimeOwner,
    control: &TreeControl,
    cleanup_deadline: Instant,
) -> CharacterizationError {
    let _ = slot.mark_recovery(owner);
    if control
        .request_termination(TreeExit::ParentShutdown)
        .is_err()
        || control.wait_before(cleanup_deadline).is_err()
    {
        CharacterizationError::ProcessRecovery
    } else {
        CharacterizationError::Recovery
    }
}

pub(super) fn deadlines() -> Result<(Instant, Instant), CharacterizationError> {
    let cleanup = Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.ordinary_rpc);
    let reserve = Duration::from_millis(
        LIMITS
            .deadlines_ms
            .tree_termination
            .saturating_mul(2)
            .saturating_add(LIMITS.deadlines_ms.wal_convergence),
    );
    let operation = cleanup
        .checked_sub(reserve)
        .ok_or(CharacterizationError::Capacity)?;
    (operation > Instant::now())
        .then_some((operation, cleanup))
        .ok_or(CharacterizationError::Capacity)
}

pub(super) fn command_owner(command: &PrepareTurnCommand) -> RuntimeOwner {
    RuntimeOwner {
        project_id: command.project_id.clone(),
        chat_id: command.chat_id.clone(),
        session_id: command.session_id.clone(),
        runtime_epoch: command.runtime_epoch,
    }
}

pub(super) fn native_text(path: &Path) -> Result<&str, CharacterizationError> {
    path.to_str().ok_or(CharacterizationError::Intent)
}

pub(super) fn map_slot(error: RuntimeSlotError) -> CharacterizationError {
    match error {
        RuntimeSlotError::Occupied => CharacterizationError::Capacity,
        RuntimeSlotError::Foreign => CharacterizationError::Intent,
        RuntimeSlotError::Stopping | RuntimeSlotError::Recovery => CharacterizationError::Recovery,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_failures_are_fail_closed_as_intent_capacity_or_recovery() {
        assert_eq!(
            classify_host_error(HostError::Intent),
            CharacterizationError::Intent
        );
        assert_eq!(
            classify_host_error(HostError::ConfigMutationRejected),
            CharacterizationError::Intent
        );
        assert_eq!(
            classify_host_error(HostError::Capacity),
            CharacterizationError::Capacity
        );
        for error in [
            HostError::Unavailable,
            HostError::State,
            HostError::DeliveryUnknown,
            HostError::RetryExhausted,
            HostError::RecoveryRequired,
            HostError::Deadline,
            HostError::Closed,
        ] {
            assert_eq!(classify_host_error(error), CharacterizationError::Recovery);
        }
    }
}
