#[cfg(test)]
pub(crate) use super::lifecycle_active::stop;
pub(crate) use super::lifecycle_active::{health_observed, stop_observed};
pub(crate) use super::lifecycle_cleanup::retire_runtime_owner;
pub(crate) use super::lifecycle_cleanup::stop_before_epoch;
#[cfg(test)]
pub(crate) use super::lifecycle_cleanup::stop_before_epoch_observed;
#[cfg(test)]
pub(super) use super::lifecycle_process::{
    test_channel_slots_admitted, test_deadline_millis_admitted, test_drainer_count_admitted,
    test_startup_output_bytes_admitted,
};
pub(super) use super::lifecycle_registry::{ActivePreview, Registry, StartingPreview};
#[cfg(test)]
pub(super) use super::lifecycle_slot::{CleanupTestStep, test_cleanup_occupied};
pub(super) use super::lifecycle_slot::{cleanup_owned_preview, stop_starting};
pub(super) use super::lifecycle_start::starting_exited;
pub(crate) use super::lifecycle_start::{
    fail_start_after_reservation, fail_start_without_child, finish_start,
};
use super::{
    discovery::{PreviewDiscoveryRequest, PreviewTargetRef},
    error::PreviewError,
    lifecycle_process::ManagedChild,
    lifecycle_registry,
    projection::{PreviewFailure, PreviewPhase, PreviewProjection, PreviewProjectionOwner},
};
use serde::Serialize;
use std::io::Read;
#[cfg(not(windows))]
use std::process::{Command, Stdio};
pub(crate) type PreviewObserver<'a> =
    dyn Fn(&PreviewProjection) -> Result<(), PreviewError> + Send + Sync + 'a;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStartResponse {
    #[serde(rename = "ref")]
    pub ref_: PreviewTargetRef,
    pub origin: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStopResponse {
    #[serde(rename = "ref")]
    pub ref_: PreviewTargetRef,
}
pub(crate) struct PreviewStartReservation {
    pub(super) target: PreviewTargetRef,
    pub(super) stdout: Box<dyn Read + Send>,
    pub(super) stderr: Box<dyn Read + Send>,
}
pub(crate) fn begin_discovery(
    request: &PreviewDiscoveryRequest,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    transition_for_owner(
        PreviewProjectionOwner::from_discovery(request),
        PreviewPhase::Discovering,
        None,
        None,
        None,
        observer,
    )
}
pub(crate) fn finish_discovery(
    request: &PreviewDiscoveryRequest,
    targets: Vec<(PreviewTargetRef, std::path::PathBuf)>,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let missing = targets.is_empty();
    lifecycle_registry::with_registry(|registry| {
        ensure_owner(registry, &PreviewProjectionOwner::from_discovery(request))?;
        transition_locked(
            registry,
            PreviewPhase::Idle,
            missing.then_some(PreviewFailure::ScriptMissing),
            None,
            None,
            observer,
        )?;
        registry.discovered.clear();
        registry.discovered.extend(targets);
        Ok(())
    })
}
pub(crate) fn begin_start(
    target: &PreviewTargetRef,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let owner = PreviewProjectionOwner::from_target(target);
    lifecycle_registry::with_registry(|registry| {
        ensure_owner(registry, &owner)?;
        let phase_busy = registry.projection.as_ref().is_some_and(|projection| {
            matches!(
                projection.phase,
                PreviewPhase::Starting
                    | PreviewPhase::WaitingForOrigin
                    | PreviewPhase::HealthChecking
                    | PreviewPhase::Active
                    | PreviewPhase::Stopping
            )
        });
        if registry.active.is_some()
            || registry.starting.is_some()
            || registry.cleanup.is_some()
            || phase_busy
        {
            record_failure_locked(registry, PreviewFailure::Conflict, observer)?;
            return Err(PreviewError::conflict(
                "activePreview",
                "A Preview target is already active.",
            ));
        }
        if !registry.discovered.contains_key(target) {
            transition_locked(
                registry,
                PreviewPhase::Starting,
                None,
                Some(target.clone()),
                None,
                observer,
            )?;
            transition_locked(
                registry,
                PreviewPhase::HealthFailed,
                Some(PreviewFailure::ScriptMissing),
                Some(target.clone()),
                None,
                observer,
            )?;
            return Err(PreviewError::conflict(
                "previewTarget",
                "Preview target must be discovered before it can start.",
            ));
        }
        transition_locked(
            registry,
            PreviewPhase::Starting,
            None,
            Some(target.clone()),
            None,
            observer,
        )
    })
}
pub(crate) fn mark_waiting_for_origin(
    target: &PreviewTargetRef,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    transition_for_owner(
        PreviewProjectionOwner::from_target(target),
        PreviewPhase::WaitingForOrigin,
        None,
        Some(target.clone()),
        None,
        observer,
    )
}
pub(super) fn transition_for_owner(
    owner: PreviewProjectionOwner,
    phase: PreviewPhase,
    failure: Option<PreviewFailure>,
    target: Option<PreviewTargetRef>,
    origin: Option<String>,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    lifecycle_registry::with_registry(|registry| {
        let current = if registry.projection.as_ref().map(|value| &value.owner) == Some(&owner) {
            registry
                .projection
                .as_ref()
                .cloned()
                .ok_or_else(|| PreviewError::internal("Preview projection is unavailable."))?
        } else {
            if registry.active.is_some()
                || registry.starting.is_some()
                || registry.cleanup.is_some()
            {
                return Err(PreviewError::conflict(
                    "activePreview",
                    "A foreign Preview owner is active.",
                ));
            }
            PreviewProjection::initial(owner)?
        };
        let mut next = current.advance(phase, failure, origin)?;
        next.target = target.or(current.target);
        // Observer rejection is a veto: neither a fresh owner nor a phase
        // advance becomes visible until its observer accepted this exact value.
        observer(&next)?;
        registry.projection = Some(next);
        Ok(())
    })
}
pub(super) fn ensure_owner(
    registry: &Registry,
    owner: &PreviewProjectionOwner,
) -> Result<(), PreviewError> {
    if registry.projection.as_ref().map(|value| &value.owner) == Some(owner) {
        Ok(())
    } else {
        Err(PreviewError::conflict(
            "previewOwner",
            "Preview owner is stale.",
        ))
    }
}
pub(super) fn transition_locked(
    registry: &mut Registry,
    phase: PreviewPhase,
    failure: Option<PreviewFailure>,
    target: Option<PreviewTargetRef>,
    origin: Option<String>,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let current = registry
        .projection
        .as_ref()
        .ok_or_else(|| PreviewError::internal("Preview projection is unavailable."))?;
    let mut next = current.advance(phase, failure, origin)?;
    next.target = target.or_else(|| current.target.clone());
    observer(&next)?;
    registry.projection = Some(next);
    Ok(())
}
fn record_failure_locked(
    registry: &mut Registry,
    failure: PreviewFailure,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let current = registry
        .projection
        .as_ref()
        .ok_or_else(|| PreviewError::internal("Preview projection is unavailable."))?;
    let next = current.with_failure(failure)?;
    observer(&next)?;
    registry.projection = Some(next);
    Ok(())
}
#[cfg(test)]
pub(crate) fn replace_discovered(
    request: &PreviewDiscoveryRequest,
    targets: Vec<(PreviewTargetRef, std::path::PathBuf)>,
) {
    lifecycle_registry::with_registry(|registry| {
        if registry.active.is_none() && registry.starting.is_none() && registry.cleanup.is_none() {
            registry.projection =
                PreviewProjection::initial(PreviewProjectionOwner::from_discovery(request)).ok();
        }
        registry.discovered.clear();
        registry.discovered.extend(targets);
    });
}

