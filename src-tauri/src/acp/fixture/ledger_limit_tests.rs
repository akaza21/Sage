use std::{fs, path::PathBuf};

use serde_json::Value;

use sage_acp_protocol::fixture_protocol::validate_limit_fixture_coverage;

use super::generated_cases::{FROZEN_CASES, FrozenCase, FrozenPhase};
use super::{child, host, oracle};

fn contract(name: &str) -> String {
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a checkout parent")
        .to_path_buf();
    let path = checkout
        .join("docs/work-items/ADE-0006A-acp-session-foundation/contracts")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("contract artifact {}: {error}", path.display()))
}

fn frozen_case(id: &str) -> &'static FrozenCase {
    FROZEN_CASES
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("ledger names unknown fixture case {id}"))
}

fn observe(case: &FrozenCase) -> Value {
    type Family = (&'static [&'static str], fn(&FrozenCase) -> Value);
    let families: [Family; 11] = [
        (child::WIRE_CASE_IDS, child::execute_wire),
        (child::PERMISSION_CASE_IDS, child::execute_permission),
        (child::SEMANTIC_CASE_IDS, child::execute_semantic),
        (child::STDERR_CASE_IDS, child::execute_stderr),
        (child::ACTIVITY_CASE_IDS, child::execute_activity),
        (host::CAPACITY_CASE_IDS, host::execute_capacity),
        (host::QUEUE_CASE_IDS, host::execute_queue),
        (host::PROCESS_CASE_IDS, host::execute_process),
        (host::STORE_CASE_IDS, host::execute_store),
        (host::CONTRACT_CASE_IDS, host::execute_contract),
        (host::RESOURCE_CASE_IDS, host::execute_resource),
    ];
    let (_, execute) = families
        .iter()
        .find(|(ids, _)| ids.contains(&case.id))
        .unwrap_or_else(|| panic!("no production observer runs fixture case {}", case.id));
    execute(case)
}

#[test]
fn every_ledger_limit_admits_its_exact_input_and_rejects_limit_plus_one_in_production() {
    let limits = contract("limits-v1.json");
    let coverage = contract("limit-fixture-coverage-v1.json");
    validate_limit_fixture_coverage(&limits, &coverage, &contract("fixture-vectors-v1.json"))
        .expect("each named fixture carries its ledger limit and limit plus one");

    let coverage: Value = serde_json::from_str(&coverage).unwrap();
    let pairs = coverage["pairs"].as_array().expect("coverage pairs");
    let mut executed = 0;
    let mut platform_bound = Vec::new();
    for pair in pairs {
        let [path, exact_id, over_id] = [0, 1, 2].map(|index| pair[index].as_str().unwrap());
        let (exact, over) = (frozen_case(exact_id), frozen_case(over_id));
        if exact.phase != FrozenPhase::Foundation || over.phase != FrozenPhase::Foundation {
            platform_bound.push(path);
            continue;
        }
        let admitted = observe(exact);
        let rejected = observe(over);
        oracle::assert_literal(exact, admitted.clone());
        oracle::assert_literal(over, rejected.clone());
        assert_ne!(
            admitted, rejected,
            "{path}: limit plus one ({over_id}) must change the production outcome of the exact limit ({exact_id})"
        );
        executed += 1;
    }

    assert_eq!(
        platform_bound,
        [
            "process.descendantsPerTree",
            "process.windowsJobActiveProcessLimit",
            "process.terminalTailBytes",
        ]
    );
    assert_eq!(executed, pairs.len() - platform_bound.len());
}
