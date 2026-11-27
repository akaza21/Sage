//! One Attach pick: a picked PNG reaches the chat as image content when the
//! harness takes images, and stays a file when it does not.
use super::super::read::read_picked;
use super::*;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use base64::{Engine as _, engine::general_purpose::STANDARD};

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

struct Fixture {
    state: AppState,
    project: String,
    chat: String,
    root: std::path::PathBuf,
    picks: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-pick-{}", uuid::Uuid::new_v4()));
        let picks = root.join("picks");
        std::fs::create_dir_all(&picks).unwrap();
        std::fs::write(picks.join("shot.png"), STANDARD.decode(PNG_1X1).unwrap()).unwrap();
        std::fs::write(picks.join("notes.txt"), b"context").unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let project = database.create_project_with_chat(&root).unwrap().id;
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
            picks,
        }
    }
    async fn pick(&self, owner: serde_json::Value, images: bool) -> StagedPick {
        self.pick_at(owner, images, None).await.unwrap().unwrap()
    }
    async fn pick_at(
        &self,
        owner: serde_json::Value,
        images: bool,
        expected: Option<&str>,
    ) -> Result<Option<StagedPick>, String> {
        let request = serde_json::from_value(serde_json::json!({
            "owner": owner, "pick": "files", "expectedRevision": expected, "images": images,
        }))
        .unwrap();
        let paths = ["shot.png", "notes.txt"].map(|name| self.picks.join(name));
        let paths = paths.map(|path| path.to_str().unwrap().to_owned());
        stage_with(&self.state, request, move || {
            read_picked(&paths, images).map(Some)
        })
        .await
    }
    fn artifacts(&self) -> i64 {
        self.state
            .database
            .connection()
            .query_row("SELECT COUNT(*) FROM chat_image_artifacts", [], |row| {
                row.get(0)
            })
            .unwrap()
    }
    fn chat_owner(&self) -> serde_json::Value {
        serde_json::json!({"projectId": self.project, "chatId": self.chat, "projectGeneration": 0})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.state.acp.shutdown_blocking();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn names(staged: &StagedPick) -> Vec<&str> {
    let files = staged.files.as_ref().map(|files| files.files.as_slice());
    files
        .unwrap_or_default()
        .iter()
        .map(|file| file.name.as_str())
        .collect()
}

#[test]
fn a_picked_png_reaches_the_chat_as_an_image_artifact_and_text_stays_a_file() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let staged = fixture.pick(fixture.chat_owner(), true).await;
        assert_eq!(names(&staged), ["notes.txt"]);
        assert!(staged.draft_images.is_empty());
        let [image] = staged.images.as_slice() else {
            panic!("one image artifact");
        };
        assert_eq!(image.label, "shot.png");
        assert_eq!((image.width, image.height), (1, 1));
        // The send path reads prompt images through this same check.
        let database = &fixture.state.database;
        let bytes = sage_images::verify_image_reference(
            &database.connection(),
            database.image_artifact_root().unwrap(),
            &fixture.project,
            &fixture.chat,
            image,
        )
        .unwrap();
        assert_eq!(bytes, STANDARD.decode(PNG_1X1).unwrap());
    });
}

#[test]
fn without_image_support_a_picked_png_is_staged_as_a_file() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let staged = fixture.pick(fixture.chat_owner(), false).await;
        assert_eq!(names(&staged), ["shot.png", "notes.txt"]);
        assert!(staged.images.is_empty() && staged.draft_images.is_empty());
        assert_eq!(fixture.artifacts(), 0);
    });
}

#[test]
fn a_drafts_picked_png_comes_back_as_image_bytes_for_the_draft() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let token = uuid::Uuid::new_v4().to_string();
        let staged = fixture
            .pick(serde_json::json!({"draftToken": token}), true)
            .await;
        assert_eq!(names(&staged), ["notes.txt"]);
        assert!(staged.images.is_empty());
        let value = serde_json::to_value(&staged.draft_images).unwrap();
        assert_eq!(
            value,
            serde_json::json!([{"mediaType": "image/png", "label": "shot.png", "dataBase64": PNG_1X1}])
        );
    });
}

#[test]
fn a_pick_over_the_message_image_count_is_refused() {
    let fixture = Fixture::new();
    let png = STANDARD.decode(PNG_1X1).unwrap();
    let paths: Vec<String> = (0..=sage_core::limits::PROMPT_IMAGE_COUNT)
        .map(|index| {
            let path = fixture.picks.join(format!("{index}.png"));
            std::fs::write(&path, &png).unwrap();
            path.to_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        read_picked(&paths, true).err(),
        Some(super::super::limits::IMAGE_OVER_COUNT.as_str())
    );
    assert!(read_picked(&paths[1..], true).is_ok());
}

#[test]
fn a_pick_whose_files_fail_to_stage_removes_the_images_it_stored() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = Fixture::new();
        let stale = fixture
            .pick_at(fixture.chat_owner(), true, Some("stale-revision"))
            .await;
        assert!(stale.is_err());
        assert_eq!(fixture.artifacts(), 0);
    });
}
