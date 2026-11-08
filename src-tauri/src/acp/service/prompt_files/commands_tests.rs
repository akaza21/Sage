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
    use super::super::{FILE_COUNT, PASTED_TEXT_NAME, TOTAL_BYTES};
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
            Some("Pasted text exceeds the 96 KiB attachment limit.")
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
            Some("Attachments exceed 96 KiB total.")
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
