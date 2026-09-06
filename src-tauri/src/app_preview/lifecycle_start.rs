use std::{
    thread,
    time::{Duration, Instant},
};

use crate::acp::limits::PREVIEW_LIMITS;

use super::{
    discovery::PreviewTargetRef,
    error::PreviewError,
    health,
    lifecycle::{
        ActivePreview, PreviewObserver, PreviewStartReservation, PreviewStartResponse,
        StartingPreview, matches_target, stop_starting, transition_for_owner, transition_locked,
    },
    lifecycle_process::{StartupObservation, wait_for_origin},
    lifecycle_registry,
    projection::{PreviewFailure, PreviewPhase, PreviewProjectionOwner},
};

pub(crate) fn fail_start_without_child(
    target: &PreviewTargetRef,
    failure: PreviewFailure,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    transition_for_owner(
        PreviewProjectionOwner::from_target(target),
        PreviewPhase::HealthFailed,
        Some(failure),
        Some(target.clone()),
        None,
        observer,
    )
}

pub(crate) fn finish_start(
    PreviewStartReservation {
        target,
        stdout,
        stderr,
    }: PreviewStartReservation,
    observer: &PreviewObserver<'_>,
) -> Result<PreviewStartResponse, PreviewError> {
    let (startup, drainers) = wait_for_origin(&target, stdout, stderr);
    let drainer_deadline =
        Instant::now() + Duration::from_millis(PREVIEW_LIMITS.process_termination_deadline_ms);
    let unattached = lifecycle_registry::with_registry(|registry| {
        match registry
            .starting
            .as_mut()
            .filter(|starting| matches_target(&starting.target, &target))
        {
            Some(starting) => starting
                .child
                .attach_drainers(drainers, drainer_deadline)
                .err(),
            None => Some(drainers),
        }
    });
    if let Some(drainers) = unattached {
        if let Err(drainers) =
            super::lifecycle_process::join_unattached_drainers_before(drainers, drainer_deadline)
        {
            super::lifecycle::retain_unsettled_drainers(drainers);
            return Err(PreviewError::internal(
                "Preview output drainers did not settle before their deadline.",
            ));
        }
        return Err(PreviewError::conflict(
            "activePreview",
            "Preview ownership changed during startup.",
        ));
    }
    let origin = match startup {
        StartupObservation::Origin(origin) => origin,
        StartupObservation::OriginNotLoopback => {
            fail_starting(
                &target,
                PreviewPhase::HealthFailed,
                PreviewFailure::OriginNotLoopback,
                observer,
            )?;
            return Err(PreviewError::Unavailable {
                message: "Preview script exposed a non-loopback origin.".into(),
            });
        }
        StartupObservation::ExitedEarly => {
            fail_starting(
                &target,
                PreviewPhase::Exited,
                PreviewFailure::ExitedEarly,
                observer,
            )?;
            return Err(PreviewError::Unavailable {
                message: "Preview script exited before activation.".into(),
            });
        }
        StartupObservation::Timeout => {
            fail_starting(
                &target,
                PreviewPhase::HealthFailed,
                PreviewFailure::StartupTimeout,
                observer,
            )?;
            return Err(PreviewError::Unavailable {
                message: "Preview script did not expose a loopback origin in time.".into(),
            });
        }
    };
    if let Err(error) = transition_for_owner(
        PreviewProjectionOwner::from_target(&target),
        PreviewPhase::HealthChecking,
        None,
        Some(target.clone()),
        Some(origin.clone()),
        observer,
    ) {
        stop_starting(&target)?;
        return Err(error);
    }
    let deadline = Instant::now() + Duration::from_millis(PREVIEW_LIMITS.health_deadline_ms);
    loop {
        if starting_exited(&target) {
            fail_starting(
                &target,
                PreviewPhase::Exited,
                PreviewFailure::ExitedEarly,
                observer,
            )?;
            return Err(PreviewError::Unavailable {
                message: "Preview script exited before activation.".into(),
            });
        }
        if health::get(Some(&origin)) {
            break;
        }
        if Instant::now() >= deadline {
            fail_starting(
                &target,
                PreviewPhase::HealthFailed,
                PreviewFailure::HealthTimeout,
                observer,
            )?;
            return Err(PreviewError::Unavailable {
                message: "Preview origin did not become healthy in time.".into(),
            });
        }
        thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
    }
    if let Err(error) = promote_start(&target, &origin, observer) {
        stop_starting(&target)?;
        return Err(error);
    }
    Ok(PreviewStartResponse {
        ref_: target,
        origin,
    })
}

pub(crate) fn fail_start_after_reservation(
    target: &PreviewTargetRef,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    fail_starting(
        target,
        PreviewPhase::HealthFailed,
        PreviewFailure::Internal,
        observer,
    )
}

fn promote_start(
    target: &PreviewTargetRef,
    origin: &str,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let promoted = lifecycle_registry::with_registry(|registry| {
        let still_current = registry
            .starting
            .as_ref()
            .is_some_and(|starting| matches_target(&starting.target, target))
            && registry.discovered.contains_key(target)
            && registry.active.is_none();
        if !still_current {
            return Ok(false);
        }
        transition_locked(
            registry,
            PreviewPhase::Active,
            None,
            Some(target.clone()),
            Some(origin.to_owned()),
            observer,
        )?;
        let Some(StartingPreview { child, .. }) = registry.starting.take() else {
            return Err(PreviewError::internal(
                "Preview start reservation was lost.",
            ));
        };
        registry.active = Some(ActivePreview {
            target: target.clone(),
            origin: origin.to_owned(),
            child,
        });
        Ok(true)
    })?;
    if !promoted {
        stop_starting(target)?;
        return Err(PreviewError::conflict(
            "activePreview",
            "A Preview target is already active.",
        ));
    }
    Ok(())
}

fn fail_starting(
    target: &PreviewTargetRef,
    phase: PreviewPhase,
    failure: PreviewFailure,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    stop_starting(target)?;
    transition_for_owner(
        PreviewProjectionOwner::from_target(target),
        phase,
        Some(failure),
        Some(target.clone()),
        None,
        observer,
    )
}

pub(super) fn starting_exited(target: &PreviewTargetRef) -> bool {
    lifecycle_registry::with_registry(|registry| {
        registry
            .starting
            .as_mut()
            .filter(|starting| matches_target(&starting.target, target))
            .is_some_and(|starting| starting.child.exited())
    })
}
