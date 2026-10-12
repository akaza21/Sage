use super::turn_failures::Reopened;
use super::*;
use crate::acp::{
    lifecycle::{ItemLifecycle, TurnState},
    projection::ItemContent,
    store::load_chat_page,
};

#[tokio::test]
async fn pi_read_image_bytes_and_identity_survive_live_pump_and_sqlite_reopen() {
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/xioAAAAASUVORK5CYII=";
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    pump.push_stdout_chunk(&line(tool_start("image-read")), 3)
        .await
        .unwrap();
    let end = tool_end(
        "image-read",
        json!([{"type":"image","mimeType":"image/png","data":PNG}]),
    );
    for _ in 0..2 {
        pump.push_stdout_chunk(&line(end.clone()), 4).await.unwrap();
    }
    {
        let connection = fixture.database.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        assert_eq!(page.turns.as_slice()[0].state, TurnState::Running);
        assert_eq!(
            page.turns.as_slice()[0]
                .items
                .as_slice()
                .iter()
                .filter(|item| matches!(item.content, ItemContent::Image { .. }))
                .count(),
            1
        );
    }
    settle(&mut pump).await;
    let reopened = Reopened::new(&fixture);
    for database in [&fixture.database, &reopened.database] {
        let connection = database.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        let turn = &page.turns.as_slice()[0];
        assert_eq!(turn.state, TurnState::Completed);
        let images = turn
            .items
            .as_slice()
            .iter()
            .filter(|item| matches!(item.content, ItemContent::Image { .. }))
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].lifecycle, ItemLifecycle::Completed);
        assert!(
            matches!(&images[0].content, ItemContent::Image { media_type, data_url, source }
            if media_type.as_str() == "image/png" && data_url.as_str() == format!("data:image/png;base64,{PNG}")
            && *source == crate::acp::intent::ImageSource::Agent)
        );
        assert!(
            !turn
                .items
                .as_slice()
                .iter()
                .any(|item| matches!(item.content, ItemContent::Unsupported { .. }))
        );
    }
    drop(reopened);
    fixture.finish().await;
}

#[tokio::test]
async fn invalid_and_oversized_pi_images_reopen_as_notices_without_retained_bytes() {
    let mut oversized = "A".repeat(262_144);
    oversized.replace_range(..12, "iVBORw0KGgoA");
    for (media, data) in [
        ("image/png", "not-base64".to_owned()),
        ("image/png", oversized),
        ("image/png", "aGVsbG8=".to_owned()),
        ("image/jpeg", "aGVsbG8=".to_owned()),
        ("image/webp", "aGVsbG8=".to_owned()),
        ("image/png", "/9j/4A==".to_owned()),
        ("image/jpeg", "iVBORw0KGgo=".to_owned()),
        ("image/webp", "iVBORw0KGgo=".to_owned()),
        ("image/png", "UklGRgQAAABXRUJQ".to_owned()),
        (
            "image/gif",
            "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7".to_owned(),
        ),
    ] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        pump.push_stdout_chunk(&line(tool_start("invalid-image")), 3)
            .await
            .unwrap();
        let result = line(tool_end(
            "invalid-image",
            json!([
                {"type":"image","mimeType":media,"data":data}
            ]),
        ));
        for chunk in result.chunks(8 * 1024) {
            pump.push_stdout_chunk(chunk, 4).await.unwrap();
        }
        assert!(!pump.teardown_required());
        settle(&mut pump).await;
        let reopened = Reopened::new(&fixture);
        {
            let connection = reopened.database.connection();
            let page =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            let turn = &page.turns.as_slice()[0];
            assert_eq!(turn.state, TurnState::Completed);
            assert!(
                !turn
                    .items
                    .as_slice()
                    .iter()
                    .any(|item| matches!(item.content, ItemContent::Image { .. }))
            );
            let notices = turn
                .items
                .as_slice()
                .iter()
                .filter_map(|item| match &item.content {
                    ItemContent::Unsupported { category, .. } => Some(category.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(notices, ["pi_image"]);
            assert!(!serde_json::to_string(&page).unwrap().contains(&data));
        }
        drop(reopened);
        fixture.finish().await;
    }
}
