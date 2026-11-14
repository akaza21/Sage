use super::super::{MutationDisposition, prepare_turn};
use super::{command, setup};

#[test]
fn prepare_commits_turn_user_item_and_send_intent_atomically() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(prepared.disposition, MutationDisposition::Applied);
    for table in ["turns", "items", "session_facts"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
    let duplicate = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.turn_id, prepared.turn_id);
}

#[test]
fn prepare_records_sent_images_as_user_items_right_after_the_prompt() {
    use crate::acp::attached_context::files::images::{ImagePreview, PromptImage, PromptImages};
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sage_acp_protocol::intent::{ImageMediaType, ImageSource, NormalizedPayload};
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nsent-image";
    const THUMB: &[u8] = b"\x89PNG\r\n\x1a\nthumbnail";
    let mut connection = setup();
    let mut prepare = command("Look at these");
    prepare.prompt.images = PromptImages(std::sync::Arc::new(vec![
        PromptImage {
            media_type: ImageMediaType::Png,
            bytes: PNG.to_vec(),
            preview: ImagePreview::Full,
        },
        PromptImage {
            media_type: ImageMediaType::Jpeg,
            bytes: vec![0xff; 32],
            preview: ImagePreview::Thumbnail(ImageMediaType::Png, THUMB.to_vec()),
        },
        PromptImage {
            media_type: ImageMediaType::Png,
            bytes: PNG.to_vec(),
            preview: ImagePreview::Unavailable,
        },
    ]));
    prepare_turn(&mut connection, prepare).unwrap();
    let rows: Vec<(i64, String, String)> = connection
        .prepare("SELECT ordinal,kind,payload_json FROM items ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.0, row.1.as_str()))
            .collect::<Vec<_>>(),
        [(0, "text"), (1, "image"), (2, "image")]
    );
    let images: Vec<(ImageMediaType, String, ImageSource)> = rows[1..]
        .iter()
        .map(
            |row| match serde_json::from_str::<NormalizedPayload>(&row.2).unwrap() {
                NormalizedPayload::Image {
                    media_type,
                    data,
                    source,
                } => (media_type, data.as_str().to_owned(), source),
                other => panic!("expected a user image, got {other:?}"),
            },
        )
        .collect();
    assert_eq!(
        images,
        [
            (ImageMediaType::Png, STANDARD.encode(PNG), ImageSource::User),
            (
                ImageMediaType::Png,
                STANDARD.encode(THUMB),
                ImageSource::User
            ),
        ]
    );
    let usage: (i64, i64) = connection
        .query_row(
            "SELECT item_count,item_bytes FROM acp_session_usage WHERE session_id='session-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let stored: i64 = connection
        .query_row("SELECT SUM(payload_bytes) FROM items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(usage, (3, stored));
}

#[test]
fn a_session_action_never_titles_its_chat() {
    let title = |connection: &rusqlite::Connection| -> (String, String) {
        connection
            .query_row(
                "SELECT title,title_source FROM chats WHERE id='chat-one'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    let untitled = || {
        let connection = setup();
        connection
            .execute_batch(
                "UPDATE chats SET title_source='default';
                 CREATE TABLE workbench_tabs(project_id TEXT,id TEXT,kind TEXT,title TEXT,updated_at INTEGER);
                 CREATE TABLE chat_tabs(project_id TEXT,chat_id TEXT,tab_id TEXT);",
            )
            .unwrap();
        connection
    };
    let mut connection = untitled();
    let before = title(&connection);
    let mut action = command("/compact");
    action.prompt.action =
        Some(serde_json::from_value(serde_json::json!({"kind": "compact"})).unwrap());
    prepare_turn(&mut connection, action).unwrap();
    assert_eq!(title(&connection), before);

    let mut connection = untitled();
    prepare_turn(&mut connection, command("Fix the sidebar")).unwrap();
    assert_eq!(
        title(&connection),
        ("Chat: Fix the sidebar".to_owned(), "prompt".to_owned())
    );
}
