use std::cell::Cell;

use crate::error::{AppError, AppResult};

thread_local! {
    static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    static AVAILABLE_BYTES: Cell<Option<u64>> = const { Cell::new(None) };
    static RETAINED_PAIR_LOADS: Cell<Option<u64>> = const { Cell::new(None) };
    static SNAPSHOT_IO: Cell<Option<(u64, u64, u64)>> = const { Cell::new(None) };
}

pub(super) struct SnapshotIoGuard {
    previous: Option<(u64, u64, u64)>,
}

pub(super) fn observe_snapshot_io() -> SnapshotIoGuard {
    let previous = SNAPSHOT_IO.with(|value| value.replace(Some((0, 0, 0))));
    SnapshotIoGuard { previous }
}

pub(super) fn record_snapshot_copy(bytes: u64) {
    SNAPSHOT_IO.with(|value| {
        if let Some((copies, copied_bytes, hashes)) = value.get() {
            value.set(Some((copies + 1, copied_bytes + bytes, hashes)));
        }
    });
}

pub(super) fn record_main_hash() {
    SNAPSHOT_IO.with(|value| {
        if let Some((copies, bytes, hashes)) = value.get() {
            value.set(Some((copies, bytes, hashes + 1)));
        }
    });
}

impl SnapshotIoGuard {
    pub(super) fn copies(&self) -> (u64, u64) {
        SNAPSHOT_IO.with(|value| {
            let (copies, bytes, _) = value.get().unwrap();
            (copies, bytes)
        })
    }

    pub(super) fn main_hashes(&self) -> u64 {
        SNAPSHOT_IO.with(|value| value.get().unwrap().2)
    }
}

impl Drop for SnapshotIoGuard {
    fn drop(&mut self) {
        SNAPSHOT_IO.with(|value| value.set(self.previous));
    }
}

pub(super) struct RetainedPairLoadsGuard {
    previous: Option<u64>,
}

pub(super) fn observe_retained_pair_loads() -> RetainedPairLoadsGuard {
    let previous = RETAINED_PAIR_LOADS.with(|value| value.replace(Some(0)));
    RetainedPairLoadsGuard { previous }
}

pub(super) fn record_retained_pair_load() {
    RETAINED_PAIR_LOADS.with(|value| {
        if let Some(count) = value.get() {
            value.set(Some(count + 1));
        }
    });
}

impl RetainedPairLoadsGuard {
    pub(super) fn count(&self) -> u64 {
        RETAINED_PAIR_LOADS.with(|value| value.get().unwrap_or(0))
    }
}

impl Drop for RetainedPairLoadsGuard {
    fn drop(&mut self) {
        RETAINED_PAIR_LOADS.with(|value| value.set(self.previous));
    }
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
