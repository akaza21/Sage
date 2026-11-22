//! Rewind here with files, replayed through the real bridge: Claude Code's
//! `rewind_files` control frames out, its recorded replies in.
use std::{cell::RefCell, path::PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    SdkBridge,
    tests::{SESSION, initialized, success},
};
use crate::acp::{
    errors::SessionControlError,
    goal::NativeRpcReply,
    service::{
        AcpService,
        file_rewind::{FileRewindFailure, files_then_conversation},
    },
    store::{AdapterStoreOwner, agent_writes, turn_undo::fixture::Fixture},
};
use crate::project_registry::ProjectRegistryStore;
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageRootId, SageTurnId},
    intent::{ExternalId, SafeResourceRef},
    scheduler::RuntimeOwner,
    session_control::{ConversationTarget, SessionControl},
};

const TURN: &str = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964ff";

struct Rewind {
    /// Holds the project directory and database for the test.
    _fixture: Fixture,
    service: AcpService,
    owner: RuntimeOwner,
    file: PathBuf,
    bridge: SdkBridge,
}

/// The agent wrote `a.rs` in the prompted Turn; Sage recorded the bytes.
fn agent_wrote_a_file() -> Rewind {
    let fixture = Fixture::new();
    let service = AcpService::new(fixture.database.clone()).unwrap();
    fixture.run();
    fixture.turn(TURN, 0);
    let root = fixture
        .database
        .root_records(fixture.project.as_str())
        .unwrap()
        .remove(0);
    let file = root.absolute_path.join("a.rs");
    std::fs::write(&file, "fn agent() {}\n").unwrap();
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let mut connection = fixture.database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    agent_writes::record(
        &transaction,
        &AdapterStoreOwner {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: SageTurnId::parse(TURN).unwrap(),
            binding_digest: "c2".repeat(32),
            external_session_id: ExternalId::new("externalSessionId", SESSION).unwrap(),
        },
        &SafeResourceRef::new(SageRootId::parse(&root.id).unwrap(), "a.rs").unwrap(),
        &Sha256::digest(b"fn agent() {}\n").into(),
        1,
    )
    .unwrap();
    transaction.commit().unwrap();
    drop(connection);
    let mut bridge = initialized();
    bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
            "sessionId":SESSION,"messageId":TURN,"prompt":[{"type":"text","text":"edit a.rs"}]}}),
        )
        .unwrap();
    Rewind {
        _fixture: fixture,
        service,
        owner,
        file,
        bridge,
    }
}

/// Sends each control through the bridge and answers it as Claude Code
/// 2.1.328 does, logging the request frames in order.
fn rewind(case: &mut Rewind, log: &RefCell<Vec<String>>) -> Result<(), FileRewindFailure<()>> {
    let target = ConversationTarget {
        turn_id: SageTurnId::parse(TURN).unwrap(),
        native_turn_id: None,
        following_messages: 0,
        prompt_digest: None,
    };
    let file = case.file.to_str().unwrap().to_owned();
    let bridge = &mut case.bridge;
    let mut control = |control: SessionControl| -> Result<NativeRpcReply, SessionControlError> {
        let sdk = bridge.session_control(&control).unwrap();
        let request = &sdk["request"];
        log.borrow_mut().push(match &request["dry_run"] {
            Value::Bool(true) => "rewind_files dry_run".to_owned(),
            _ => request["subtype"].as_str().unwrap().to_owned(),
        });
        assert_eq!(
            request["user_message_id"],
            if request["subtype"] == "rewind_files" {
                json!("6f9619ff-8b86-4011-b42d-00cf4fc964ff")
            } else {
                Value::Null
            }
        );
        let reply = match request["subtype"].as_str() {
            Some("rewind_files") => {
                json!({"canRewind":true,"filesChanged":[file],"insertions":1,"deletions":1})
            }
            _ => json!({"applied":{}}),
        };
        Ok(bridge
            .inbound(success(&sdk, reply))
            .unwrap()
            .native
            .unwrap())
    };
    files_then_conversation(
        &target,
        &mut control,
        |files| case.service.check_files(&case.owner, files),
        || {
            log.borrow_mut().push("stage".into());
            Ok((
                (),
                SessionControl::RewindConversation {
                    before: target.clone(),
                },
            ))
        },
    )
    .map(drop)
}

#[test]
fn a_file_changed_since_the_agent_wrote_it_refuses_before_any_restore() {
    let mut case = agent_wrote_a_file();
    std::fs::write(&case.file, "fn agent() {}\n// mine\n").unwrap();
    let log = RefCell::new(Vec::new());
    let refused = rewind(&mut case, &log);
    assert!(matches!(
        refused,
        Err(FileRewindFailure::Conflicts(preview))
            if preview.conflicts == ["a.rs"] && preview.file_count == 1
    ));
    assert_eq!(*log.borrow(), ["rewind_files dry_run"]);
    assert!(!case.bridge.controlling());
}

#[test]
fn files_rewind_after_a_dry_run_and_before_the_conversation_is_cut() {
    let mut case = agent_wrote_a_file();
    let log = RefCell::new(Vec::new());
    assert!(rewind(&mut case, &log).is_ok());
    assert_eq!(
        *log.borrow(),
        [
            "rewind_files dry_run",
            "stage",
            "rewind_files",
            "get_settings"
        ]
    );
}
