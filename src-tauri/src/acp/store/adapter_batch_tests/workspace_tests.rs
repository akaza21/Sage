use super::*;
use crate::{
    acp::runtime::{JournalHandle, JournalLane},
    persistence::Database,
};
use std::path::PathBuf;

fn workspace_fixture() -> (Database, AdapterStoreOwner, PathBuf) {
    let fixture = migrated_fixture();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    {
        let mut connection = database.connection();
        let backup = rusqlite::backup::Backup::new(&fixture.connection, &mut connection).unwrap();
        backup
            .run_to_completion(128, std::time::Duration::ZERO, None)
            .unwrap();
    }
    let root =
        std::env::temp_dir().join(format!("sage-workspace-terminal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        crate::test_git::command()
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let root = root.canonicalize().unwrap();
    register(&database, &root, "intent-one");
    (database, fixture.owner, root)
}

fn register(database: &Database, root: &std::path::Path, key: &str) {
    let baseline = tauri::async_runtime::block_on(crate::turn_workspace::capture(root)).unwrap();
    crate::turn_workspace::remember(
        &mut database.connection(),
        "session-one",
        key,
        "root-one",
        root,
        Some(baseline),
    )
    .unwrap();
}

fn terminal(owner: &AdapterStoreOwner) -> StoreMutation {
    StoreMutation::AdapterInboundBatch(batch(
        owner,
        vec![AdapterInboundEvent::Turn {
            state: TurnState::Completed,
            now_millis: 7,
        }],
    ))
}

fn commit(
    database: &Database,
    mutation: StoreMutation,
    deadline: u64,
) -> Result<StoreCommit, crate::acp::runtime::JournalError> {
    let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
    let journal = journal.with_workspace_tracking(database.clone());
    let ticket = journal
        .enqueue_one(JournalLane::Control, mutation, deadline)
        .unwrap();
    journal.close_when_drained();
    runner.run_blocking();
    ticket.blocking_committed()
}

fn paths(database: &Database, key: &str) -> Vec<String> {
    let changes =
        crate::turn_workspace::load(&database.connection(), "session-one", key, "root-one")
            .unwrap();
    assert!(changes.tracked);
    changes
        .changes
        .into_iter()
        .map(|change| change.relative_path)
        .collect()
}

#[test]
fn journal_terminal_adapter_commit_freezes_shell_edits_before_any_ui_read() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (database, owner, root) = workspace_fixture();
    std::fs::write(root.join("shell.txt"), "From the turn").unwrap();
    commit(&database, terminal(&owner), 0).unwrap();
    std::fs::write(root.join("later.txt"), "Another chat").unwrap();
    assert_eq!(paths(&database, "intent-one"), ["shell.txt"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_and_expired_terminal_commands_never_consume_the_baseline() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (database, owner, root) = workspace_fixture();
    std::fs::write(root.join("first.txt"), "Before rejected event").unwrap();
    let mut rejected = owner.clone();
    rejected.binding_digest = "f".repeat(64);
    assert!(commit(&database, terminal(&rejected), 0).is_err());
    let baseline: bool = database.connection().query_row("SELECT baseline_json IS NOT NULL AND result_json IS NULL AND terminal_json IS NULL FROM acp_turn_workspace_changes",[],|row| row.get(0)).unwrap();
    assert!(baseline);
    assert!(commit(&database, terminal(&owner), 1).is_err());
    std::fs::write(root.join("second.txt"), "After failed events").unwrap();
    commit(&database, terminal(&owner), 0).unwrap();
    assert_eq!(paths(&database, "intent-one"), ["first.txt", "second.txt"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn replay_of_an_old_turn_or_epoch_cannot_freeze_a_newer_turn() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (database, owner, root) = workspace_fixture();
    commit(&database, terminal(&owner), 0).unwrap();
    database.connection().execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at) VALUES('turn-two','project-one','chat-one','session-one',1,1,'intent-two','running','correlated',8,8)",[]).unwrap();
    register(&database, &root, "intent-two");
    commit(&database, terminal(&owner), 0).unwrap();
    let mut stale_epoch = owner.clone();
    stale_epoch.turn_id = SageTurnId::parse("turn-two").unwrap();
    stale_epoch.runtime_epoch = RuntimeEpoch::new(2).unwrap();
    assert!(commit(&database, terminal(&stale_epoch), 0).is_err());
    let staged: bool = database.connection().query_row("SELECT terminal_json IS NOT NULL FROM acp_turn_workspace_changes WHERE client_intent_id='intent-two'",[],|row| row.get(0)).unwrap();
    assert!(!staged);
    std::fs::write(root.join("new.txt"), "New turn only").unwrap();
    let mut current = owner;
    current.turn_id = SageTurnId::parse("turn-two").unwrap();
    commit(&database, terminal(&current), 0).unwrap();
    assert_eq!(paths(&database, "intent-two"), ["new.txt"]);
    assert!(paths(&database, "intent-one").is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn journal_close_and_native_end_capture_the_terminal_turn() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    for close in [false, true] {
        let (database, owner, root) = workspace_fixture();
        std::fs::write(root.join("shell.txt"), "Before native end").unwrap();
        if close {
            let request = CloseSessionCommand {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::parse("close-workspace").unwrap(),
                now_millis: 8,
            };
            commit(&database, StoreMutation::CloseSession(request.clone()), 0).unwrap();
            commit(
                &database,
                StoreMutation::FinalizeClose(FinalizeCloseCommand { request }),
                0,
            )
            .unwrap();
        } else {
            commit(
                &database,
                StoreMutation::RecoverRuntimeAfterZero(RecoverRuntimeCommand {
                    project_id: owner.project_id,
                    chat_id: owner.chat_id,
                    session_id: owner.session_id,
                    runtime_epoch: owner.runtime_epoch,
                    now_millis: 8,
                }),
                0,
            )
            .unwrap();
        }
        std::fs::write(root.join("later.txt"), "After native end").unwrap();
        assert_eq!(paths(&database, "intent-one"), ["shell.txt"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
