use std::collections::BTreeSet;
use std::ffi::OsString;

use serde_json::Value;

use crate::acp::limits::LIMITS;

use super::cases::FixtureInvocation;
use super::generated_cases::FROZEN_CASES;
use super::interpreter::classify;
use super::*;

#[test]
fn fixture_accepts_only_exact_frozen_arguments() {
    assert_eq!(
        run(["--case", "unknown", "--seed", "0"].map(OsString::from)),
        Err(FixtureError::InvalidArguments)
    );
    assert_eq!(
        run(["--case", "frame-invalid-utf8", "--seed", "invalid",].map(OsString::from)),
        Err(FixtureError::InvalidArguments)
    );
    assert!(
        FixtureInvocation::parse(
            ["--case", "frame-invalid-utf8", "--seed", "7"].map(OsString::from)
        )
        .is_ok()
    );
    for case in [
        "characterization-two-turn-success",
        "characterization-cancel-reuse",
        "codex-journey-turn-success",
    ] {
        let invocation =
            FixtureInvocation::parse(["--case", case, "--seed", "0"].map(OsString::from)).unwrap();
        let steps: Vec<Value> = serde_json::from_str(invocation.steps_json).unwrap();
        assert!(
            steps
                .iter()
                .any(|step| { step.get("kind") == Some(&Value::String("codexInitialize".into())) })
        );
        assert!(steps.iter().any(|step| {
            step.get("shape") == Some(&Value::String("codexInitializeResult".into()))
        }));
        assert!(!invocation.steps_json.contains("fixture/"));
    }
}

#[test]
fn all_179_frozen_case_ids_and_steps_are_unique_bounded_and_recognized() {
    assert_eq!(FROZEN_CASES.len(), 179);
    let mut ids = BTreeSet::new();
    for fixture_case in FROZEN_CASES {
        assert!(
            ids.insert(fixture_case.id),
            "duplicate fixture ID {}",
            fixture_case.id
        );
        assert!(!fixture_case.id.is_empty());
        assert!(fixture_case.steps_json.len() as u64 <= LIMITS.fixture.instruction_bytes_per_case);
        let steps: Vec<Value> = serde_json::from_str(fixture_case.steps_json).unwrap();
        assert!(steps.len() as u64 <= LIMITS.fixture.steps_per_case);
        for step in steps {
            let op = step.get("op").and_then(Value::as_str).unwrap();
            classify(op).unwrap_or_else(|_| panic!("unrecognized fixture operation {op}"));
        }
        let oracle: Value = serde_json::from_str(fixture_case.oracle_json).unwrap();
        assert!(oracle.as_object().is_some_and(|fields| !fields.is_empty()));
    }
}

#[test]
fn frozen_execution_ownership_and_phase_counts_are_exact() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    let mut child = 0;
    let mut host = 0;
    let mut foundation = 0;
    let mut windows = 0;
    let mut package = 0;
    let mut deferred = 0;
    for fixture_case in FROZEN_CASES {
        match fixture_case.owner {
            FrozenOwner::Child => child += 1,
            FrozenOwner::Host => host += 1,
        }
        match fixture_case.phase {
            FrozenPhase::Foundation => foundation += 1,
            FrozenPhase::Windows => windows += 1,
            FrozenPhase::Package => package += 1,
            FrozenPhase::Deferred => deferred += 1,
        }
    }
    assert_eq!((child, host), (70, 109));
    assert_eq!((foundation, windows, package, deferred), (167, 5, 1, 6));
}

#[test]
fn generated_table_records_the_exact_frozen_source_hash() {
    use sha2::{Digest, Sha256};

    let source = include_bytes!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/fixture-vectors-v1.json"
    );
    let digest = Sha256::digest(source)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert!(include_str!("generated_cases.rs").contains(&digest));
}

#[test]
fn child_runner_rejects_host_owned_case_before_execution() {
    assert_eq!(
        run(["--case", "runtime-epoch-item-mismatch", "--seed", "0",].map(OsString::from)),
        Err(FixtureError::HostOracleRequired)
    );
}

#[test]
fn frozen_schema_cases_execute_against_the_production_migration() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute(fixture_case));
    }
}

#[test]
fn frozen_child_framing_and_structural_cases_execute_against_production_ingress() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::WIRE_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_wire(fixture_case));
    }
}

#[test]
fn frozen_child_semantic_limits_execute_against_shared_production_ingress() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::SEMANTIC_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_semantic(fixture_case));
    }
}

#[test]
fn frozen_child_unknown_events_fail_closed_without_capability_escalation() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::UNKNOWN_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_unknown(fixture_case));
    }
}

#[test]
fn frozen_child_stderr_limits_execute_against_the_bounded_native_ring() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::STDERR_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_stderr(fixture_case));
    }
}

#[test]
fn frozen_child_item_terminals_are_idempotent_and_late_safe() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::ITEM_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_item(fixture_case));
    }
}

#[test]
fn frozen_child_permissions_decline_durably_and_fail_closed() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::PERMISSION_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_permission(fixture_case));
    }
}

#[test]
fn frozen_stale_project_completion_is_durable_but_projection_suppressed() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::STALE_PROJECTION_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_stale_projection(fixture_case));
    }
}

