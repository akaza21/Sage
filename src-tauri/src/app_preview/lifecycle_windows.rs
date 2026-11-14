use std::{
    fs::File,
    path::Path,
    time::{Duration, Instant},
};

use crate::acp::process::{PreviewLaunchFailure, PreviewWindowsProcess};

pub(super) fn reserve_recovery_slot(registry: &mut super::lifecycle::Registry) -> bool {
    if registry.recovery_slot_reserved {
        return false;
    }
    let available = if let Some(recovery) = registry.retained_recovery.as_mut() {
        if recovery.retry_zero().is_ok() {
            registry.retained_recovery.take();
            true
        } else {
            false
        }
    } else {
        true
    };
    registry.recovery_slot_reserved = available;
    available
}

pub(super) fn retain_recovery(
    registry: &mut super::lifecycle::Registry,
    failure: PreviewLaunchFailure,
) {
    let PreviewLaunchFailure::Retained(recovery) = failure else {
        registry.recovery_slot_reserved = false;
        return;
    };
    debug_assert!(registry.retained_recovery.is_none());
    registry.retained_recovery = Some(recovery);
    registry.recovery_slot_reserved = false;
}

pub(super) fn release_recovery_slot(registry: &mut super::lifecycle::Registry) {
    registry.recovery_slot_reserved = false;
}

#[cfg(test)]
pub(crate) fn retain_recovery_for_test(
    recovery: crate::acp::process::PreviewLaunchRecovery,
) -> Result<(), sage_process::error::LaunchError> {
    let mut incoming = Some(recovery);
    let retained = super::lifecycle_registry::with_registry(|registry| {
        if registry.retained_recovery.is_some() || !registry.recovery_slot_reserved {
            return false;
        }
        retain_recovery(
            registry,
            crate::acp::process::PreviewLaunchFailure::Retained(
                incoming.take().expect("test recovery must remain owned"),
            ),
        );
        true
    });
    if retained {
        Ok(())
    } else {
        let mut recovery = incoming.expect("unretained test recovery must remain owned");
        if recovery.retry_zero().is_err() {
            let _ = recovery.retry_zero();
        }
        drop(recovery);
        Err(sage_process::error::LaunchError::ContainmentFailure)
    }
}

#[cfg(test)]
pub(crate) fn reserve_recovery_slot_for_test() -> bool {
    super::lifecycle_registry::with_registry(reserve_recovery_slot)
}

#[cfg(test)]
pub(crate) fn retained_recovery_for_test() -> bool {
    super::lifecycle_registry::with_registry(|registry| registry.retained_recovery.is_some())
}

#[cfg(test)]
pub(crate) fn release_recovery_slot_for_test() {
    super::lifecycle_registry::with_registry(release_recovery_slot);
}

#[cfg(test)]
pub(crate) fn cleanup_retained_recovery_for_test() {
    super::lifecycle_registry::with_registry(|registry| {
        registry.recovery_slot_reserved = false;
        if let Some(mut recovery) = registry.retained_recovery.take() {
            if recovery.retry_zero().is_err() {
                let _ = recovery.retry_zero();
            }
            drop(recovery);
        }
    });
}

pub(crate) struct PreviewProcess(PreviewWindowsProcess);

// SAFETY: the adapter owns its exact process and Job handles and is only
// accessed under the Preview registry mutex.
unsafe impl Send for PreviewProcess {}

impl PreviewProcess {
    pub(crate) fn launch(
        package_root: &Path,
        script: &'static str,
    ) -> Result<(Self, File, File), PreviewLaunchFailure> {
        PreviewWindowsProcess::launch_curated_npm(package_root, script)
            .map(|(process, stdout, stderr)| (Self(process), stdout, stderr))
    }

    pub(crate) fn terminate_and_wait_zero(&self) -> bool {
        self.0
            .terminate_and_wait_zero(
                Instant::now()
                    + Duration::from_millis(
                        sage_core::limits::PREVIEW_LIMITS.process_termination_deadline_ms,
                    ),
            )
            .is_ok()
    }

    pub(crate) fn exited(&self) -> bool {
        self.0.exited().unwrap_or(false)
    }
}
