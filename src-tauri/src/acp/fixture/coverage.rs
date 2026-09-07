use std::collections::BTreeSet;

use super::{
    child,
    generated_cases::{FROZEN_CASES, FrozenOwner, FrozenPhase},
    host,
};

const NODE_POLICY_CASES: &[&str] = &["csp-production-and-development"];
const PATH_BOUND_CHILD_CASES: &[&str] = &[
    "turn-text-success",
    "send-zero-byte-failure",
    "send-complete-uncorrelated-exit",
    "send-correlated-then-exit",
    "resume-success-once",
    "resume-failure-no-second-attempt",
    "native-activity-child-text",
    "native-activity-foreign-parent",
];
const OPEN_CHILD_CASES: &[&str] = &[];

#[test]
fn foundation_literal_observer_registry_is_exact_and_nonoverlapping() {
    let host_families: &[&[&str]] = &[
        host::CASE_IDS,
        host::CAPACITY_CASE_IDS,
        host::CANCEL_CASE_IDS,
        host::DELIVERY_CASE_IDS,
        host::QUEUE_CASE_IDS,
        host::PROCESS_CASE_IDS,
        host::STORE_CASE_IDS,
        host::CONTRACT_CASE_IDS,
        host::METADATA_CASE_IDS,
        host::PERMISSION_CASE_IDS,
        host::MIGRATION_CASE_IDS,
        host::REDACTION_CASE_IDS,
        host::RESOURCE_CASE_IDS,
        NODE_POLICY_CASES,
    ];
    let child_families: &[&[&str]] = &[
        child::ACTIVITY_CASE_IDS,
        child::ITEM_CASE_IDS,
        child::PERMISSION_CASE_IDS,
        child::WIRE_CASE_IDS,
        child::SEMANTIC_CASE_IDS,
        child::STALE_PROJECTION_CASE_IDS,
        child::STDERR_CASE_IDS,
        child::UNKNOWN_CASE_IDS,
    ];

    let host = collect_unique(host_families, FrozenOwner::Host);
    let child = collect_unique(child_families, FrozenOwner::Child);
    assert!(host.is_disjoint(&child));
    assert_eq!(
        child.len(),
        54,
        "registered child literal observers changed"
    );

    let expected_host = FROZEN_CASES
        .iter()
        .filter(|case| case.owner == FrozenOwner::Host && case.phase == FrozenPhase::Foundation)
        .map(|case| case.id)
        .collect::<BTreeSet<_>>();
    assert_platform_host_obligations(
        &host,
        &expected_host,
        host::EXPLICIT_WINDOWS_ONLY_CASE_IDS,
        host::MIGRATION_PLATFORM_OBLIGATIONS,
    );

    let expected_child = FROZEN_CASES
        .iter()
        .filter(|case| case.owner == FrozenOwner::Child && case.phase == FrozenPhase::Foundation)
        .map(|case| case.id)
        .collect::<BTreeSet<_>>();
    assert!(child.is_subset(&expected_child));
    let unregistered = expected_child
        .difference(&child)
        .copied()
        .collect::<BTreeSet<_>>();
    let declared_unregistered = PATH_BOUND_CHILD_CASES
        .iter()
        .chain(OPEN_CHILD_CASES)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unregistered, declared_unregistered,
        "update the child observer registry and frozen coverage accounting together"
    );
}

