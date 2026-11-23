use super::super::{Resource, Selection};
use super::*;
use crate::chat_lifecycle::create::ChatCreateStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use serde_json::json;

const DRAFT: &str = "0b6f3c1e-8a2d-4f5b-9c7e-1d2a3b4c5d6e";
const LEFTOVER: &str = "7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

struct Fixture {
    state: AppState,
    project: String,
    chat: String,
    second_chat: String,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-draft-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_with_chat(&root).unwrap().id;
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let second_chat = database
            .create_chat(&sage_domain::chat::ProjectLayoutRequest {
                project_id: project.clone(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: snapshot.selected_root_id,
                selected_resource: snapshot.selected_resource,
            })
            .unwrap();
        Self {
            state: AppState::new(database),
            project,
            chat,
            second_chat,
            root,
        }
    }
    fn chat(&self) -> serde_json::Value {
        json!({"projectId": self.project, "chatId": self.chat, "projectGeneration": 0})
    }
    fn second(&self) -> serde_json::Value {
        json!({"projectId": self.project, "chatId": self.second_chat, "projectGeneration": 0})
    }
    async fn stage(&self, token: &str, name: &str) -> Result<Option<StagedFiles>, String> {
        let request = serde_json::from_value(json!({
            "owner": {"draftToken": token}, "pick": "files", "expectedRevision": null,
        }))
        .unwrap();
        let picked = Resources(std::sync::Arc::new(vec![Resource {
            id: format!("{name}-id"),
            name: name.into(),
            bytes: b"context".to_vec(),
        }]));
        stage_with(&self.state, request, move || Ok(Some(picked))).await
    }
    async fn hand_off(
        &self,
        from: serde_json::Value,
        to: serde_json::Value,
        revision: &str,
    ) -> Result<StagedFiles, String> {
        let request = serde_json::from_value(json!({"from": from, "to": to, "revision": revision}))
            .map_err(|error| error.to_string())?;
        hand_off(&self.state, request).await
    }
    /// What `list_prompt_files` answers, detached tabs released first.
    fn listed(&self, project: &str, chat: &str) -> Option<Vec<String>> {
        release_detached(&self.state.acp.file_stages, &self.state.database);
        let stages = self.state.acp.file_stages.lock().unwrap();
        stages
            .list(project, chat)
            .map(|staged| staged.files.into_iter().map(|file| file.name).collect())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.acp.shutdown_blocking();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn draft(token: &str) -> serde_json::Value {
    json!({"draftToken": token})
}

#[test]
fn a_draft_batch_goes_whole_to_the_created_chat_and_is_sent_once() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let staged = fixture.stage(DRAFT, "notes.txt").await.unwrap().unwrap();
        // A tab sweep never ends a draft's batch, and no chat can read it.
        assert_eq!(fixture.listed("", DRAFT), Some(vec!["notes.txt".into()]));
        assert_eq!(fixture.listed(&fixture.project, &fixture.chat), None);
        assert!(fixture.stage("not-a-token", "x").await.is_err());

        // A stale revision moves nothing; the draft keeps its batch.
        let stale = fixture
            .hand_off(draft(DRAFT), fixture.chat(), "stale")
            .await;
        assert!(stale.is_err());
        assert_eq!(fixture.listed(&fixture.project, &fixture.chat), None);
        let moved = fixture
            .hand_off(draft(DRAFT), fixture.chat(), &staged.revision)
            .await
            .unwrap();
        assert_eq!(fixture.listed("", DRAFT), None);
        assert_eq!(
            fixture.listed(&fixture.project, &fixture.chat),
            Some(vec!["notes.txt".into()])
        );
        // The same batch cannot be handed off a second time.
        let again = fixture.hand_off(draft(DRAFT), fixture.chat(), &staged.revision);
        assert!(again.await.is_err());

        // The chat's first turn takes exactly those files; dispatch consumes them.
        let mut stages = fixture.state.acp.file_stages.lock().unwrap();
        let (project, chat) = (fixture.project.as_str(), fixture.chat.as_str());
        let sent = stages
            .selected(project, chat, &Selection::Revision(moved.revision))
            .unwrap();
        assert_eq!(sent.0[0].name, "notes.txt");
        assert_eq!(sent.0[0].bytes, b"context");
        stages.consume(project, chat, 1, "intent-first", &sent);
        assert!(stages.list(project, chat).is_none());
        assert!(stages.list("", DRAFT).is_none());
    });
}

