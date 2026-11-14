use super::super::Resource;
use super::*;

struct Fixture {
    state: AppState,
    project: String,
    chat: String,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-stage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap().id;
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        Self {
            state: AppState::new(database),
            project,
            chat,
            root,
        }
    }
    fn request(&self) -> StageFilesRequest {
        serde_json::from_value(serde_json::json!({
            "owner": {"projectId": self.project, "chatId": self.chat, "projectGeneration": 0},
            "pick": "files",
            "expectedRevision": null,
        }))
        .unwrap()
    }
    fn listed(&self) -> Option<StagedFiles> {
        release_detached(&self.state.acp.file_stages, &self.state.database);
        self.state
            .acp
            .file_stages
            .lock()
            .unwrap()
            .list(&self.project, &self.chat)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.acp.shutdown_blocking();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn snapshot() -> Resources {
    Resources(std::sync::Arc::new(vec![Resource {
        id: "file-1".into(),
        name: "picked.txt".into(),
        bytes: b"context".to_vec(),
    }]))
}

#[test]
fn webview_supplied_paths_are_refused() {
    let request = |body: serde_json::Value| {
        let mut value = serde_json::json!({
            "owner": {"projectId": "p", "chatId": "c", "projectGeneration": 0},
            "expectedRevision": null,
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        serde_json::from_value::<StageFilesRequest>(value)
    };
    assert!(request(serde_json::json!({"pick": "files"})).is_ok());
    assert!(request(serde_json::json!({"pick": "folder"})).is_ok());
    for refused in [
        serde_json::json!({"paths": ["/home/u/.aws/credentials"]}),
        serde_json::json!({"pick": "files", "paths": ["C:\\Users\\u\\.ssh"]}),
        serde_json::json!({"pick": "/home/u/.ssh"}),
    ] {
        assert!(request(refused.clone()).is_err(), "{refused}");
    }
}

#[test]
fn slow_picks_release_the_project_guard_and_stale_snapshots_are_not_staged() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let state = &fixture.state;
        for switch_during_read in [false, true] {
            *state.project_transitions.lock().await = 0;
            let (started, read_started) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::channel::<()>();
            let pick_and_read = move || {
                started.send(()).unwrap();
                released.recv().unwrap();
                Ok(Some(snapshot()))
            };
            let transition = async {
                read_started.await.unwrap();
                // The guard is free while the picker and reader are blocked.
                let mut epoch = state.project_transitions.try_lock().unwrap();
                if switch_during_read {
                    *epoch += 1;
                }
                drop(epoch);
                release.send(()).unwrap();
            };
            let (staged, ()) = tokio::join!(
                stage_with(state, fixture.request(), pick_and_read),
                transition
            );
            if switch_during_read {
                assert_eq!(
                    staged.err().as_deref(),
                    Some("Attachment owner is stale or invalid.")
                );
                *state.project_transitions.lock().await = 0;
                assert!(fixture.listed().is_none());
            } else {
                assert!(staged.unwrap().is_some());
                assert!(fixture.listed().is_some());
                state.acp.file_stages.lock().unwrap().clear();
            }
        }
        let cancelled = stage_with(state, fixture.request(), || Ok(None)).await;
        assert!(cancelled.unwrap().is_none());
        assert!(fixture.listed().is_none());
    });
}

#[test]
fn a_closed_tab_releases_its_batch_and_a_reopened_tab_never_inherits_it() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let state = &fixture.state;
        let stage = || stage_with(state, fixture.request(), || Ok(Some(snapshot())));
        assert!(stage().await.unwrap().is_some());
        let (tab, attached_at): (String, i64) = state
            .database
            .connection()
            .query_row(
                "SELECT tab_id, attached_at FROM chat_tabs WHERE chat_id=?1",
                [&fixture.chat],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        // Saving a layout without the tab deletes its chat_tabs row.
        state
            .database
            .connection()
            .execute("DELETE FROM chat_tabs WHERE chat_id=?1", [&fixture.chat])
            .unwrap();
        assert!(fixture.listed().is_none());
        // Stage, close and reopen with no attachment command in between: the
        // reopened tab is a new attachment and starts empty.
        state
            .database
            .connection()
            .execute(
                "INSERT INTO chat_tabs(project_id, chat_id, tab_id, attached_at) VALUES(?1,?2,?3,?4)",
                rusqlite::params![fixture.project, fixture.chat, tab, attached_at],
            )
            .unwrap();
        assert!(stage().await.unwrap().is_some());
        state
            .database
            .connection()
            .execute(
                "UPDATE chat_tabs SET attached_at=attached_at+1 WHERE chat_id=?1",
                [&fixture.chat],
            )
            .unwrap();
        assert!(fixture.listed().is_none());
        assert!(
            state
                .acp
                .file_stages
                .lock()
                .unwrap()
                .get(&fixture.project, &fixture.chat)
                .0
                .is_empty()
        );
    });
}