fn assert_platform_host_obligations(
    registered_host: &BTreeSet<&'static str>,
    expected_host: &BTreeSet<&'static str>,
    explicit_windows_only_case_ids: &[&'static str],
    migration_platform_obligations: &[host::PlatformObligation],
) {
    let explicit_windows_only = explicit_windows_only_case_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        explicit_windows_only.len(),
        explicit_windows_only_case_ids.len(),
        "duplicate explicit Windows-only Foundation exclusion"
    );
    let obligation_ids = migration_platform_obligations
        .iter()
        .map(|obligation| {
            assert!(
                !obligation.unavailable_native_mechanism.is_empty()
                    && !obligation.windows_observer.is_empty()
                    && !obligation.portable_counterparts.is_empty(),
                "platform obligation must name its unavailable mechanism, Windows observer, and portable counterpart"
            );
            let portable_counterparts = obligation
                .portable_counterparts
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            assert_eq!(
                portable_counterparts.len(),
                obligation.portable_counterparts.len(),
                "duplicate portable counterpart for {}",
                obligation.case_id
            );
            obligation.case_id
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        obligation_ids.len(),
        migration_platform_obligations.len(),
        "duplicate platform obligation row"
    );
    #[cfg(not(windows))]
    assert_eq!(
        obligation_ids, explicit_windows_only,
        "every explicit Windows-only Foundation case needs one named platform obligation"
    );
    assert!(
        registered_host.is_disjoint(&explicit_windows_only),
        "registered and explicit Windows-only Foundation cases must be disjoint"
    );
    let accounted_host = registered_host
        .union(&explicit_windows_only)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        accounted_host, *expected_host,
        "registered host observers plus explicit Windows-only obligations must exactly cover the frozen Foundation Host set"
    );
    #[cfg(windows)]
    {
        assert!(
            explicit_windows_only.is_empty(),
            "Windows must execute every frozen Foundation Host observer"
        );
        assert_eq!(
            obligation_ids,
            [
                "migration-backup-failpoint-rollback",
                "migration-retirement-prefixes",
            ]
            .into_iter()
            .collect(),
            "Windows retains the named portable counterparts while executing the frozen literal observers"
        );
    }
}

#[test]
fn duplicate_platform_obligation_rows_are_rejected() {
    let obligations = [
        host::PlatformObligation {
            case_id: "migration-backup-failpoint-rollback",
            unavailable_native_mechanism: "identity-bound replacement publication",
            windows_observer: "fixture_backup_failpoint_observation",
            portable_counterparts: &[
                "portable_retained_backup_survives_build_failure_then_retry_publishes",
            ],
        },
        host::PlatformObligation {
            case_id: "migration-backup-failpoint-rollback",
            unavailable_native_mechanism: "identity-bound replacement publication",
            windows_observer: "fixture_backup_failpoint_observation",
            portable_counterparts: &[
                "portable_retained_backup_survives_build_failure_then_retry_publishes",
            ],
        },
    ];
    let expected = ["migration-backup-failpoint-rollback"]
        .into_iter()
        .collect();
    let registered = BTreeSet::new();
    assert!(
        std::panic::catch_unwind(|| {
            assert_platform_host_obligations(
                &registered,
                &expected,
                &["migration-backup-failpoint-rollback"],
                &obligations,
            );
        })
        .is_err()
    );
}

#[test]
fn duplicate_explicit_windows_only_exclusions_are_rejected() {
    let expected = ["migration-backup-failpoint-rollback"]
        .into_iter()
        .collect();
    let registered = BTreeSet::new();
    assert!(
        std::panic::catch_unwind(|| {
            assert_platform_host_obligations(
                &registered,
                &expected,
                &[
                    "migration-backup-failpoint-rollback",
                    "migration-backup-failpoint-rollback",
                ],
                &[],
            );
        })
        .is_err()
    );
}

fn collect_unique(
    families: &[&[&'static str]],
    expected_owner: FrozenOwner,
) -> BTreeSet<&'static str> {
    let mut observed = BTreeSet::new();
    for family in families {
        for case_id in *family {
            assert!(
                observed.insert(*case_id),
                "duplicate observer for {case_id}"
            );
            let case = FROZEN_CASES
                .iter()
                .find(|candidate| candidate.id == *case_id)
                .unwrap_or_else(|| panic!("unknown observed case {case_id}"));
            assert!(case.owner == expected_owner, "wrong owner for {case_id}");
            assert!(
                case.phase == FrozenPhase::Foundation,
                "non-Foundation case registered by the headless observer: {case_id}"
            );
        }
    }
    observed
}
