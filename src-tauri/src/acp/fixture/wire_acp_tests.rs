use super::*;
use serde_json::json;
use std::io::Cursor;

#[path = "wire_native_projection_tests.rs"]
mod native_projection;

#[test]
fn replay_resource_paths_resolve_existing_files_in_the_capture_project() {
    let root = std::env::temp_dir().join(format!("sage-replay-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/example.ts"), b"export const example = 1;\n").unwrap();
    let canonical = root.canonicalize().unwrap();
    let value = super::acp::substitute_root("$ROOT/src/example.ts", &canonical).unwrap();
    assert_eq!(
        std::path::Path::new(&value).canonicalize().unwrap(),
        root.join("src/example.ts").canonicalize().unwrap()
    );
    #[cfg(windows)]
    assert!(!value.contains('/'));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn acp_replay_preserves_prompt_id_across_config_and_cancel() {
    let mut state = WireState::new();
    let mut output = Vec::new();
    let mut error = Vec::new();
    let requests = b"{\"jsonrpc\":\"2.0\",\"id\":41,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"session/new\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":43,\"method\":\"session/prompt\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":44,\"method\":\"session/set_mode\",\"params\":{\"modeId\":\"default\"}}\n{\"jsonrpc\":\"2.0\",\"method\":\"session/cancel\",\"params\":{}}\n";
    let mut input = Cursor::new(requests);
    for step in [
        json!({"op":"awaitClient","kind":"acpInitialize"}),
        json!({"op":"emitKnownFrame","shape":"acpInitialize"}),
        json!({"op":"awaitClient","kind":"acpNew"}),
        json!({"op":"emitKnownFrame","shape":"acpSession"}),
        json!({"op":"awaitClient","kind":"acpPrompt"}),
        json!({"op":"awaitClient","kind":"acpCancel"}),
        json!({"op":"emitKnownFrame","shape":"acpCancelled"}),
    ] {
        execute_with_io(&step, &mut state, &mut input, &mut output, &mut error).unwrap();
    }
    let frames: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames[0]["id"], 41);
    assert_eq!(frames[1]["id"], 42);
    assert_eq!(frames[2]["id"], 44);
    assert_eq!(
        frames[3],
        json!({"jsonrpc":"2.0","id":43,"result":{"stopReason":"cancelled"}})
    );
}

#[test]
fn acp_replay_rejects_unmatched_permission_and_wrong_method() {
    let mut state = WireState::new();
    for bytes in [
        b"{\"id\":1,\"method\":\"session/prompt\"}\n".as_slice(),
        b"{\"id\":\"foreign-permission\",\"result\":{}}\n".as_slice(),
    ] {
        assert!(
            execute_with_io(
                &json!({"op":"awaitClient","kind":"acpPermissionDecision"}),
                &mut state,
                &mut Cursor::new(bytes),
                &mut Vec::new(),
                &mut Vec::new()
            )
            .is_err()
        );
    }
}
