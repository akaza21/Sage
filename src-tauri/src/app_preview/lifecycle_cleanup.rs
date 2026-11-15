use super::{
    discovery::PreviewTargetRef,
    error::PreviewError,
    lifecycle::{PreviewObserver, cleanup_owned_preview, transition_locked},
    lifecycle_registry,
    projection::{PreviewPhase, PreviewProjectionOwner},
};

use sage_acp_protocol::scheduler::RuntimeOwner;

pub(crate) fn retire_runtime_owner(owner: &RuntimeOwner) -> Result<(), PreviewError> {
    let observer = |_projection: &super::projection::PreviewProjection| Ok(());
    let (projection, cleanup_target) = lifecycle_registry::with_registry(|registry| {
        let Some(current) = registry.projection.as_ref() else {
            return Ok((None, None));
        };
        if !matches_runtime_owner(&current.owner, owner) {
            return Ok((None, None));
        }
        if current.phase == PreviewPhase::Stopped && registry.cleanup.is_none() {
            registry.projection = None;
            return Ok((None, None));
        }
        let projection = current.owner.clone();
        if registry
            .projection
            .as_ref()
            .is_some_and(|current| current.phase != PreviewPhase::Stopping)
        {
            transition_locked(
                registry,
                PreviewPhase::Stopping,
                None,
                None,
                None,
                &observer,
            )?;
        }
        let target = cleanup_target_for_runtime(registry, owner);
        if let Some(target) = target.as_ref() {
            let _ = registry.move_owned_to_cleanup(target);
        }
        Ok((Some(projection), target))
    })?;
    let Some(projection) = projection else {
        return Ok(());
    };
    if let Some(target) = cleanup_target.as_ref() {
        cleanup_owned_preview(target)?;
    }
    lifecycle_registry::with_registry(|registry| {
        if registry
            .projection
            .as_ref()
            .is_some_and(|current| current.owner == projection)
            && registry.cleanup.is_none()
        {
            transition_locked(registry, PreviewPhase::Stopped, None, None, None, &observer)?;
            registry.projection = None;
        }
        Ok(())
    })
}

fn matches_runtime_owner(preview: &PreviewProjectionOwner, runtime: &RuntimeOwner) -> bool {
    preview.project_id == runtime.project_id
        && preview.chat_id == runtime.chat_id
        && preview.session_id == runtime.session_id
        && preview.runtime_epoch == runtime.runtime_epoch
}

fn cleanup_target_for_runtime(
    registry: &super::lifecycle::Registry,
    runtime: &RuntimeOwner,
) -> Option<PreviewTargetRef> {
    registry
        .cleanup
        .as_ref()
        .map(|cleanup| &cleanup.target)
        .or_else(|| registry.starting.as_ref().map(|starting| &starting.target))
        .or_else(|| registry.active.as_ref().map(|active| &active.target))
        .filter(|target| matches_target_runtime(target, runtime))
        .cloned()
}

fn matches_target_runtime(target: &PreviewTargetRef, runtime: &RuntimeOwner) -> bool {
    target.project_id == runtime.project_id
        && target.chat_id == runtime.chat_id
        && target.session_id == runtime.session_id
        && target.runtime_epoch == runtime.runtime_epoch
}

pub(crate) fn stop_before_epoch(new_epoch: u64) {
    let _ = stop_before_epoch_with_observer(new_epoch, &|_| Ok(()));
}

#[cfg(test)]
pub(crate) fn stop_before_epoch_observed(
    new_epoch: u64,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    stop_before_epoch_with_observer(new_epoch, observer)
}

fn stop_before_epoch_with_observer(
    new_epoch: u64,
    observer: &PreviewObserver<'_>,
) -> Result<(), PreviewError> {
    let (stale_target, transition_error) = lifecycle_registry::with_registry(|registry| {
        registry
            .discovered
            .retain(|target, _| target.expected_transition_epoch >= new_epoch);
        let stale_target = registry
            .cleanup
            .as_ref()
            .map(|cleanup| &cleanup.target)
            .or_else(|| registry.starting.as_ref().map(|starting| &starting.target))
            .or_else(|| registry.active.as_ref().map(|active| &active.target))
            .filter(|target| target.expected_transition_epoch < new_epoch)
            .cloned();
        let transition_error = stale_target.as_ref().and_then(|target| {
            transition_locked(
                registry,
                PreviewPhase::Stopping,
                None,
                Some(target.clone()),
                None,
                observer,
            )
            .err()
        });
        if let Some(target) = stale_target.as_ref() {
            let _ = registry.move_owned_to_cleanup(target);
        }
        (stale_target, transition_error)
    });
    if let Some(target) = stale_target.as_ref() {
        cleanup_owned_preview(target)?;
    }
    let stopped_error = lifecycle_registry::with_registry(|registry| {
        if registry.cleanup.is_none()
            && registry
                .projection
                .as_ref()
                .is_some_and(|projection| projection.owner.transition_epoch < new_epoch)
        {
            let error = if transition_error.is_none() {
                transition_locked(registry, PreviewPhase::Stopped, None, None, None, observer).err()
            } else {
                None
            };
            registry.projection = None;
            return error;
        }
        None
    });
    transition_error.or(stopped_error).map_or(Ok(()), Err)
}
