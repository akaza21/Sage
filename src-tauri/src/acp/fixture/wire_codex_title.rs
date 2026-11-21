use super::super::FixtureError;
use super::{WireState, codex::emit_json};
use serde_json::{Value, json};
use std::io::Write;

const THREAD: &str = "fixture-ephemeral-title";
const TURN: &str = "fixture-ephemeral-title-turn";

pub(super) fn answer(
    request: &Value,
    output: &mut impl Write,
    state: &mut WireState,
) -> Result<bool, FixtureError> {
    let method = request["method"].as_str().unwrap_or_default();
    let temporary = request.pointer("/params/threadId").and_then(Value::as_str) == Some(THREAD);
    let result = match method {
        "config/read" => json!({"config":{"mcp_servers":{"fixture_external":{"enabled":true}}}}),
        "thread/start" if request.pointer("/params/ephemeral") == Some(&json!(true)) => {
            let params = &request["params"];
            if params["sandbox"] != "read-only"
                || params["dynamicTools"] != json!([])
                || params["environments"] != json!([])
                || params["config"]["features.shell_tool"] != false
                || params["config"]["mcp_servers"]["fixture_external"]["enabled"] != false
            {
                return Err(FixtureError::InvalidArguments);
            }
            emit_json(
                output,
                state,
                &json!({"method":"thread/started","params":{"thread":{"id":THREAD,"ephemeral":true}}}),
            )?;
            json!({"thread":{"id":THREAD,"ephemeral":true},"sandbox":{"type":"readOnly"}})
        }
        "turn/start" if temporary => {
            emit_json(
                output,
                state,
                &json!({"id":request["id"],"result":{"turn":{"id":TURN}}}),
            )?;
            for event in [
                json!({"method":"turn/started","params":{"threadId":THREAD,"turn":{"id":TURN}}}),
                json!({"method":"item/started","params":{"threadId":THREAD,"turnId":TURN,"item":{"id":"title-item","type":"agentMessage","text":""}}}),
                json!({"method":"item/completed","params":{"threadId":THREAD,"turnId":TURN,"item":{"id":"title-item","type":"agentMessage","text":"{\"title\":\"Investigate empty sidebar chats\"}"}}}),
                json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":"completed"}}}),
            ] {
                emit_json(output, state, &event)?;
            }
            return Ok(true);
        }
        "turn/interrupt" if temporary => json!({}),
        "thread/unsubscribe" if temporary => {
            emit_json(output, state, &json!({"id":request["id"],"result":{}}))?;
            emit_json(
                output,
                state,
                &json!({"method":"thread/closed","params":{"threadId":THREAD}}),
            )?;
            return Ok(true);
        }
        _ => return Ok(false),
    };
    emit_json(output, state, &json!({"id":request["id"],"result":result}))?;
    Ok(true)
}

#[cfg(test)]
pub(crate) fn title_wire(request: &Value) -> Vec<Value> {
    let mut output = Vec::new();
    assert!(answer(request, &mut output, &mut WireState::new()).unwrap());
    serde_json::Deserializer::from_slice(&output)
        .into_iter::<Value>()
        .map(Result::unwrap)
        .collect()
}
