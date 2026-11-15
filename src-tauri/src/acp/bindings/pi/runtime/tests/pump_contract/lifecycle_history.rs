use super::*;
use crate::acp::{projection::ItemContent, store::load_chat_page};
use sage_acp_protocol::lifecycle::{ItemLifecycle, TurnState};

struct TempRoot(std::path::PathBuf);
impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-hx2b-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn unfinished_compaction_preserves_failed_and_cancelled_outcomes_after_reopen() {
    for (stop_reason, turn_state, lifecycle) in [
        ("stop", TurnState::Completed, ItemLifecycle::Cancelled),
        ("error", TurnState::Failed, ItemLifecycle::Failed),
        ("aborted", TurnState::Interrupted, ItemLifecycle::Cancelled),
    ] {
        let root = TempRoot::new();
        let path = root.0.join("sage.db");
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        pump.push_stdout_chunk(&line(json!({"type":"compaction_start"})), 3)
            .await
            .unwrap();
        pump.push_stdout_chunk(
            &line(json!({"type":"agent_end", "messages":[{
                "role":"assistant", "stopReason":stop_reason
            }]})),
            20,
        )
        .await
        .unwrap();
        pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 21)
            .await
            .unwrap();
        assert!(pump.completed_turn_ready());
        {
            let source = fixture.database.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy)
                .unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None)
                .unwrap();
        }
        let reopened = crate::persistence::Database::open(&path).unwrap();
        for database in [&fixture.database, &reopened] {
            let connection = database.connection();
            let page =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            assert_eq!(page.turns.as_slice().len(), 1);
            let turn = &page.turns.as_slice()[0];
            assert_eq!(turn.state, turn_state);
            let compactions = turn
                .items
                .as_slice()
                .iter()
                .filter(|item| matches!(item.content, ItemContent::CompactionNotice {}))
                .collect::<Vec<_>>();
            assert_eq!(compactions.len(), 1);
            assert_eq!(compactions[0].lifecycle, lifecycle);
        }
        drop(reopened);
        fixture.finish().await;
    }
}

#[tokio::test]
async fn four_compaction_outcomes_and_cancelled_dialog_copy_survive_pump_sqlite_reopen() {
    let root = TempRoot::new();
    let path = root.0.join("sage.db");
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    for ordinal in 0..4 {
        start_turn(&mut pump, ordinal).await;
        pump.push_stdout_chunk(&line(json!({"type":"compaction_start"})), 3)
            .await
            .unwrap();
        if ordinal < 3 {
            let end = match ordinal {
                0 => json!({"type":"compaction_end","result":{}}),
                1 => json!({"type":"compaction_end","errorMessage":"Failed"}),
                _ => json!({"type":"compaction_end","aborted":true}),
            };
            pump.push_stdout_chunk(&line(end), 4).await.unwrap();
            settle(&mut pump).await;
            let idle = pump
                .try_into_completed_parts()
                .unwrap_or_else(|_| panic!("idle handoff"));
            fixture.add_turn(ordinal + 1);
            pump = idle
                .bind_turn(fixture.intent(ordinal + 1).active_turn)
                .unwrap_or_else(|_| panic!("next turn"));
        }
    }
    pump.push_stdout_chunk(
        &line(json!({"type":"extension_ui_request",
        "id":"retained-dialog","method":"input"})),
        5,
    )
    .await
    .unwrap();
    pump.flush_one(&mut Capture::default()).unwrap();
    {
        let source = fixture.database.connection();
        let mut copy = rusqlite::Connection::open(&path).unwrap();
        rusqlite::backup::Backup::new(&source, &mut copy)
            .unwrap()
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .unwrap();
    }
    let reopened = crate::persistence::Database::open(&path).unwrap();
    {
        let connection = reopened.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        assert_eq!(page.turns.as_slice().len(), 4);
        for (ordinal, lifecycle) in [
            ItemLifecycle::Completed,
            ItemLifecycle::Failed,
            ItemLifecycle::Cancelled,
            ItemLifecycle::Started,
        ]
        .into_iter()
        .enumerate()
        {
            let turn = page
                .turns
                .as_slice()
                .iter()
                .find(|turn| turn.id.as_str() == format!("turn-pump-{ordinal}"))
                .unwrap();
            let compactions = turn
                .items
                .as_slice()
                .iter()
                .filter(|item| matches!(item.content, ItemContent::CompactionNotice {}))
                .collect::<Vec<_>>();
            assert_eq!(compactions.len(), 1);
            assert_eq!(compactions[0].lifecycle, lifecycle);
        }
        let notices = page
            .turns
            .as_slice()
            .iter()
            .flat_map(|turn| turn.items.as_slice())
            .filter_map(|item| match &item.content {
                ItemContent::Unsupported {
                    category, summary, ..
                } => Some((item.lifecycle, category.as_str(), summary.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            notices,
            [(
                ItemLifecycle::Cancelled,
                "pi_dialog_cancelled",
                "Pi extension dialog was cancelled: this dialog is not supported"
            )]
        );
    }
    drop(reopened);
    settle(&mut pump).await;
    fixture.finish().await;
}