#[test]
fn pasted_text_joins_the_batch_as_a_text_resource_within_the_batch_caps() {
    use super::super::PASTED_TEXT_NAME;
    use sage_core::limits::prompt_files::{FILE_COUNT, TOTAL_BYTES};
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let state = &fixture.state;
        let request = |expected: Option<&str>, text: String| -> StageTextRequest {
            serde_json::from_value(serde_json::json!({
                "owner": {"projectId": fixture.project, "chatId": fixture.chat, "projectGeneration": 0},
                "expectedRevision": expected,
                "text": text,
            }))
            .unwrap()
        };
        // A paste never names a path.
        assert!(
            serde_json::from_value::<StageTextRequest>(serde_json::json!({
                "owner": {"projectId": "p", "chatId": "c", "projectGeneration": 0},
                "expectedRevision": null, "text": "x", "path": "/etc/passwd",
            }))
            .is_err()
        );
        assert_eq!(
            stage_text(state, request(None, String::new()))
                .await
                .err()
                .as_deref(),
            Some("Pasted text is empty.")
        );
        assert_eq!(
            stage_text(state, request(None, "x".repeat(TOTAL_BYTES + 1)))
                .await
                .err()
                .as_deref(),
            Some("Pasted text exceeds the 96 KB attachment limit.")
        );
        let log = "line\n".repeat(4096);
        let first = stage_text(state, request(None, log.clone())).await.unwrap();
        assert_eq!(first.files.len(), 1);
        assert_eq!(first.files[0].name, PASTED_TEXT_NAME);
        assert_eq!(first.files[0].bytes, log.len());
        // A stale revision never overwrites the batch.
        assert!(stage_text(state, request(None, "x".into())).await.is_err());
        let room = TOTAL_BYTES - log.len();
        assert_eq!(
            stage_text(state, request(Some(&first.revision), "x".repeat(room + 1)))
                .await
                .err()
                .as_deref(),
            Some("Attachments exceed 96 KB total.")
        );
        let second = stage_text(state, request(Some(&first.revision), "tail".into()))
            .await
            .unwrap();
        assert_eq!(second.files.len(), 2);
        let mut revision = second.revision;
        for _ in 2..FILE_COUNT {
            revision = stage_text(state, request(Some(&revision), "x".into()))
                .await
                .unwrap()
                .revision;
        }
        assert_eq!(
            stage_text(state, request(Some(&revision), "x".into()))
                .await
                .err()
                .as_deref(),
            Some("Attachments exceed the 16-file limit.")
        );
        // The harness receives the paste as an embedded text resource.
        let resources = state
            .acp
            .file_stages
            .lock()
            .unwrap()
            .get(&fixture.project, &fixture.chat);
        let blocks = resources.wire("Explain this log", true).unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "resource");
        assert_eq!(blocks[1]["resource"]["mimeType"], "text/plain");
        assert_eq!(blocks[1]["resource"]["text"], log.as_str());
        assert!(resources.wire("Explain this log", false).is_err());
    });
}

/// A turn in the fixture chat whose prompt payload records `count`, as
/// every prompt now records one, none included.
fn turn(fixture: &Fixture, id: &str, intent: &str, ordinal: i64, count: u8, state: &str) {
    let payload = serde_json::json!({"role": "user", "text": "Explain", "attachments": count});
    let payload = serde_json::json!({"kind": "text", "payload": payload});
    turn_with_prompt(
        fixture,
        id,
        intent,
        ordinal,
        state,
        Some(payload.to_string()),
    );
}

