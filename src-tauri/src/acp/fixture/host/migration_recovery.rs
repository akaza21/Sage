use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase, migration::tests::fixture_insufficient_space_observation,
};

pub(in crate::acp::fixture) struct PlatformObligation {
    pub(in crate::acp::fixture) case_id: &'static str,
    pub(in crate::acp::fixture) unavailable_native_mechanism: &'static str,
    pub(in crate::acp::fixture) windows_observer: &'static str,
    pub(in crate::acp::fixture) portable_counterparts: &'static [&'static str],
}

const WINDOWS_ONLY_MIGRATION_OBLIGATIONS: &[PlatformObligation] = &[
    PlatformObligation {
        case_id: "migration-backup-failpoint-rollback",
        unavailable_native_mechanism: "identity-bound replacement publication and held-identity deletion",
        windows_observer: "fixture_backup_failpoint_observation",
        portable_counterparts: &[
            "portable_retained_backup_survives_build_failure_then_retry_publishes",
        ],
    },
    PlatformObligation {
        case_id: "migration-retirement-prefixes",
        unavailable_native_mechanism: "identity-bound replacement publication and held-identity deletion",
        windows_observer: "fixture_retirement_observation",
        portable_counterparts: &[
            "portable_publish_recovers_retirement_restore_and_cleanup_prefixes",
            "portable_publish_restores_and_cleans_wal_shm_prefixes_in_order",
            "portable_publish_rejects_invalid_retirement_order_without_mutating_artifacts",
        ],
    },
];

#[cfg(windows)]
pub(in crate::acp::fixture) const EXPLICIT_WINDOWS_ONLY_CASE_IDS: &[&str] = &[];

#[cfg(not(windows))]
pub(in crate::acp::fixture) const EXPLICIT_WINDOWS_ONLY_CASE_IDS: &[&str] = &[
    "migration-backup-failpoint-rollback",
    "migration-retirement-prefixes",
];

pub(in crate::acp::fixture) const PLATFORM_OBLIGATIONS: &[PlatformObligation] =
    WINDOWS_ONLY_MIGRATION_OBLIGATIONS;

#[cfg(windows)]
use crate::acp::migration::tests::{
    FIXTURE_RETIREMENT_FAILPOINTS, fixture_backup_failpoint_observation,
    fixture_retirement_observation,
};

#[cfg(windows)]
pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "migration-backup-failpoint-rollback",
    "migration-retirement-prefixes",
    "migration-recovery-insufficient-space",
];

#[cfg(not(windows))]
pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &["migration-recovery-insufficient-space"];

#[cfg(windows)]
pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "migration-backup-failpoint-rollback" => backup(&steps),
        "migration-retirement-prefixes" => retirement(&steps),
        "migration-recovery-insufficient-space" => insufficient_space(&steps),
        id => panic!("unexpected migration fixture {id}"),
    }
}

#[cfg(not(windows))]
pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    assert_eq!(case.id, "migration-recovery-insufficient-space");
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    insufficient_space(&steps)
}

#[cfg(windows)]
fn backup(steps: &[Value]) -> Value {
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["op"], "runMigrationFailpoint");
    assert_eq!(steps[0]["at"], "replacement-build");
    assert_eq!(steps[1]["op"], "restart");
    let observed = fixture_backup_failpoint_observation();
    json!({
        "oldSchemaReadable": observed.old_schema_readable,
        "newSchemaVisible": observed.new_schema_visible,
        "backupState": if observed.verified_retained_backup {
            "verified-retained"
        } else {
            "invalid"
        },
        "partialRows": observed.partial_rows,
    })
}

#[cfg(windows)]
fn retirement(steps: &[Value]) -> Value {
    let [step] = steps else {
        panic!("expected one migration retirement step");
    };
    assert_eq!(step["op"], "crashRestartEach");
    let failpoints = step["failpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(failpoints, FIXTURE_RETIREMENT_FAILPOINTS);
    let observed = fixture_retirement_observation();
    json!({
        "verifiedSourceOrV5": observed.verified_source_or_v5,
        "permanentBlockerForValidPrefix": observed.permanent_blocker_for_valid_prefix,
        "partialLogicalDatabase": observed.partial_logical_database,
        "unlistedPrefix": observed.unlisted_prefix,
    })
}

fn insufficient_space(steps: &[Value]) -> Value {
    let [step] = steps else {
        panic!("expected one migration recovery step");
    };
    assert_eq!(step["op"], "runMigration");
    let free_bytes = step["freeBytes"].as_u64().unwrap();
    let observed = fixture_insufficient_space_observation(free_bytes);
    json!({
        "accepted": observed.accepted,
        "error": observed.error,
        "sqliteWrites": observed.sqlite_writes,
        "backupCreated": observed.backup_created,
    })
}
