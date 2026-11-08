//! Codex `item/tool/requestUserInput` for native replay journeys: the
//! fixture asks the step's questions, waits for the client's answer, and
//! completes the turn by echoing the option labels it received. Typed
//! answers (Codex `user_note:` values, which carry secrets) are only
//! counted, never echoed.

use std::io::{Read, Write};

use serde_json::{Value, json};

use super::super::FixtureError;
use super::{WireState, acp, codex};

/// The id of the fixture's first server request; later ones count up.
const FIRST_REQUEST_ID: u64 = 900;
const USER_NOTE: &str = "user_note:";

pub(super) fn emit_request(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let turn = state
        .current_turn_id
        .clone()
        .ok_or(FixtureError::InvalidArguments)?;
    let questions = step
        .get("questions")
        .filter(|questions| questions.is_array())
        .ok_or(FixtureError::InvalidArguments)?;
    let auto_resolution = step.get("autoResolutionMs").cloned().unwrap_or(Value::Null);
    if state.pending_user_input.is_some() {
        return Err(FixtureError::InvalidArguments);
    }
    let request_id = FIRST_REQUEST_ID
        .checked_add(state.server_requests_sent)
        .ok_or(FixtureError::ExpandedLimit)?;
    state.server_requests_sent += 1;
    state.pending_user_input = Some(request_id);
    codex::emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","id":request_id,"method":"item/tool/requestUserInput","params":{
            "threadId":acp::SESSION,"turnId":turn,"itemId":format!("fixture-user-input-{turn}"),
            "questions":questions,"autoResolutionMs":auto_resolution
        }}),
    )
}

pub(super) fn await_answer(
    input: &mut impl Read,
    state: &mut WireState,
) -> Result<(), FixtureError> {
    for _ in 0..32 {
        let frame = codex::read_json_line(input, state)?.ok_or(FixtureError::Io)?;
        if frame.get("id").is_none() {
            continue;
        }
        let pending = state
            .pending_user_input
            .take()
            .ok_or(FixtureError::InvalidArguments)?;
        if frame.get("id") != Some(&json!(pending)) || frame.get("method").is_some() {
            return Err(FixtureError::InvalidArguments);
        }
        let answers = frame
            .pointer("/result/answers")
            .and_then(Value::as_object)
            .ok_or(FixtureError::InvalidArguments)?;
        state.user_input_echo = Some(echo(answers)?);
        return Ok(());
    }
    Err(FixtureError::ExpandedLimit)
}

pub(super) fn emit_echo(
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let text = state
        .user_input_echo
        .take()
        .ok_or(FixtureError::InvalidArguments)?;
    let turn = state
        .current_turn_id
        .clone()
        .ok_or(FixtureError::InvalidArguments)?;
    codex::emit_json(
        output,
        state,
        &json!({"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{
            "threadId":acp::SESSION,"turnId":turn,"itemId":format!("fixture-user-input-echo-{turn}"),
            "delta":text
        }}),
    )
}

fn echo(answers: &serde_json::Map<String, Value>) -> Result<String, FixtureError> {
    if answers.is_empty() {
        return Ok("Synthetic replay: the questions were declined.".to_owned());
    }
    let mut labels = Vec::new();
    let mut typed = 0_usize;
    for answer in answers.values() {
        for value in answer
            .get("answers")
            .and_then(Value::as_array)
            .ok_or(FixtureError::InvalidArguments)?
        {
            let value = value.as_str().ok_or(FixtureError::InvalidArguments)?;
            if value.starts_with(USER_NOTE) {
                typed += 1;
            } else {
                labels.push(value);
            }
        }
    }
    Ok(format!(
        "Synthetic replay: received option {}; {typed} typed answer(s) not echoed.",
        if labels.is_empty() {
            "none".to_owned()
        } else {
            labels.join(", ")
        }
    ))
}

