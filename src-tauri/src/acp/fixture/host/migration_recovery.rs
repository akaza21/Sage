use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    migration::tests::{
        FIXTURE_RETIREMENT_FAILPOINTS, fixture_backup_failpoint_observation,
        fixture_insufficient_space_observation, fixture_retirement_observation,
    },
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "migration-backup-failpoint-rollback",
    "migration-retirement-prefixes",
    "migration-recovery-insufficient-space",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "migration-backup-failpoint-rollback" => backup(&steps),
        "migration-retirement-prefixes" => retirement(&steps),
        "migration-recovery-insufficient-space" => insufficient_space(&steps),
        id => panic!("unexpected migration fixture {id}"),
    }
}

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
