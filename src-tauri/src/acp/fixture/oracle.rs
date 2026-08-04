use serde_json::Value;

use super::generated_cases::FrozenCase;

pub(super) fn assert_literal(case: &FrozenCase, observed: Value) {
    let expected: Value = serde_json::from_str(case.oracle_json).expect("frozen oracle is JSON");
    assert_eq!(
        observed, expected,
        "fixture case {} did not satisfy its literal oracle",
        case.id
    );
}
