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
        file_rewind::{FileRewindFailure, RESTORED_NOT_CUT, plan, restore_then_cut, staged_error},
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
        Some(&Sha256::digest(b"fn agent() {}\n").into()),
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

/// How the recorded Claude Code 2.1.328 replies go in one replay.
struct Replies {
    /// `filesChanged` of both the dry run and the restore.
    changed: Vec<String>,
    /// The cut's control request is refused.
    cut_refused: bool,
}

/// The service's own sequence (dry run, check, then stage, restore and cut),
/// each control sent through the bridge and answered with a recorded reply
/// frame. Refusals map as the session control path maps them.
fn rewind(
    case: &mut Rewind,
    replies: &Replies,
    log: &RefCell<Vec<String>>,
) -> Result<bool, FileRewindFailure<()>> {
    let target = ConversationTarget {
        turn_id: SageTurnId::parse(TURN).unwrap(),
        native_turn_id: None,
        following_messages: 0,
        prompt_digest: None,
    };
    let bridge = &mut case.bridge;
    let mut control = |control: SessionControl| -> Result<NativeRpcReply, SessionControlError> {
        let sdk = bridge.session_control(&control).unwrap();
        let request = &sdk["request"];
        log.borrow_mut().push(match &request["dry_run"] {
            Value::Bool(true) => "rewind_files dry_run".to_owned(),
            _ => request["subtype"].as_str().unwrap().to_owned(),
        });
        if request["subtype"] == "rewind_files" {
            assert_eq!(
                request["user_message_id"],
                "6f9619ff-8b86-4011-b42d-00cf4fc964ff"
            );
        }
        let frame = match request["subtype"].as_str() {
            Some("rewind_files") => success(
                &sdk,
                json!({"canRewind":true,"filesChanged":replies.changed,"insertions":1,"deletions":1}),
            ),
            _ if replies.cut_refused => json!({"type":"control_response","response":{
                "subtype":"error","request_id":sdk["request_id"],"error":"Session is busy"}}),
            _ => success(&sdk, json!({"applied":{}})),
        };
        match bridge.inbound(frame).unwrap().native.unwrap() {
            NativeRpcReply::ControlRefused(reason) => Err(SessionControlError::Refused(reason)),
            reply => Ok(reply),
        }
    };
    let planned = plan(&target, &mut control).map_err(FileRewindFailure::Before)?;
    let preview =
        tauri::async_runtime::block_on(case.service.check_files(&case.owner, planned.clone()))
            .map_err(FileRewindFailure::Before)?;
    restore_then_cut(&target, &planned, preview, &mut control, || {
        log.borrow_mut().push("stage".into());
        Ok((
            (),
            SessionControl::RewindConversation {
                before: target.clone(),
            },
        ))
    })
    .map(|((), _, restored)| restored)
}

fn changed(case: &Rewind) -> Replies {
    Replies {
        changed: vec![case.file.to_str().unwrap().to_owned()],
        cut_refused: false,
    }
}

#[test]
fn a_file_changed_since_the_agent_wrote_it_refuses_before_any_restore() {
    let mut case = agent_wrote_a_file();
    std::fs::write(&case.file, "fn agent() {}\n// mine\n").unwrap();
    let log = RefCell::new(Vec::new());
    let replies = changed(&case);
    let refused = rewind(&mut case, &replies, &log);
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
    let replies = changed(&case);
    assert!(matches!(rewind(&mut case, &replies, &log), Ok(true)));
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

#[test]
fn an_empty_dry_run_restores_nothing_and_only_cuts() {
    let mut case = agent_wrote_a_file();
    let log = RefCell::new(Vec::new());
    let replies = Replies {
        changed: Vec::new(),
        cut_refused: false,
    };
    assert!(matches!(rewind(&mut case, &replies, &log), Ok(false)));
    assert_eq!(
        *log.borrow(),
        ["rewind_files dry_run", "stage", "get_settings"]
    );
}

#[test]
fn a_cut_refused_after_the_restore_says_the_files_were_restored() {
    let mut case = agent_wrote_a_file();
    let log = RefCell::new(Vec::new());
    let mut replies = changed(&case);
    replies.cut_refused = true;
    let Err(FileRewindFailure::Staged {
        restored, error, ..
    }) = rewind(&mut case, &replies, &log)
    else {
        panic!("the refused cut must fail after staging");
    };
    assert!(restored);
    assert!(matches!(
        staged_error(restored, error),
        SessionControlError::Refused(Some(text)) if text.as_str() == RESTORED_NOT_CUT
    ));
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