#[test]
fn frozen_native_child_activity_uses_one_session_tree_and_bounded_projection() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in child::ACTIVITY_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Child);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, child::execute_activity(fixture_case));
    }
}

#[test]
fn frozen_scoped_capacity_cases_execute_against_the_shared_lease_authority() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::CAPACITY_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_capacity(fixture_case));
    }
}

#[test]
fn frozen_queue_cases_execute_against_production_capacity_authorities() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::QUEUE_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_queue(fixture_case));
    }
}

#[test]
fn frozen_process_launch_metadata_bounds_are_cross_platform_and_pre_spawn() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::PROCESS_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_process(fixture_case));
    }
}

#[test]
fn frozen_item_activity_and_collaboration_bounds_use_production_observers() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::STORE_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_store(fixture_case));
    }
}

#[test]
fn frozen_cursor_prompt_and_normalized_payload_bounds_use_shared_contracts() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::CONTRACT_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_contract(fixture_case));
    }
}

#[test]
fn frozen_capability_and_provenance_integrity_rejects_before_persistence() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::METADATA_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_metadata(fixture_case));
    }
}

#[test]
fn frozen_permission_failures_use_the_mandatory_deny_and_teardown_contract() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::PERMISSION_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_permission(fixture_case));
    }
}

#[cfg(windows)]
#[test]
fn frozen_migration_cases_execute_against_production_recovery() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::MIGRATION_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_migration(fixture_case));
    }
}

#[cfg(not(windows))]
#[test]
fn frozen_migration_cases_execute_against_production_recovery() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    let fixture_case = FROZEN_CASES
        .iter()
        .find(|candidate| candidate.id == "migration-recovery-insufficient-space")
        .unwrap();
    assert!(fixture_case.owner == FrozenOwner::Host);
    assert!(fixture_case.phase == FrozenPhase::Foundation);
    oracle::assert_literal(fixture_case, host::execute_migration(fixture_case));
}

#[test]
fn frozen_native_and_relative_path_bounds_fail_before_filesystem_or_webview_ingress() {
    use super::generated_cases::{FrozenOwner, FrozenPhase};

    for case_id in host::RESOURCE_CASE_IDS {
        let fixture_case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == *case_id)
            .unwrap();
        assert!(fixture_case.owner == FrozenOwner::Host);
        assert!(fixture_case.phase == FrozenPhase::Foundation);
        oracle::assert_literal(fixture_case, host::execute_resource(fixture_case));
    }
}

#[test]
fn deferred_native_control_case_cannot_be_claimed_by_the_foundation_fixture() {
    assert_eq!(
        run(["--case", "native-fork-correlated-success", "--seed", "0",].map(OsString::from)),
        Err(FixtureError::DeferredCase)
    );
}

#[test]
fn fixture_adapter_requires_written_prompt_and_emits_one_bounded_text_lifecycle() {
    use crate::acp::{
        bindings::adapter::{
            AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
        },
        contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
        fixture::adapter::FixtureAdapter,
        intent::{ExternalId, Prompt, SafeResourceRef, UnavailableResourceReason},
        lifecycle::{ItemLifecycle, TurnState},
        transport::IncrementalFrameDecoder,
    };

    struct Mapper;
    impl ResourceMapper for Mapper {
        fn map_existing(
            &self,
            _absolute_path: &str,
        ) -> Result<SafeResourceRef, UnavailableResourceReason> {
            Err(UnavailableResourceReason::Unsupported)
        }
    }

    fn frame(shape: &str) -> crate::acp::transport::Frame {
        let bytes =
            format!("{{\"jsonrpc\":\"2.0\",\"method\":\"fixture/{shape}\",\"params\":{{}}}}\n");
        IncrementalFrameDecoder::new()
            .push_chunk(bytes.as_bytes())
            .unwrap()
            .unwrap()
    }

    let mut adapter = FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("fixture-session").unwrap(),
        ExternalId::new("externalSession", "fixture-external-session").unwrap(),
    )
    .unwrap();
    let egress = adapter
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "read only").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
        })
        .unwrap();
    assert!(matches!(
        adapter.ingest(&frame("textStart"), &Mapper),
        Err(crate::acp::bindings::adapter::AdapterRuntimeError::Operation)
    ));
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();

    let start = adapter.ingest(&frame("textStart"), &Mapper).unwrap();
    assert!(matches!(
        start.events(),
        [
            AdapterEvent::Turn {
                state: TurnState::Running,
                ..
            },
            AdapterEvent::Item {
                lifecycle: ItemLifecycle::Started,
                ..
            }
        ]
    ));
    let delta = adapter.ingest(&frame("textDelta"), &Mapper).unwrap();
    assert!(matches!(
        delta.events(),
        [AdapterEvent::Item {
            lifecycle: ItemLifecycle::Streaming,
            ..
        }]
    ));
    let item = adapter.ingest(&frame("textComplete"), &Mapper).unwrap();
    assert!(matches!(
        item.events(),
        [AdapterEvent::Item {
            lifecycle: ItemLifecycle::Completed,
            ..
        }]
    ));
    let turn = adapter.ingest(&frame("turnComplete"), &Mapper).unwrap();
    assert!(matches!(
        turn.events(),
        [AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        }]
    ));
}
