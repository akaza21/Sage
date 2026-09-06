//! Process lifecycle state with isolation for parallel native tests.
//!
//! Production has one registry because Sage has one current Project. Unit tests
//! receive a registry per test worker thread, so one test cannot reserve or
//! cancel another test's child while Cargo runs tests in parallel.

#[cfg(test)]
use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};

use super::{
    discovery::PreviewTargetRef, lifecycle_process::ManagedChild, projection::PreviewProjection,
};
#[cfg(windows)]
use crate::acp::process::PreviewLaunchRecovery;

pub(super) struct ActivePreview {
    pub(super) target: PreviewTargetRef,
    pub(super) origin: String,
    pub(super) child: ManagedChild,
}

pub(super) struct StartingPreview {
    pub(super) target: PreviewTargetRef,
    pub(super) child: ManagedChild,
}

// This is deliberately an owned slot rather than a boolean. A child leaves
// `active`/`starting` before bounded cleanup so the registry must continue to
// own both the capacity claim and the exact handle until tree-zero is proven.
pub(super) struct CleanupPreview {
    pub(super) target: PreviewTargetRef,
    pub(super) child: Arc<Mutex<ManagedChild>>,
}

#[derive(Default)]
pub(super) struct Registry {
    pub(super) discovered: HashMap<PreviewTargetRef, std::path::PathBuf>,
    pub(super) starting: Option<StartingPreview>,
    pub(super) active: Option<ActivePreview>,
    pub(super) cleanup: Option<CleanupPreview>,
    pub(super) projection: Option<PreviewProjection>,
    pub(super) unsettled_drainers: Vec<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    pub(super) retained_recovery: Option<PreviewLaunchRecovery>,
    #[cfg(windows)]
    pub(super) recovery_slot_reserved: bool,
}

#[cfg(test)]
thread_local! {
    static TEST_REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

#[cfg(not(test))]
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub(super) fn with_registry<T>(callback: impl FnOnce(&mut Registry) -> T) -> T {
    #[cfg(test)]
    {
        TEST_REGISTRY.with(|registry| callback(&mut registry.borrow_mut()))
    }

    #[cfg(not(test))]
    {
        let mut registry = registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        callback(&mut registry)
    }
}
