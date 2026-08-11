thread_local! {
    static COPY_FAILURE_AFTER: std::cell::Cell<Option<usize>> = const {
        std::cell::Cell::new(None)
    };
}

pub(super) fn inject_copy_failure_after(successful_copies: usize) {
    COPY_FAILURE_AFTER.with(|slot| slot.set(Some(successful_copies)));
}

pub(super) fn run_copy_failpoint() -> bool {
    COPY_FAILURE_AFTER.with(|slot| match slot.get() {
        Some(0) => {
            slot.set(None);
            true
        }
        Some(remaining) => {
            slot.set(Some(remaining - 1));
            false
        }
        None => false,
    })
}
