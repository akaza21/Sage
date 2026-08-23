//! Process lifecycle state with isolation for parallel native tests.
//!
//! Production has one registry because Sage has one current Project. Unit tests
//! receive a registry per test worker thread, so one test cannot reserve or
//! cancel another test's child while Cargo runs tests in parallel.

use std::sync::{Mutex, OnceLock};

use super::lifecycle::Registry;

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
struct Registries {
    by_test_thread: HashMap<std::thread::ThreadId, Registry>,
}

#[cfg(test)]
fn registries() -> &'static Mutex<Registries> {
    static REGISTRIES: OnceLock<Mutex<Registries>> = OnceLock::new();
    REGISTRIES.get_or_init(|| {
        Mutex::new(Registries {
            by_test_thread: HashMap::new(),
        })
    })
}

#[cfg(not(test))]
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub(super) fn with_registry<T>(callback: impl FnOnce(&mut Registry) -> T) -> T {
    #[cfg(test)]
    {
        let mut registries = registries().lock().expect("preview lifecycle registries");
        let registry = registries
            .by_test_thread
            .entry(std::thread::current().id())
            .or_default();
        callback(registry)
    }

    #[cfg(not(test))]
    {
        let mut registry = registry().lock().expect("preview lifecycle registry");
        callback(&mut registry)
    }
}
