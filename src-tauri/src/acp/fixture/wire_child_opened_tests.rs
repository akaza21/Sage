#![cfg(test)]

use serde_json::Value;

use super::{WireState, execute_with_io};

pub(crate) fn child_opened_wire_for_test(read_id: u64) -> Vec<Value> {
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-child-opened")
        .unwrap();
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let requests = [
        serde_json::json!({"id":1,"method":"initialize"}),
        serde_json::json!({"id":2,"method":"thread/start"}),
        serde_json::json!({"id":3,"method":"thread/goal/get"}),
        serde_json::json!({"id":4,"method":"turn/start"}),
        serde_json::json!({"id":read_id,"method":"thread/read","params":{"threadId":"fixture-child-1","includeTurns":true}}),
        serde_json::json!({"id":99,"method":"thread/unsubscribe"}),
    ].iter().map(|request| format!("{request}\n")).collect::<String>();
    let mut input = std::io::Cursor::new(requests);
    let mut state = WireState::new();
    let mut output = Vec::new();
    for step in steps {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
    }
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
