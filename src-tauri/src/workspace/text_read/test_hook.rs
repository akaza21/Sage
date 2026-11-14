//! Serialize hook installation and every possible observer at the bounded-read
//! entry point, including indirect reads from other tests and worker threads.
use std::{
    fs::File,
    sync::{Arc, Barrier, Mutex, MutexGuard},
};

use sage_core::error::AppResult;

static AFTER_METADATA_BARRIER: Mutex<Option<Arc<Barrier>>> = Mutex::new(None);

pub(super) fn lock() -> MutexGuard<'static, Option<Arc<Barrier>>> {
    AFTER_METADATA_BARRIER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) struct AfterMetadataGuard {
    state: MutexGuard<'static, Option<Arc<Barrier>>>,
}

impl AfterMetadataGuard {
    pub(crate) fn install(barrier: Arc<Barrier>) -> Self {
        let mut state = lock();
        *state = Some(barrier);
        Self { state }
    }

    // Only the installing test can read with this hook. All ordinary readers
    // acquire the same mutex and wait until Drop clears it before unlocking.
    pub(crate) fn read(&self, file: &mut File) -> AppResult<Vec<u8>> {
        super::read_text_bytes_bounded_inner(file, self.state.as_deref())
    }
}

impl Drop for AfterMetadataGuard {
    fn drop(&mut self) {
        *self.state = None;
    }
}

#[test]
fn hook_is_exclusive_and_cleared_on_unwind() {
    let result = std::panic::catch_unwind(|| {
        let _hook = AfterMetadataGuard::install(Arc::new(Barrier::new(2)));
        assert!(matches!(
            AFTER_METADATA_BARRIER.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        panic!("exercise hook cleanup");
    });
    assert!(result.is_err());
    assert!(lock().is_none());
    // The poisoned lock remains usable and the next installation also clears.
    {
        let _hook = AfterMetadataGuard::install(Arc::new(Barrier::new(1)));
    }
    assert!(lock().is_none());
}
