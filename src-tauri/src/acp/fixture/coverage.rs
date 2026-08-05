use std::collections::BTreeSet;

use super::{
    child,
    generated_cases::{FROZEN_CASES, FrozenOwner, FrozenPhase},
    host,
};

const NODE_POLICY_CASES: &[&str] = &["csp-production-and-development"];
const PATH_BOUND_CHILD_CASES: &[&str] = &[
    "turn-text-success",
    "send-complete-uncorrelated-exit",
    "send-correlated-then-exit",
];
const OPEN_HOST_CASES: &[&str] = &[
    "send-partial-write-failure",
    "cancel-dispatch-at-capacity",
    "redaction-canaries-all-sinks",
];
const OPEN_CHILD_CASES: &[&str] = &[
    "permission-identical-live-duplicate",
    "send-zero-byte-failure",
    "resume-success-once",
    "resume-failure-no-second-attempt",
    "native-activity-child-text",
    "native-activity-foreign-parent",
];

#[test]
fn foundation_literal_observer_registry_is_exact_and_nonoverlapping() {
    let host_families: &[&[&str]] = &[
        host::CASE_IDS,
        host::CAPACITY_CASE_IDS,
        host::QUEUE_CASE_IDS,
        host::PROCESS_CASE_IDS,
        host::STORE_CASE_IDS,
        host::CONTRACT_CASE_IDS,
        host::METADATA_CASE_IDS,
        host::PERMISSION_CASE_IDS,
        host::MIGRATION_CASE_IDS,
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
    assert_eq!(host.len(), 102, "registered host literal observers changed");
    assert_eq!(
        child.len(),
        53,
        "registered child literal observers changed"
    );

    let expected_host = FROZEN_CASES
        .iter()
        .filter(|case| case.owner == FrozenOwner::Host && case.phase == FrozenPhase::Foundation)
        .map(|case| case.id)
        .collect::<BTreeSet<_>>();
    assert!(host.is_subset(&expected_host));
    let unregistered_host = expected_host
        .difference(&host)
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unregistered_host,
        OPEN_HOST_CASES.iter().copied().collect::<BTreeSet<_>>(),
        "update the host observer registry and open coverage ledger together"
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
