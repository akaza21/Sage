//! Undo and rewind routes through the checked file seam against a harness
//! that lists, restores and cuts one file.
use super::{
    TurnUndo,
    route::{FILES_STAY, Rewind},
};
use crate::acp::{
    errors::SessionControlError,
    goal::NativeRpcReply,
    service::{
        AcpService,
        file_rewind::{RESTORED_NOT_CUT, RESTORED_RECOVERING},
    },
    store::{AdapterStoreOwner, TurnUndoneCommand, agent_writes, rewind_tests::history},
    transcript_quote::Prompt,
};
use crate::project_registry::ProjectRegistryStore;
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SafeText, SageAgentSessionId, SageRootId, SageTurnId},
    intent::{ExternalId, SafeResourceRef},
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};
use sha2::Digest;
use std::{path::PathBuf, sync::Mutex};

const AGENT: &str = "fn agent() {}\n";
const BEFORE: &str = "fn before() {}\n";
/// The first Claude Code release whose rewinds preview files.
const PREVIEWS: &str = "2.1.326";

/// One finished turn in which the agent wrote `a.rs`; the file now holds
/// `on_disk`. A `legacy` turn predates rewind targets, so the seam cannot
/// check it.
struct Written {
    history: history::History,
    service: AcpService,
    owner: RuntimeOwner,
    turn: SageTurnId,
    file: PathBuf,
    undo: TurnUndo,
}

fn written(on_disk: &str, claude: &str, legacy: bool) -> Written {
    // The service starts first: its startup recovery would settle a
    // runtime it does not run, as in `running`.
    use crate::acp::store::turn_undo::fixture::{CAPABILITIES, Fixture};
    let rewind = crate::acp::bindings::claude::sdk::rewind_capability(claude);
    let mut capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1 =
        serde_json::from_str(CAPABILITIES).unwrap();
    capabilities.rewind = rewind;
    let fixture = Fixture::with_thread(
        crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID,
        "thread-a",
        &serde_json::to_string(&capabilities).unwrap(),
    );
    let service = AcpService::new(fixture.database.clone()).unwrap();
    fixture.run();
    let mut history = history::History {
        fixture,
        turns: Vec::new(),
    };
    let prompt = Prompt::new("prompt", "edit a.rs").unwrap();
    if legacy {
        history.historical_turn(prompt, None);
    } else {
        history.turn(prompt, None);
    }
    let fixture = &history.fixture;
    let turn = history.turns[0].clone();
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let root = fixture
        .database
        .root_records(fixture.project.as_str())
        .unwrap()
        .remove(0);
    let file = root.absolute_path.join("a.rs");
    std::fs::write(&file, on_disk).unwrap();
    let mut connection = fixture.database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    agent_writes::record(
        &transaction,
        &AdapterStoreOwner {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: turn.clone(),
            binding_digest: "c2".repeat(32),
            external_session_id: ExternalId::new("externalSessionId", "thread-a").unwrap(),
        },
        &SafeResourceRef::new(SageRootId::parse(&root.id).unwrap(), "a.rs").unwrap(),
        Some(&sha2::Sha256::digest(AGENT.as_bytes()).into()),
        1,
    )
    .unwrap();
    transaction.commit().unwrap();
    drop(connection);
    Written {
        history,
        service,
        owner,
        turn,
        file,
        undo: TurnUndo::from_capability(rewind).unwrap(),
    }
}

type Outcome = Result<(TurnUndoneCommand, Option<SafeText<512>>, bool), SessionControlError>;

impl Written {
    /// Route `mode` for the turn as `apply_rewind` does, against a harness
    /// whose dry run lists `a.rs`, whose restore writes it back, whose
    /// own undo answers, and whose cut answers `cut`. Returns the outcome
    /// and the steps the harness saw.
    fn rewind(
        &self,
        mode: Rewind,
        cut: fn() -> Result<NativeRpcReply, SessionControlError>,
    ) -> (Outcome, Vec<&'static str>) {
        let steps = Mutex::new(Vec::new());
        let (file, turn) = (&self.file, &self.turn);
        let mut control = |control: SessionControl| {
            let (step, reply) = match control {
                SessionControl::RewindFiles { dry_run: true, .. } => (
                    "dry run",
                    Ok(NativeRpcReply::FileRewindPlan(vec![file.clone()])),
                ),
                SessionControl::RewindFiles { .. } => {
                    std::fs::write(file, BEFORE).unwrap();
                    (
                        "restore",
                        Ok(NativeRpcReply::FileRewindPlan(vec![file.clone()])),
                    )
                }
                SessionControl::RewindConversation { before } => {
                    assert_eq!(&before.turn_id, turn);
                    ("cut", cut())
                }
                SessionControl::UndoTurn { turn_id } => {
                    assert_eq!(&turn_id, turn);
                    ("undo", Ok(NativeRpcReply::Controlled(None)))
                }
                _ => panic!("only undo, file and conversation rewinds"),
            };
            steps.lock().unwrap().push(step);
            reply
        };
        let outcome = tauri::async_runtime::block_on(self.service.rewind_turn(
            &self.owner,
            &self.turn,
            self.undo,
            mode,
            &mut control,
            || Ok::<_, String>(None),
        ));
        (outcome, steps.into_inner().unwrap())
    }