pub(crate) fn reserve_start(
    target: PreviewTargetRef,
) -> Result<PreviewStartReservation, PreviewError> {
    let (stdout, stderr) = lifecycle_registry::with_registry(|registry| {
        if !settle_unsettled_drainers_locked(registry) {
            return Err(PreviewError::internal(
                "Preview output drainers did not settle before their deadline.",
            ));
        }
        if registry.active.is_some() || registry.starting.is_some() || registry.cleanup.is_some() {
            return Err(PreviewError::conflict(
                "activePreview",
                "A Preview target is already active.",
            ));
        }
        ensure_owner(registry, &PreviewProjectionOwner::from_target(&target))?;
        let package_root = registry.discovered.get(&target).cloned().ok_or_else(|| {
            PreviewError::conflict(
                "previewTarget",
                "Preview target must be discovered before it can start.",
            )
        })?;
        #[cfg(windows)]
        let (child, stdout, stderr) = {
            if !super::lifecycle_windows::reserve_recovery_slot(registry) {
                return Err(PreviewError::internal(
                    "Preview retained process recovery has not converged to zero.",
                ));
            }
            let (child, stdout, stderr) = match super::lifecycle_windows::PreviewProcess::launch(
                &package_root,
                target.script_name.as_str(),
            ) {
                Ok(value) => value,
                Err(failure) => {
                    super::lifecycle_windows::retain_recovery(registry, failure);
                    return Err(PreviewError::Unavailable {
                        message: "Preview could not start its curated package script.".into(),
                    });
                }
            };
            super::lifecycle_windows::release_recovery_slot(registry);
            (
                ManagedChild::reserved(child),
                Box::new(stdout) as Box<dyn Read + Send>,
                Box::new(stderr) as Box<dyn Read + Send>,
            )
        };
        #[cfg(not(windows))]
        let (child, stdout, stderr) = {
            use super::lifecycle_process::kill_and_reap_unreserved;
            let mut command = Command::new(npm_executable());
            command
                .args(["run", target.script_name.as_str()])
                .current_dir(package_root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            let mut raw_child = command.spawn().map_err(|_| PreviewError::Unavailable {
                message: "Preview could not start its curated package script.".into(),
            })?;
            let stdout = raw_child.stdout.take().ok_or_else(|| {
                let _ = kill_and_reap_unreserved(&mut raw_child);
                PreviewError::internal("Preview stdout ownership was unavailable.")
            })?;
            let stderr = raw_child.stderr.take().ok_or_else(|| {
                let _ = kill_and_reap_unreserved(&mut raw_child);
                PreviewError::internal("Preview stderr ownership was unavailable.")
            })?;
            let child = ManagedChild::reserve(raw_child).map_err(|mut raw_child| {
                let _ = kill_and_reap_unreserved(&mut raw_child);
                PreviewError::internal("Preview could not reserve its process tree.")
            })?;
            (
                child,
                Box::new(stdout) as Box<dyn Read + Send>,
                Box::new(stderr) as Box<dyn Read + Send>,
            )
        };
        registry.starting = Some(StartingPreview {
            target: target.clone(),
            child,
        });
        Ok((stdout, stderr))
    })?;
    Ok(PreviewStartReservation {
        target,
        stdout,
        stderr,
    })
}

pub(super) fn retain_unsettled_drainers(drainers: Vec<std::thread::JoinHandle<()>>) {
    lifecycle_registry::with_registry(|registry| {
        // A reservation is consumed once, and new reservations are refused
        // while this registry slot is nonempty. Retain every handle even if an
        // invariant violation reaches here; dropping would detach a drainer.
        registry.unsettled_drainers.extend(drainers);
    });
}

fn settle_unsettled_drainers_locked(registry: &mut Registry) -> bool {
    if registry.unsettled_drainers.is_empty() {
        return true;
    }
    let drainers = std::mem::take(&mut registry.unsettled_drainers);
    match super::lifecycle_process::join_unattached_drainers_before(
        drainers,
        std::time::Instant::now()
            + std::time::Duration::from_millis(
                crate::acp::limits::PREVIEW_LIMITS.process_termination_deadline_ms,
            ),
    ) {
        Ok(()) => true,
        Err(drainers) => {
            registry.unsettled_drainers = drainers;
            false
        }
    }
}

pub(super) fn matches_target(left: &PreviewTargetRef, right: &PreviewTargetRef) -> bool {
    left == right
}

pub(super) fn stop_child(child: &mut ManagedChild) -> Result<(), PreviewError> {
    child.terminate_and_reap().then_some(()).ok_or_else(|| {
        PreviewError::internal("Preview process tree did not terminate before its deadline.")
    })
}

#[cfg(test)]
pub(super) fn test_owned_child_count() -> usize {
    lifecycle_registry::with_registry(|registry| {
        usize::from(registry.starting.is_some())
            + usize::from(registry.active.is_some())
            + usize::from(registry.cleanup.is_some())
    })
}

#[cfg(test)]
pub(super) fn test_projection() -> Option<PreviewProjection> {
    lifecycle_registry::with_registry(|registry| registry.projection.clone())
}

#[cfg(not(windows))]
fn npm_executable() -> &'static str {
    "npm"
}
