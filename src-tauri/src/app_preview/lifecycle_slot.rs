use super::{
    discovery::PreviewTargetRef,
    error::PreviewError,
    lifecycle::{matches_target, stop_child},
    lifecycle_registry,
    lifecycle_registry::{ActivePreview, CleanupPreview, Registry, StartingPreview},
};
use std::sync::{Arc, Mutex};

pub(super) fn stop_starting(target: &PreviewTargetRef) -> Result<(), PreviewError> {
    let owns_cleanup = lifecycle_registry::with_registry(|registry| {
        registry.move_starting_to_cleanup(target) || registry.cleanup_matches(target)
    });
    if !owns_cleanup {
        return Ok(());
    }
    cleanup_owned_preview(target).map(|_| ())
}

pub(super) fn cleanup_owned_preview(target: &PreviewTargetRef) -> Result<bool, PreviewError> {
    // Keep the registry slot occupied while termination runs, but never hold
    // the registry mutex across the bounded process/drainer wait. The Arc is
    // the slot token: only this exact retained child may release the slot.
    let child = lifecycle_registry::with_registry(|registry| {
        registry
            .cleanup
            .as_ref()
            .filter(|cleanup| matches_target(&cleanup.target, target))
            .map(|cleanup| Arc::clone(&cleanup.child))
    });
    let Some(child) = child else {
        return Ok(false);
    };
    let mut child_guard = child
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    stop_child(&mut child_guard)?;
    drop(child_guard);
    lifecycle_registry::with_registry(|registry| {
        let owns_slot = registry.cleanup.as_ref().is_some_and(|cleanup| {
            matches_target(&cleanup.target, target) && Arc::ptr_eq(&cleanup.child, &child)
        });
        if owns_slot {
            let removed = registry.cleanup.take();
            debug_assert!(removed.is_some());
        }
        Ok(owns_slot)
    })
}

impl Registry {
    pub(super) fn cleanup_matches(&self, target: &PreviewTargetRef) -> bool {
        self.cleanup
            .as_ref()
            .is_some_and(|cleanup| matches_target(&cleanup.target, target))
    }

    pub(super) fn move_active_to_cleanup(&mut self, target: &PreviewTargetRef) -> bool {
        if self.cleanup.is_some() {
            return false;
        }
        let Some(ActivePreview { target, child, .. }) = self
            .active
            .take_if(|active| matches_target(&active.target, target))
        else {
            return false;
        };
        self.cleanup = Some(CleanupPreview {
            target,
            child: Arc::new(Mutex::new(child)),
        });
        true
    }

    pub(super) fn move_starting_to_cleanup(&mut self, target: &PreviewTargetRef) -> bool {
        if self.cleanup.is_some() {
            return false;
        }
        let Some(StartingPreview { target, child }) = self
            .starting
            .take_if(|starting| matches_target(&starting.target, target))
        else {
            return false;
        };
        self.cleanup = Some(CleanupPreview {
            target,
            child: Arc::new(Mutex::new(child)),
        });
        true
    }

    pub(super) fn move_owned_to_cleanup(&mut self, target: &PreviewTargetRef) -> bool {
        self.move_active_to_cleanup(target) || self.move_starting_to_cleanup(target)
    }
}

#[cfg(test)]
pub(super) struct CleanupTestStep {
    target: PreviewTargetRef,
}

#[cfg(test)]
impl CleanupTestStep {
    // The adapter only moves a child already created by `reserve_start`; it
    // cannot synthesize capacity or bypass the production cleanup path.
    pub(super) fn capture(
        target: &PreviewTargetRef,
        drainers: Vec<std::thread::JoinHandle<()>>,
    ) -> Result<Self, PreviewError> {
        lifecycle_registry::with_registry(|registry| {
            if !drainers.is_empty() {
                registry
                    .starting
                    .as_mut()
                    .filter(|starting| matches_target(&starting.target, target))
                    .ok_or_else(|| PreviewError::internal("Preview test child was not starting."))?
                    .child
                    .attach_drainers(
                        drainers,
                        std::time::Instant::now()
                            + std::time::Duration::from_millis(
                                sage_core::limits::PREVIEW_LIMITS.process_termination_deadline_ms,
                            ),
                    )
                    .map_err(|_| {
                        PreviewError::internal("Preview test drainers were not retained.")
                    })?;
            }
            if !registry.move_owned_to_cleanup(target) {
                return Err(PreviewError::internal(
                    "Preview test child could not enter cleanup.",
                ));
            }
            Ok(Self {
                target: target.clone(),
            })
        })
    }

    pub(super) fn finish(&self) -> Result<(), PreviewError> {
        cleanup_owned_preview(&self.target).map(|_| ())
    }
}

#[cfg(test)]
pub(super) fn test_cleanup_occupied(target: &PreviewTargetRef) -> bool {
    lifecycle_registry::with_registry(|registry| registry.cleanup_matches(target))
}