/// A turn in the fixture chat whose prompt item holds `payload`, or that
/// has no prompt item at all.
fn turn_with_prompt(
    fixture: &Fixture,
    id: &str,
    intent: &str,
    ordinal: i64,
    state: &str,
    payload: Option<String>,
) {
    let connection = fixture.state.database.connection();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    connection
        .execute(
            "INSERT INTO turns (id,project_id,chat_id,session_id,runtime_epoch,ordinal,
             client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
             VALUES (?1,?2,?3,'session-a',1,?4,?5,?6,'completeBytes',0,0,0,
             CASE WHEN ?6='running' THEN NULL ELSE 0 END)",
            rusqlite::params![id, fixture.project, fixture.chat, ordinal, intent, state],
        )
        .unwrap();
    let Some(payload) = payload else { return };
    connection
        .execute(
            "INSERT INTO items (id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
             origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at)
             VALUES (?1,?2,?3,'session-a',?4,1,0,'primary','text','completed',?5,?6,?7,0,0,0,0)",
            rusqlite::params![format!("item-{id}"), fixture.project, fixture.chat, id, payload, payload.len() as i64, "0".repeat(64)],
        )
        .unwrap();
}

#[test]
fn retry_reads_the_durable_attachment_count_of_that_turn_only() {
    use super::super::{RetryOrigin, Selection, retry_origin};
    let fixture = Fixture::new();
    turn(&fixture, "turn-files", "intent-files", 1, 2, "failed");
    turn(&fixture, "turn-plain", "intent-plain", 2, 0, "failed");
    let origin = |turn: &str, chat: &str| {
        retry_origin(&fixture.state.database, &fixture.project, chat, turn).unwrap()
    };
    let RetryOrigin {
        client_intent_id,
        attachments,
    } = origin("turn-files", &fixture.chat).unwrap();
    assert_eq!(
        (client_intent_id.as_str(), attachments),
        ("intent-files", Some(2))
    );
    assert_eq!(
        origin("turn-plain", &fixture.chat).unwrap().attachments,
        Some(0)
    );
    assert!(origin("turn-missing", &fixture.chat).is_none());
    assert!(origin("turn-files", "another-chat").is_none());
    // With nothing held (a restart), a turn that carried files is refused.
    let stages = fixture.state.acp.file_stages.lock().unwrap();
    let refused = stages.selected(
        &fixture.project,
        &fixture.chat,
        &Selection::Retry(origin("turn-files", &fixture.chat).unwrap()),
    );
    assert!(refused.unwrap_err().contains("no longer held"));
    let plain = Selection::Retry(origin("turn-plain", &fixture.chat).unwrap());
    assert!(
        stages
            .selected(&fixture.project, &fixture.chat, &plain)
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn retry_of_a_turn_without_a_readable_prompt_record_is_refused_as_carrying_files() {
    use super::super::{Selection, retry_origin};
    let fixture = Fixture::new();
    turn_with_prompt(&fixture, "turn-missing-item", "intent-a", 1, "failed", None);
    let unreadable = serde_json::json!({"kind": "text", "payload": {"attachments": "two"}});
    turn_with_prompt(
        &fixture,
        "turn-unreadable",
        "intent-b",
        2,
        "failed",
        Some(unreadable.to_string()),
    );
    // Builds from before the count was recorded never sent prompt files,
    // so a prompt without a count retries as plain text.
    let uncounted =
        serde_json::json!({"kind": "text", "payload": {"role": "user", "text": "Explain"}});
    turn_with_prompt(
        &fixture,
        "turn-uncounted",
        "intent-c",
        3,
        "failed",
        Some(uncounted.to_string()),
    );
    let stages = fixture.state.acp.file_stages.lock().unwrap();
    let uncounted = retry_origin(
        &fixture.state.database,
        &fixture.project,
        &fixture.chat,
        "turn-uncounted",
    )
    .unwrap()
    .unwrap();
    assert_eq!(uncounted.attachments, Some(0));
    let resent = stages
        .selected(
            &fixture.project,
            &fixture.chat,
            &Selection::Retry(uncounted),
        )
        .unwrap();
    assert!(resent.0.is_empty());
    for turn in ["turn-missing-item", "turn-unreadable"] {
        let origin = retry_origin(
            &fixture.state.database,
            &fixture.project,
            &fixture.chat,
            turn,
        )
        .unwrap()
        .unwrap();
        assert_eq!(origin.attachments, None, "{turn}");
        let refused = stages
            .selected(&fixture.project, &fixture.chat, &Selection::Retry(origin))
            .unwrap_err();
        assert!(
            refused.contains("does not show whether it carried attachments"),
            "{turn}: {refused}"
        );
    }
}

/// Stages a paste and records it as sent by `intent`, as a dispatch does.
async fn send_paste(fixture: &Fixture, intent: &str) {
    use super::super::Selection;
    let request = serde_json::from_value(serde_json::json!({
        "owner": {"projectId": fixture.project, "chatId": fixture.chat, "projectGeneration": 0},
        "expectedRevision": null, "text": "pasted log",
    }))
    .unwrap();
    let revision = stage_text(&fixture.state, request).await.unwrap().revision;
    let at = attachment(&fixture.state.database, &fixture.project, &fixture.chat)
        .unwrap()
        .unwrap();
    let mut stages = fixture.state.acp.file_stages.lock().unwrap();
    let sent = stages
        .selected(
            &fixture.project,
            &fixture.chat,
            &Selection::Revision(revision),
        )
        .unwrap();
    stages.consume(&fixture.project, &fixture.chat, at, intent, &sent);
}

#[test]
fn kept_sent_files_outlive_a_completed_turn_and_end_with_their_tab() {
    use super::super::{RetryOrigin, Selection};
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let state = &fixture.state;
        let retry = |intent: &str| {
            release_detached(&state.acp.file_stages, &state.database);
            let origin = RetryOrigin { client_intent_id: intent.into(), attachments: Some(1) };
            state.acp.file_stages.lock().unwrap()
                .selected(&fixture.project, &fixture.chat, &Selection::Retry(origin))
                .map(|files| files.0.len())
        };
        turn(&fixture, "turn-a", "intent-a", 1, 1, "running");
        send_paste(&fixture, "intent-a").await;
        assert_eq!(retry("intent-a"), Ok(1));
        // A completed turn still offers Retry, which resends its files.
        state.database.connection().execute(
            "UPDATE turns SET state='completed', terminal_at=1 WHERE client_intent_id='intent-a'", [],
        ).unwrap();
        assert_eq!(retry("intent-a"), Ok(1));
        // Closing the tab ends what was kept for it.
        send_paste(&fixture, "intent-b").await;
        assert_eq!(retry("intent-b"), Ok(1));
        state.database.connection().execute(
            "DELETE FROM chat_tabs WHERE chat_id=?1", [&fixture.chat],
        ).unwrap();
        assert!(retry("intent-b").unwrap_err().contains("no longer held"));
    });
}

#[test]
fn an_owner_without_a_chat_is_refused() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let request: StageTextRequest = serde_json::from_value(serde_json::json!({
            "owner": {"projectId": fixture.project, "chatId": "", "projectGeneration": 0},
            "expectedRevision": null, "text": "x",
        }))
        .unwrap();
        assert_eq!(
            stage_text(&fixture.state, request).await.err().as_deref(),
            Some("Attachment owner is stale or invalid.")
        );
    });
}

#[test]
fn a_paste_names_the_resource_it_added() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let request = |expected: Option<&str>| -> StageTextRequest {
            serde_json::from_value(serde_json::json!({
                "owner": {"projectId": fixture.project, "chatId": fixture.chat, "projectGeneration": 0},
                "expectedRevision": expected, "text": "pasted",
            }))
            .unwrap()
        };
        let first = stage_text(&fixture.state, request(None)).await.unwrap();
        let second = stage_text(&fixture.state, request(Some(&first.revision))).await.unwrap();
        assert_eq!(first.added.as_deref(), Some(first.files[0].id.as_str()));
        assert_eq!(second.added.as_deref(), Some(second.files[1].id.as_str()));
        assert!(fixture.listed().unwrap().added.is_none());
    });
}
