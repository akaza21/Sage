//! An undo the harness already carried out is answered by the pump without
//! another frame, so an undo Sage could not record is recorded on retry.

use super::*;
use crate::acp::{contract::SageTurnId, goal::NativeRpcReply, session_control::SessionControl};

const TURN: &str = "turn-6f9619ff-8b86-d011-b42d-00cf4fc964ff";

fn undo() -> SessionControl {
    SessionControl::UndoTurn {
        turn_id: SageTurnId::parse(TURN.to_owned()).unwrap(),
    }
}

/// Runs one Turn to its end and reverts it, as the pump would have.
fn reverted_codex() -> CodexRuntime {
    let mut codex = ready_codex();
    let prompt = codex
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", TURN).unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = codex_request_id(&prompt);
    codex
        .resolve_egress(prompt.into_token(), EgressResolution::Written)
        .unwrap();
    for frame in [
        serde_json::json!({"id":prompt_id,"result":{"turn":{"id":"turn-1"}}}),
        serde_json::json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1"}}}),
        serde_json::json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":"turn-1","status":"completed","items":[]}
        }}),
    ] {
        codex.ingest(&codex_frame(frame), &Mapper).unwrap();
    }
    let revert = codex.session_control(&undo()).unwrap();
    let revert_id = codex_request_id(&revert);
    codex
        .resolve_egress(revert.into_token(), EgressResolution::Written)
        .unwrap();
    codex
        .ingest(
            &codex_frame(serde_json::json!({
                "id":revert_id,"result":{"thread":{"id":"thread-1","turns":[]}}
            })),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        codex.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    codex
}

#[test]
fn a_repeated_undo_of_the_reverted_turn_is_answered_without_a_frame() {
    let (mut pump, _) = codex_pump(reverted_codex());
    let mut output = CountingTransport::default();
    pump.write_control(&mut output, &undo()).unwrap();
    assert_eq!(output.0, 0, "Codex is not asked again");
    assert_eq!(
        pump.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
}
