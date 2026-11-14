use std::{
    thread,
    time::{Duration, Instant},
};

use sage_core::limits::PREVIEW_LIMITS;

use super::{
    discovery::PreviewTargetRef,
    error::PreviewError,
    health,
    lifecycle::{
        PreviewObserver, PreviewStopResponse, cleanup_owned_preview, ensure_owner, matches_target,
        transition_for_owner, transition_locked,
    },
    lifecycle_registry,
    projection::{PreviewFailure, PreviewPhase, PreviewProjectionOwner},
};

pub(crate) fn health_observed(
    target: &PreviewTargetRef,
    observer: &PreviewObserver<'_>,
) -> Result<Option<String>, PreviewError> {
    let owner = PreviewProjectionOwner::from_target(target);
    let observation = lifecycle_registry::with_registry(|registry| {
        ensure_owner(registry, &owner)?;
        let exited = registry
            .active
            .as_mut()
            .filter(|active| matches_target(&active.target, target))
            .ok_or_else(|| {
                PreviewError::conflict("previewTarget", "Preview target is not active.")
            })?
            .child
            .exited();
        if exited {
            return Ok((None, registry.move_active_to_cleanup(target)));
        }
        let origin = registry
            .active
            .as_ref()
            .expect("active Preview was checked above")
            .origin
            .clone();
        transition_locked(
            registry,
            PreviewPhase::HealthChecking,
            None,
            Some(target.clone()),
            Some(origin.clone()),
            observer,
        )?;
        Ok((Some(origin), false))
    });
    let (origin, exited) = match observation {
        Ok(value) => value,
        Err(error) => {
            cleanup_active_preview(target)?;
            return Err(error);
        }
    };
    if exited {
        cleanup_owned_preview(target)?;
        transition_for_owner(
            owner,
            PreviewPhase::Exited,
            Some(PreviewFailure::ExitedEarly),
            Some(target.clone()),
            None,
            observer,
        )?;
        return Ok(None);
    }
    let Some(origin) = origin else {
        return Ok(None);
    };
    let deadline = Instant::now() + Duration::from_millis(PREVIEW_LIMITS.health_deadline_ms);
    loop {
        if health::get(Some(&origin)) {
            if let Err(error) = transition_for_owner(
                owner,
                PreviewPhase::Active,
                None,
                Some(target.clone()),
                Some(origin.clone()),
                observer,
            ) {
                cleanup_active_preview(target)?;
                return Err(error);
            }
            return Ok(Some(origin));
        }
        if active_exited(target) {
            cleanup_active_preview(target)?;
            transition_for_owner(
                owner,
                PreviewPhase::Exited,
                Some(PreviewFailure::ExitedEarly),
                Some(target.clone()),
                None,
                observer,
            )?;
            return Ok(None);
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
    }
    cleanup_active_preview(target)?;
    transition_for_owner(
        owner,
        PreviewPhase::HealthFailed,
        Some(PreviewFailure::HealthTimeout),
        Some(target.clone()),
        None,
        observer,
    )?;
    Ok(None)
}

fn active_exited(target: &PreviewTargetRef) -> bool {
    lifecycle_registry::with_registry(|registry| {
        registry
            .active
            .as_mut()
            .filter(|active| matches_target(&active.target, target))
            .is_some_and(|active| active.child.exited())
    })
}

fn cleanup_active_preview(target: &PreviewTargetRef) -> Result<(), PreviewError> {
    let owns_cleanup = lifecycle_registry::with_registry(|registry| {
        registry.move_active_to_cleanup(target) || registry.cleanup_matches(target)
    });
    if owns_cleanup {
        cleanup_owned_preview(target)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn stop(target: &PreviewTargetRef) -> Result<PreviewStopResponse, PreviewError> {
    let observer = |_projection: &super::projection::PreviewProjection| Ok(());
    stop_observed(target, &observer)
}

pub(crate) fn stop_observed(
    target: &PreviewTargetRef,
    observer: &PreviewObserver<'_>,
) -> Result<PreviewStopResponse, PreviewError> {
    let owner = PreviewProjectionOwner::from_target(target);
    let (duplicate, already_stopping) = lifecycle_registry::with_registry(|registry| {
        ensure_owner(registry, &owner)?;
        let duplicate = registry
            .projection
            .as_ref()
            .is_some_and(|projection| projection.phase == PreviewPhase::Stopped)
            && registry.active.is_none()
            && registry.starting.is_none()
            && registry.cleanup.is_none();
        let already_stopping = registry
            .projection
            .as_ref()
            .is_some_and(|projection| projection.phase == PreviewPhase::Stopping);
        Ok((duplicate, already_stopping))
    })?;
    if duplicate {
        return Ok(PreviewStopResponse {
            ref_: target.clone(),
        });
    }
    let owns_child = lifecycle_registry::with_registry(|registry| {
        registry
            .active
            .as_ref()
            .is_some_and(|active| matches_target(&active.target, target))
            || registry
                .starting
                .as_ref()
                .is_some_and(|starting| matches_target(&starting.target, target))
            || registry.cleanup_matches(target)
    });
    if !owns_child {
        return Err(PreviewError::conflict(
            "previewTarget",
            "Preview target is not active.",
        ));
    }
    if !already_stopping
        && let Err(error) = transition_for_owner(
            owner.clone(),
            PreviewPhase::Stopping,
            None,
            Some(target.clone()),
            None,
            observer,
        )
    {
        cleanup_active_or_starting_preview(target)?;
        return Err(error);
    }
    cleanup_active_or_starting_preview(target)?;
    transition_for_owner(
        owner,
        PreviewPhase::Stopped,
        None,
        Some(target.clone()),
        None,
        observer,
    )?;
    Ok(PreviewStopResponse {
        ref_: target.clone(),
    })
}

fn cleanup_active_or_starting_preview(target: &PreviewTargetRef) -> Result<(), PreviewError> {
    let owns_cleanup = lifecycle_registry::with_registry(|registry| {
        registry.move_owned_to_cleanup(target) || registry.cleanup_matches(target)
    });
    if !owns_cleanup || !cleanup_owned_preview(target)? {
        return Err(PreviewError::internal("Preview child ownership was lost."));
    }
    Ok(())
}
