//! The `custom-acp-embedded-resources` journey on the wire: it advertises
//! embedded context and answers each prompt with what Sage's own prompt
//! builder put in it.

use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::*;
use crate::acp::attached_context::files::{Resource, Resources};

const CASE: &str = "custom-acp-embedded-resources";

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn resources(files: &[(&str, &[u8])]) -> Resources {
    Resources(Arc::new(
        files
            .iter()
            .enumerate()
            .map(|(index, (name, bytes))| Resource {
                id: format!("resource-{index}"),
                name: (*name).to_owned(),
                bytes: bytes.to_vec(),
            })
            .collect(),
    ))
}

/// The requests Sage sends for one session with these two prompts.
fn session(first: &Value, second: &Value) -> Vec<Value> {
    vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{}}),
        json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":super::acp::SESSION,"prompt":first}}),
        json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"sessionId":super::acp::SESSION,"prompt":second}}),
        json!({"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":super::acp::SESSION}}),
    ]
}

fn answer(frames: &[Value], prompt_id: u64) -> String {
    let at = frames
        .iter()
        .position(|frame| frame["id"] == prompt_id)
        .unwrap_or_else(|| panic!("prompt {prompt_id} has no result"));
    assert_eq!(frames[at]["result"]["stopReason"], "end_turn");
    let chunk = &frames[at - 1]["params"]["update"];
    assert_eq!(chunk["sessionUpdate"], "agent_message_chunk");
    chunk["content"]["text"].as_str().unwrap().to_owned()
}

#[test]
fn the_case_advertises_embedded_context_and_echoes_each_resource_sage_sends() {
    let log = b"line one\nline two\n".repeat(2048);
    let blob: &[u8] = &[0, 159, 146, 150];
    let carried = resources(&[("pasted-text.txt", &log), ("capture.bin", blob)]);
    let prompt = Value::Array(carried.wire("Summarize the log", true).unwrap());
    let frames = replay(CASE, &session(&prompt, &prompt));

    assert!(crate::acp::intent::offers_prompt_capability(
        &frames[0],
        "embeddedContext"
    ));
    assert!(crate::acp::intent::offers_prompt_capability(
        &frames[0], "image"
    ));
    let expected = format!(
        "Synthetic replay: the prompt carried 2 embedded resources: pasted-text.txt (text/plain, {} bytes, SHA-256 {}); capture.bin (application/octet-stream, 4 bytes, SHA-256 {}).",
        log.len(),
        sha256(&log),
        sha256(blob)
    );
    // The send and its Retry are each answered with what they carried.
    assert_eq!(answer(&frames, 3), expected);
    assert_eq!(answer(&frames, 4), expected);
    assert!(
        !frames
            .iter()
            .any(|frame| frame.to_string().contains("line one"))
    );
}

#[test]
fn a_prompt_without_resources_is_answered_as_such() {
    let carried = resources(&[("notes.txt", b"synthetic notes")]);
    let with = Value::Array(carried.wire("first", true).unwrap());
    let without = Value::Array(Resources::default().wire("again", true).unwrap());
    let frames = replay(CASE, &session(&with, &without));
    assert_eq!(
        answer(&frames, 3),
        format!(
            "Synthetic replay: the prompt carried 1 embedded resource: notes.txt (text/plain, 15 bytes, SHA-256 {}).",
            sha256(b"synthetic notes")
        )
    );
    assert_eq!(
        answer(&frames, 4),
        "Synthetic replay: the prompt carried no embedded resource."
    );
}

#[test]
fn a_malformed_resource_block_is_refused() {
    let steps = journey(CASE);
    let opening = steps
        .iter()
        .position(|step| step["kind"] == "acpPrompt")
        .unwrap();
    for resource in [
        json!({"mimeType":"text/plain","text":"no uri"}),
        json!({"uri":"sage-attachment:///a/b.txt"}),
        json!({"uri":"sage-attachment:///a/b.txt","text":"both","blob":"Ym90aA=="}),
        json!({"uri":"sage-attachment:///a/b.bin","blob":"not base64!"}),
    ] {
        let prompt = json!([{"type":"text","text":"x"},{"type":"resource","resource":resource}]);
        let requests: String = session(&prompt, &prompt)[..3]
            .iter()
            .map(|request| format!("{request}\n"))
            .collect();
        assert_eq!(
            replay_steps(&requests, &steps[..=opening]),
            Err(FixtureError::InvalidArguments),
            "{resource}"
        );
    }
}

#[test]
fn cases_that_do_not_advertise_embedded_context_keep_their_initialize_and_echo_nothing() {
    let frames = replay_steps(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"session/new\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"session/prompt\",\"params\":{\"prompt\":[{\"type\":\"text\",\"text\":\"x\"}]}}\n",
        &journey("custom-acp-plain")[..5],
    )
    .unwrap();
    assert_eq!(
        frames[0]["result"]["agentCapabilities"]["promptCapabilities"],
        json!({"image":true})
    );
    let echo = replay_steps(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"session/prompt\",\"params\":{\"prompt\":[]}}\n",
        &[
            json!({"op":"awaitClient","kind":"acpInitialize"}),
            json!({"op":"emitKnownFrame","shape":"acpInitialize"}),
            json!({"op":"awaitClient","kind":"acpPrompt"}),
            json!({"op":"emitKnownFrame","shape":"acpResourceEcho"}),
        ],
    );
    assert_eq!(echo, Err(FixtureError::InvalidArguments));
}
