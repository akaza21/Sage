use std::cell::Cell;

use crate::error::{AppError, AppResult};

thread_local! {
    static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    static AVAILABLE_BYTES: Cell<Option<u64>> = const { Cell::new(None) };
}

pub(super) struct AvailableBytesGuard {
    previous: Option<u64>,
}

pub(super) fn override_available_bytes(available: u64) -> AvailableBytesGuard {
    let previous = AVAILABLE_BYTES.with(|value| value.replace(Some(available)));
    AvailableBytesGuard { previous }
}

pub(super) fn available_bytes() -> Option<u64> {
    AVAILABLE_BYTES.with(Cell::get)
}

impl Drop for AvailableBytesGuard {
    fn drop(&mut self) {
        AVAILABLE_BYTES.with(|value| value.set(self.previous));
    }
}

pub(super) fn arm(id: &'static str) {
    ARMED.with(|armed| {
        assert!(armed.get().is_none(), "migration failpoint already armed");
        armed.set(Some(id));
    });
}

pub(super) fn hit(id: &'static str) -> AppResult<()> {
    ARMED.with(|armed| {
        if armed.get() == Some(id) {
            armed.set(None);
            Err(AppError::InvalidRequest(format!(
                "injected migration failpoint: {id}"
            )))
        } else {
            Ok(())
        }
    })
}

pub(super) fn is_armed() -> bool {
    ARMED.with(|armed| armed.get().is_some())
}