    /// Nothing is pending and the turn is still on the page.
    fn untouched(&self) {
        let fixture = &self.history.fixture;
        crate::acp::store::turn_undo::intent::ensure_settled(
            &fixture.database.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str(),
        )
        .unwrap();
        assert_eq!(fixture.page_turns(), [self.turn.as_str()]);
    }

    /// Recording the staged undo takes the turn off the page.
    fn cut(&self, command: &TurnUndoneCommand) {
        let fixture = &self.history.fixture;
        crate::acp::store::record_turn_undone(&mut fixture.database.connection(), command)
            .unwrap();
        assert!(fixture.page_turns().is_empty());
    }

    fn on_disk(&self) -> String {
        std::fs::read_to_string(&self.file).unwrap()
    }
}

const UNDO: Rewind = Rewind::Undo { keep_files: false };
const CUT: fn() -> Result<NativeRpcReply, SessionControlError> =
    || Ok(NativeRpcReply::Controlled(None));

#[test]
fn undo_refuses_a_file_changed_since_the_agent_wrote_it_and_leaves_it_untouched() {
    let written = written("fn user() {}\n", PREVIEWS, false);
    let (outcome, steps) = written.rewind(UNDO, CUT);
    assert!(matches!(
        outcome,
        Err(SessionControlError::Refused(Some(text)))
            if text.as_str().starts_with("1 of the files changed since the agent wrote them")
    ));
    assert_eq!(steps, ["dry run"]);
    assert_eq!(written.on_disk(), "fn user() {}\n");
    written.untouched();
    // Keeping the files instead cuts the conversation with no native call.
    let (outcome, steps) = written.rewind(Rewind::Undo { keep_files: true }, CUT);
    let (command, summary, restored) = outcome.unwrap();
    assert!(steps.is_empty() && !restored);
    assert_eq!(summary.unwrap().as_str(), FILES_STAY);
    assert_eq!(written.on_disk(), "fn user() {}\n");
    written.cut(&command);
}

#[test]
fn undo_with_clean_files_restores_them_then_cuts_the_turn() {
    let written = written(AGENT, PREVIEWS, false);
    let (outcome, steps) = written.rewind(UNDO, CUT);
    let (command, summary, restored) = outcome.unwrap();
    assert_eq!(steps, ["dry run", "restore", "cut"]);
    assert!(restored);
    assert_eq!(summary.unwrap().as_str(), "Restored 1 file");
    written.cut(&command);
}

#[test]
fn undo_of_a_turn_the_seam_cannot_check_cuts_the_conversation_and_keeps_files() {
    let written = written(AGENT, PREVIEWS, true);
    let (outcome, steps) = written.rewind(UNDO, CUT);
    let (command, summary, restored) = outcome.unwrap();
    assert!(steps.is_empty() && !restored);
    assert_eq!(summary.unwrap().as_str(), FILES_STAY);
    assert_eq!(written.on_disk(), AGENT);
    written.cut(&command);
}

#[test]
fn undo_without_a_file_preview_keeps_the_bindings_own_undo() {
    let written = written(AGENT, "2.1.300", false);
    let (outcome, steps) = written.rewind(UNDO, CUT);
    let (command, _, restored) = outcome.unwrap();
    assert_eq!(steps, ["undo"]);
    assert!(!restored);
    written.cut(&command);
}

#[test]
fn a_cut_failing_after_files_are_restored_says_the_files_were_restored() {
    let written = written(AGENT, PREVIEWS, false);
    let (outcome, steps) = written.rewind(Rewind::Conversation { files: true }, || {
        Err(SessionControlError::Refused(None))
    });
    assert!(matches!(
        outcome,
        Err(SessionControlError::Refused(Some(text))) if text.as_str() == RESTORED_NOT_CUT
    ));
    assert_eq!(steps, ["dry run", "restore", "cut"]);
    written.untouched();
    // An unknown cut outcome after the restore is left to recovery, and
    // still says the files were restored.
    std::fs::write(&written.file, AGENT).unwrap();
    let (outcome, steps) = written.rewind(Rewind::Conversation { files: true }, || {
        Ok(NativeRpcReply::Rejected)
    });
    assert!(matches!(
        outcome,
        Err(SessionControlError::RecoveryRequired(Some(text)))
            if text.as_str() == RESTORED_RECOVERING
    ));
    assert_eq!(steps, ["dry run", "restore", "cut"]);
}
