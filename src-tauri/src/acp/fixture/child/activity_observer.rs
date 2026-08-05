use serde_json::Value;

use crate::acp::fixture::{generated_cases::FrozenCase, host, wire};

pub(crate) const CASE_IDS: &[&str] = &["native-activity-depth-4", "native-activity-depth-5"];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    assert!(CASE_IDS.contains(&case.id));
    observe_depth(case)
}

fn observe_depth(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let frames = super::item_observer::decode(&stdout);
    let [frame] = frames.as_slice() else {
        panic!("expected one child activity frame for {}", case.id);
    };
    let envelope: Value = serde_json::from_slice(frame.as_bytes()).unwrap();
    let depth = match envelope["method"].as_str() {
        Some("fixture/childActivityDepth4") => 4,
        Some("fixture/childActivityDepth5") => 5,
        _ => panic!("unrecognized child activity frames for {}", case.id),
    };
    host::execute_child_activity(depth)
}
