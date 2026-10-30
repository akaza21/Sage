use super::*;

fn connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE items(id TEXT,project_id TEXT,chat_id TEXT,kind TEXT,tool_state TEXT,payload_json TEXT,payload_bytes INTEGER);
        CREATE INDEX history_scope ON items(project_id,chat_id);").unwrap();
    connection
}

fn insert(connection: &Connection, position: usize, kind: &str, role: &str, text: &str) {
    let payload = serde_json::json!({"kind":kind,"payload":{"role":role,"text":text}}).to_string();
    connection
        .execute(
            "INSERT INTO items VALUES(?1,'project','chat',?2,NULL,?3,?4)",
            params![
                format!("item-{position}"),
                kind,
                payload,
                payload.len() as i64
            ],
        )
        .unwrap();
}

#[test]
fn history_paging_stops_at_512_candidates_including_anchors() {
    let connection = connection();
    for position in 1..=600 {
        insert(
            &connection,
            position,
            "text",
            if position % 2 == 0 {
                "assistant"
            } else {
                "user"
            },
            "small message",
        );
    }
    let watermark = watermark(&connection, "project", "chat").unwrap();
    let history = load(&connection, "project", "chat", watermark).unwrap();
    assert_eq!(watermark, 600);
    assert_eq!(history.total_items, 600);
    assert_eq!(history.items.len(), 512);
    assert_eq!(history.omissions.scan_limit, 88);
    for anchor in ["item-1", "item-599", "item-600"] {
        assert!(history.items.iter().any(|item| item.id == anchor));
    }
}

#[test]
fn history_byte_limit_preserves_exact_oversize_and_scan_omissions() {
    let connection = connection();
    insert(&connection, 1, "text", "user", "first request");
    insert(&connection, 2, "text", "user", &"large".repeat(20_000));
    for position in 3..=602 {
        insert(
            &connection,
            position,
            "text",
            "assistant",
            &"x".repeat(4_096),
        );
    }
    let history = load(&connection, "project", "chat", 602).unwrap();
    assert!(history.items.len() < 512);
    assert!(
        history
            .items
            .iter()
            .map(|item| item.text.len())
            .sum::<usize>()
            < 1_048_576
    );
    assert_eq!(history.omissions.oversize, 1);
    assert!(history.omissions.scan_limit > 0);
    assert_eq!(
        history.items.len() as u64 + history.omissions.oversize + history.omissions.scan_limit,
        602
    );
    assert!(!history.items.iter().any(|item| item.id == "item-2"));
    assert_eq!(history.items.first().unwrap().id, "item-1");
}

#[test]
fn long_chat_history_read_stays_bounded_off_the_full_scan() {
    let connection = connection();
    connection
        .execute_batch("CREATE INDEX history_role ON items(project_id,chat_id,kind,
            CASE WHEN json_extract(payload_json,'$.payload.role')='user' THEN 'user' ELSE 'assistant' END);")
        .unwrap();
    connection.execute_batch("BEGIN").unwrap();
    for position in 1..=50_000 {
        let role = if position % 2 == 0 {
            "assistant"
        } else {
            "user"
        };
        insert(&connection, position, "text", role, "long chat message");
    }
    connection.execute_batch("COMMIT").unwrap();
    let started = std::time::Instant::now();
    let watermark = watermark(&connection, "project", "chat").unwrap();
    let history = load(&connection, "project", "chat", watermark).unwrap();
    let plan = super::super::render::select_history(&history, 16_000).unwrap();
    eprintln!(
        "handoff selection over 50000 items: {:?}",
        started.elapsed()
    );
    assert_eq!(history.total_items, 50_000);
    assert_eq!(history.items.len(), 512);
    assert_eq!(history.omissions.scan_limit, 50_000 - 512);
    assert!(plan.selected_ids.len() < 512);
    assert!(plan.rendered_text.len() <= 16_000);
}
