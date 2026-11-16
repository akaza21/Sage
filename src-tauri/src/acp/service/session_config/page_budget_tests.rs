use rusqlite::params;
use sha2::{Digest, Sha256};

use super::restore_tests::{option, owner, start};
use super::stored_tests::seed_chat;
use super::*;
use crate::acp::session_config::notices::{self, NoticeReason, SessionConfigNotice};
use sage_acp_protocol::session_config::ConfigId;

const ITEMS: usize = 9;
const ITEM_TEXT: usize = 220_000;
/// What the history loader keeps free for a next-page cursor.
const CURSOR_RESERVE: usize = 514;

fn item_body(text_len: usize) -> String {
    serde_json::json!({"kind":"text","payload":{"role":"user","text":"x".repeat(text_len)}})
        .to_string()
}

fn write_item(connection: &rusqlite::Connection, index: usize, text_len: usize) {
    let body = item_body(text_len);
    let digest: String = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    connection
        .execute(
            "INSERT INTO items VALUES(?1,'project-restore','chat-restore','session-restore','turn-0',1,?2,'primary',NULL,'text','completed',NULL,NULL,?3,?4,?5,0,1,1,1)",
            params![format!("item-{index}"), index as i64, body, body.len() as i64, digest],
        )
        .unwrap();
}

fn bare_page(service: &AcpService) -> AcpChatPage {
    let first = owner(1);
    crate::acp::store::load_chat_page(
        &service.database.connection(),
        first.project_id,
        first.chat_id,
        None,
    )
    .unwrap()
}

fn bytes(page: &AcpChatPage) -> usize {
    serde_json::to_vec(page).unwrap().len()
}

fn items(page: &AcpChatPage) -> usize {
    page.turns
        .as_slice()
        .iter()
        .map(|turn| turn.items.as_slice().len())
        .sum()
}

/// Terminal items cannot be rewritten, so each size gets a chat of its own.
fn parked_chat(last_len: usize) -> AcpService {
    let service =
        AcpService::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap())
            .unwrap();
    seed_chat(&service);
    let first = owner(1);
    start(
        &service,
        &first,
        serde_json::json!([{
            "id":"model","name":"Model","category":"model","type":"select","currentValue":"a",
            "options":[{"value":"a","name":"Model A"},{"value":"b","name":"Model B"}]
        }]),
    );
    service.shutdown_blocking().unwrap();
    let connection = service.database.connection();
    crate::acp::session_config::choices::record(
        &connection,
        &first.session_id,
        &option("model", "a"),
        1,
    )
    .unwrap();
    notices::set(
        &connection,
        &first.session_id,
        &SessionConfigNotice {
            config_id: ConfigId::new("configId", "model").unwrap(),
            saved: ConfigId::new("saved", "m".repeat(1024)).unwrap(),
            current: ConfigId::new("current", "a").unwrap(),
            reason: NoticeReason::NotOffered,
        },
        1,
    )
    .unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-restore','chat-restore','session-restore',1,'exited',NULL,0,1,'fixture')", []).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-0','project-restore','chat-restore','session-restore',1,0,'turn-0','completed','correlated',0,1,1,1)", []).unwrap();
    for index in 0..ITEMS {
        write_item(&connection, index, ITEM_TEXT);
    }
    write_item(&connection, ITEMS, last_len);
    drop(connection);
    service
}

#[test]
fn a_full_history_keeps_room_for_the_usage_flag() {
    let limit = sage_core::limits::LIMITS.durable.history_page_bytes as usize;
    let flag = r#","usage":true"#.len();
    let slack = limit - CURSOR_RESERVE - bytes(&bare_page(&parked_chat(1)));
    // Fits the page only if nothing is kept for the flag.
    let mut page = bare_page(&parked_chat(1 + slack - flag / 2));
    assert_eq!(items(&page), ITEMS);
    page.usage = true;
    assert!(bytes(&page) <= limit);
}

#[test]
fn a_nearly_full_history_yields_room_for_the_restored_config_and_its_notice() {
    let first = owner(1);
    let limit = sage_core::limits::LIMITS.durable.history_page_bytes as usize;
    // Size the last item so the history alone leaves less room than the
    // cursor reserve plus a few bytes.
    let slack = limit - CURSOR_RESERVE - bytes(&bare_page(&parked_chat(1)));
    let service = parked_chat(1 + slack - 64);
    let history = bare_page(&service);
    assert_eq!(items(&history), ITEMS + 1);
    assert!(bytes(&history) > limit - CURSOR_RESERVE - 128);

    let page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    let restored = page.restored_session_config.as_ref().unwrap();
    assert_eq!(
        restored.config.config_options.as_slice()[0].name.as_str(),
        "Model"
    );
    assert_eq!(
        page.session_config_notice
            .as_ref()
            .unwrap()
            .saved
            .as_str()
            .len(),
        1024
    );
    assert_eq!(items(&page), ITEMS);
    assert!(bytes(&page) <= limit);
}