/// Every frame the `codex-user-input` journey sends, in order, when a
/// client answers each question with `answer`.
#[cfg(test)]
pub(crate) fn codex_user_input_wire_for_test(answer: &Value) -> Vec<Value> {
    let case = super::super::generated_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-user-input")
        .expect("the codex-user-input journey exists");
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let requests = [
        json!({"id":1,"method":"initialize"}),
        json!({"id":2,"method":"thread/start"}),
        json!({"id":3,"method":"thread/goal/get"}),
        json!({"id":4,"method":"turn/start"}),
        json!({"id":FIRST_REQUEST_ID,"result":answer}),
        json!({"id":5,"method":"turn/start"}),
        json!({"id":FIRST_REQUEST_ID + 1,"result":answer}),
        json!({"id":6,"method":"thread/unsubscribe"}),
    ]
    .iter()
    .map(|request| format!("{request}\n"))
    .collect::<String>();
    let mut input = std::io::Cursor::new(requests);
    let mut state = WireState::new();
    let mut output = Vec::new();
    for step in steps {
        super::execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new())
            .unwrap();
    }
    output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::execute_with_io;
    use super::*;
    use std::io::Cursor;

    fn exchange(answer: &str) -> (Value, Value) {
        let mut state = WireState::new();
        state.current_turn_id = Some("turn-1".to_owned());
        let mut input = Cursor::new(format!(
            "{}\n{answer}\n",
            r#"{"jsonrpc":"2.0","method":"initialized"}"#
        ));
        let mut output = Vec::new();
        for step in [
            json!({"op":"emitKnownFrame","shape":"codexUserInputRequest","autoResolutionMs":5000,
                "questions":[{"id":"scope","header":"Scope","question":"Which?","isOther":false,
                "isSecret":false,"options":[{"label":"Changed files","description":"d"}]}]}),
            json!({"op":"awaitClient","kind":"codexUserInputAnswer"}),
            json!({"op":"emitKnownFrame","shape":"codexUserInputEcho"}),
        ] {
            execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new()).unwrap();
        }
        let mut frames = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Value>(line).unwrap());
        (frames.next().unwrap(), frames.next().unwrap())
    }

    #[test]
    fn an_answer_echoes_the_option_label_and_never_the_typed_secret() {
        let (request, echo) = exchange(
            r#"{"id":900,"result":{"answers":{"scope":{"answers":["Changed files"]},"token":{"answers":["user_note: hunter2"]}}}}"#,
        );
        assert_eq!(request["method"], "item/tool/requestUserInput");
        assert_eq!(request["params"]["turnId"], "turn-1");
        assert_eq!(request["params"]["autoResolutionMs"], 5000);
        let text = echo["params"]["delta"].as_str().unwrap();
        assert_eq!(
            text,
            "Synthetic replay: received option Changed files; 1 typed answer(s) not echoed."
        );
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn empty_answers_echo_a_decline() {
        let (_, echo) = exchange(r#"{"id":900,"result":{"answers":{}}}"#);
        assert_eq!(
            echo["params"]["delta"],
            "Synthetic replay: the questions were declined."
        );
    }

    #[test]
    fn a_reply_to_another_request_is_rejected() {
        let mut state = WireState::new();
        state.pending_user_input = Some(900);
        let mut input = Cursor::new("{\"id\":901,\"result\":{\"answers\":{}}}\n");
        assert_eq!(
            await_answer(&mut input, &mut state),
            Err(FixtureError::InvalidArguments)
        );
    }
    #[test]
    fn each_question_gets_its_own_request_id_like_codex() {
        let mut state = WireState::new();
        let mut input = Cursor::new(
            "{\"id\":900,\"result\":{\"answers\":{}}}\n{\"id\":901,\"result\":{\"answers\":{}}}\n",
        );
        let mut output = Vec::new();
        for turn in ["turn-1", "turn-2"] {
            state.current_turn_id = Some(turn.to_owned());
            for step in [
                json!({"op":"emitKnownFrame","shape":"codexUserInputRequest","questions":[]}),
                json!({"op":"awaitClient","kind":"codexUserInputAnswer"}),
                json!({"op":"emitKnownFrame","shape":"codexUserInputEcho"}),
            ] {
                execute_with_io(&step, &mut state, &mut input, &mut output, &mut Vec::new())
                    .unwrap();
            }
        }
        let ids = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Value>(line).unwrap())
            .filter(|frame| frame["method"] == "item/tool/requestUserInput")
            .map(|frame| frame["id"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, [900, 901]);
    }
}