#[test]
fn a_draft_batch_goes_back_only_to_undo_its_own_handoff_and_never_reaches_another_chat() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let (a, b) = (fixture.chat(), fixture.second());
        let staged = fixture.stage(DRAFT, "a.rs").await.unwrap().unwrap();
        let moved = fixture
            .hand_off(draft(DRAFT), a.clone(), &staged.revision)
            .await
            .unwrap();
        // Chat to chat is refused, and a stale undo moves nothing.
        assert!(
            fixture
                .hand_off(a.clone(), b.clone(), &moved.revision)
                .await
                .is_err()
        );
        assert!(
            fixture
                .hand_off(a.clone(), draft(DRAFT), "stale")
                .await
                .is_err()
        );
        // The undo of that exact move returns the batch, once.
        let back = fixture
            .hand_off(a.clone(), draft(DRAFT), &moved.revision)
            .await
            .unwrap();
        assert_eq!(fixture.listed("", DRAFT), Some(vec!["a.rs".into()]));
        assert_eq!(fixture.listed(&fixture.project, &fixture.chat), None);
        let again = fixture
            .hand_off(draft(DRAFT), a.clone(), &back.revision)
            .await
            .unwrap();
        fixture
            .state
            .acp
            .file_stages
            .lock()
            .unwrap()
            .clear_chat(&fixture.project, &fixture.chat);
        assert!(
            fixture
                .hand_off(a.clone(), draft(DRAFT), &again.revision)
                .await
                .is_err()
        );

        // Two hops: a chat's own files never pass through the draft to another chat.
        let chat_a = json!({"owner": a, "pick": "files", "expectedRevision": null});
        let request = serde_json::from_value(chat_a).unwrap();
        let picked = Resources(std::sync::Arc::new(vec![Resource {
            id: "own-id".into(),
            name: "own.txt".into(),
            bytes: b"chat".to_vec(),
        }]));
        let own = stage_with(&fixture.state, request, move || Ok(Some(picked)))
            .await
            .unwrap()
            .unwrap();
        assert!(
            fixture
                .hand_off(a.clone(), draft(LEFTOVER), &own.revision)
                .await
                .is_err()
        );
        assert_eq!(fixture.listed("", LEFTOVER), None);
        assert_eq!(fixture.listed(&fixture.project, &fixture.second_chat), None);
        assert_eq!(
            fixture.listed(&fixture.project, &fixture.chat),
            Some(vec!["own.txt".into()])
        );
    });
}

#[test]
fn a_project_switch_returns_a_handed_unsent_batch_to_its_draft() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let staged = fixture.stage(DRAFT, "a.rs").await.unwrap().unwrap();
        fixture
            .hand_off(draft(DRAFT), fixture.chat(), &staged.revision)
            .await
            .unwrap();
        fixture.state.acp.file_stages.lock().unwrap().clear_chats();
        assert_eq!(fixture.listed("", DRAFT), Some(vec!["a.rs".into()]));
        assert_eq!(fixture.listed(&fixture.project, &fixture.chat), None);
    });
}

#[test]
fn discard_and_a_new_draft_sweep_leftover_slots_while_a_project_switch_keeps_the_draft() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        // A WebView that reloaded left this slot; its token is gone with it.
        fixture.stage(LEFTOVER, "old.txt").await.unwrap();
        fixture.stage(DRAFT, "new.txt").await.unwrap();
        assert_eq!(fixture.listed("", LEFTOVER), None);
        assert_eq!(fixture.listed("", DRAFT), Some(vec!["new.txt".into()]));

        fixture.state.acp.file_stages.lock().unwrap().clear_chats();
        assert_eq!(fixture.listed("", DRAFT), Some(vec!["new.txt".into()]));

        let (project, token, _) = serde_json::from_value::<DraftOwner>(draft(DRAFT))
            .unwrap()
            .key()
            .unwrap();
        let mut stages = fixture.state.acp.file_stages.lock().unwrap();
        stages.release_draft(&project, &token);
        assert!(stages.list("", DRAFT).is_none());
    });
}

#[test]
fn a_draft_is_held_to_the_chat_batch_caps() {
    use sage_core::limits::prompt_files::TOTAL_BYTES;
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let paste = |text: String| {
            serde_json::from_value::<StageTextRequest>(json!({
                "owner": draft(DRAFT), "expectedRevision": null, "text": text,
            }))
            .unwrap()
        };
        let over = stage_text(&fixture.state, paste("x".repeat(TOTAL_BYTES + 1))).await;
        assert_eq!(
            over.err().as_deref(),
            Some("Pasted text exceeds the 96 KB attachment limit.")
        );
        assert_eq!(fixture.listed("", DRAFT), None);
        let within = stage_text(&fixture.state, paste("x".repeat(TOTAL_BYTES))).await;
        assert!(within.is_ok());
    });
}
