use super::*;
use crate::acp::{
    bindings::codex::{CodexRuntime, Platform},
    runtime::{JournalHandle, ProjectionMailbox, SqliteAdapterBatchJournal},
    store::DatabaseAcpStore,
};

#[test]
fn codex_wire_child_identity_survives_pump_store_and_reopen() {
    runtime().block_on(async {
        let database = super::sign_in_tests::migrated_database();
        let mut adapter = ready_codex_parent();
        let intent = session_intent().bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: Some(ExternalId::new("turn", "turn-1").unwrap()),
            client_message_id: message(),
        });
        let prompt = adapter.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: message(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }).unwrap();
        let prompt_id = codex_request_id(&prompt);
        adapter.ingest(&codex_frame(serde_json::json!({
            "id":prompt_id,"result":{"turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();
        adapter.ingest(&codex_frame(serde_json::json!({
            "method":"turn/started","params":{"threadId":"external-session","turn":{"id":"turn-1"}}
        })), &Mapper).unwrap();

        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let mut pump = AdapterPump::new(
            adapter,
            SqliteAdapterBatchJournal::new(handle.clone(), intent.clone()),
            Mapper,
            Arc::new(ProjectionMailbox::default()),
            intent,
        ).unwrap();
        let wire = concat!(
            r#"{"method":"item/completed","params":{"threadId":"external-session","turnId":"turn-1","item":{"id":"activity-native","type":"subAgentActivity","agentThreadId":"child-native","agentPath":"Worker","kind":"completed"}}}"#,
            "\n",
        );
        assert!(pump.push_stdout_chunk(wire.as_bytes(), 9).await.unwrap());
        drop(pump);
        handle.close_when_drained();
        task.await.unwrap();

        let directory = std::env::temp_dir().join(format!("sage-codex-child-reopen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        {
            let source = database.connection();
            let mut copy = rusqlite::Connection::open(&path).unwrap();
            rusqlite::backup::Backup::new(&source, &mut copy).unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None).unwrap();
        }
        drop(database);
        let reopened = crate::acp::migration::open_or_migrate(&path).unwrap();
        let stored: (String, String, String, String, String, i64) = reopened.query_row(
            "SELECT o.parent_thread_id,o.native_child_thread_id,b.session_id,b.turn_id,i.external_item_id,b.runtime_epoch
             FROM acp_child_thread_owners o JOIN acp_child_thread_bindings b
               ON b.native_child_thread_id=o.native_child_thread_id
             JOIN items i ON i.id=b.item_id",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
        ).unwrap();
        assert_eq!(stored, (
            "external-session".into(), "child-native".into(), "session-one".into(),
            "turn-one".into(), "activity-native".into(), 1,
        ));
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

fn ready_codex_parent() -> CodexRuntime {
    let mut adapter =
        CodexRuntime::fixture_with_identity(Platform::WindowsX86_64, [7; 32], session("one"));
    let initialize = adapter.command(AdapterCommand::Initialize).unwrap();
    let initialize_id = codex_request_id(&initialize);
    adapter
        .resolve_egress(initialize.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = adapter
        .ingest(
            &codex_frame(serde_json::json!({
                "id":initialize_id,"result":{
                    "userAgent":"sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
                    "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.145.0-7b0ab167",
                    "platformFamily":"windows","platformOs":"windows"
                }
            })),
            &Mapper,
        )
        .unwrap();
    adapter
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let session = adapter
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let session_id = codex_request_id(&session);
    adapter
        .ingest(
            &codex_frame(serde_json::json!({
                "id":session_id,"result":{"thread":{"id":"external-session"}}
            })),
            &Mapper,
        )
        .unwrap();
    adapter
}

fn codex_request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<serde_json::Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn codex_frame(value: serde_json::Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap()
}
