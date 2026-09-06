use super::super::lifecycle_windows;
use crate::acp::process::retained_preview_recovery_with_retry_fault_for_test;

struct RecoveryCleanup;

impl Drop for RecoveryCleanup {
    fn drop(&mut self) {
        lifecycle_windows::cleanup_retained_recovery_for_test();
    }
}

#[test]
fn retained_native_recovery_blocks_reservation_until_true_zero() {
    let _cleanup = RecoveryCleanup;
    assert!(
        lifecycle_windows::reserve_recovery_slot_for_test(),
        "the initial launch must reserve Preview capacity"
    );
    let recovery = retained_preview_recovery_with_retry_fault_for_test()
        .expect("test fixture must retain a real suspended root and Job");
    lifecycle_windows::retain_recovery_for_test(recovery)
        .expect("empty registry must retain the original recovery authority");

    assert!(lifecycle_windows::retained_recovery_for_test());
    assert!(
        !lifecycle_windows::reserve_recovery_slot_for_test(),
        "the local retry fault must retain the original root and Job"
    );
    assert!(lifecycle_windows::retained_recovery_for_test());
    assert!(
        lifecycle_windows::reserve_recovery_slot_for_test(),
        "a subsequent exact retry must terminate the retained root and prove Job zero"
    );
    assert!(!lifecycle_windows::retained_recovery_for_test());
    lifecycle_windows::release_recovery_slot_for_test();
}
