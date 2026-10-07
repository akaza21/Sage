#![cfg(windows)]

use std::{fs, path::PathBuf};

use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::acp::{
    bindings::{
        adapter::ResourceMapper as NativeMapping,
        codex::{
            CodexAdapter, IncomingEvent, IngressDisposition, ResourceMapper as CodexMapping,
            parse_packet_metadata,
        },
    },
    intent::{Prompt, SafeResourceRef, UnavailableResourceReason},
    resource_mapper::{ConfiguredResourceRoot, NativeResourceMapper},
};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-codex-store-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct Mapper(NativeResourceMapper);

impl CodexMapping for Mapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        NativeMapping::map_existing(&self.0, absolute_path)
    }

    fn map_missing_display(&self, absolute_path: &str) -> Option<SafeResourceRef> {
        NativeMapping::map_missing_display(&self.0, absolute_path)
    }
}

fn mapper(root: &Root) -> Mapper {
    Mapper(
        NativeResourceMapper::new([ConfiguredResourceRoot {
            root_id: SageRootId::parse("root-one").unwrap(),
            canonical_native_root: root.0.clone(),
        }])
        .unwrap(),
    )
}

fn adapter(owner: &AdapterStoreOwner, mapper: &Mapper) -> CodexAdapter {
    let packet = parse_packet_metadata(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/acp/codex/candidate-windows.json"
    )))
    .unwrap();
    let mut adapter = CodexAdapter::from_metadata_packet(
        &packet,
        [0xaa; 32],
        owner.runtime_epoch,
        owner.session_id.clone(),
    );
    adapter.initialize(900).unwrap();
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"id":900,"result":{
                "userAgent":"sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
                "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.145.0-7b0ab167",
                "platformFamily":"windows","platformOs":"windows"
            }}))
            .unwrap(),
            mapper,
        )
        .unwrap();
    adapter
        .start(1, &adapter.project_root(r"C:\repo".into()).unwrap(), None)
        .unwrap();
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"id":1,"result":{
                "thread":{"id":"adapter-session"}
            }}))
            .unwrap(),
            mapper,
        )
        .unwrap();
    adapter
        .prompt(
            2,
            &Prompt::new("prompt", "edit").unwrap(),
            &RequiredText::new("client", "codex-file-change").unwrap(),
        )
        .unwrap();
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"method":"turn/started","params":{
                "threadId":"adapter-session","turn":{"id":"turn-1","status":"inProgress"}
            }}))
            .unwrap(),
            mapper,
        )
        .unwrap();
    adapter
}

fn persist_item(fixture: &mut Fixture, event: IncomingEvent) {
    let IncomingEvent::Item {
        id,
        lifecycle,
        tool_state,
        payload,
        ..
    } = event
    else {
        panic!("file change item");
    };
    let encoded = serde_json::to_string(&payload).unwrap();
    let command = ItemIngressCommand {
        project_id: fixture.owner.project_id.clone(),
        chat_id: fixture.owner.chat_id.clone(),
        session_id: fixture.owner.session_id.clone(),
        turn_id: fixture.owner.turn_id.clone(),
        runtime_epoch: fixture.owner.runtime_epoch,
        external_item_id: id.external_id,
        activity_id: None,
        payload,
        payload_digest: facts::hex_digest(encoded.as_bytes()),
        lifecycle,
        tool_state,
        now_millis: 6,
    };
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        AdapterInboundBatchCommand::new(
            fixture.owner.clone(),
            4096,
            4096,
            vec![AdapterInboundEvent::Item(command)],
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn codex_file_change_binds_only_final_disk_state_in_the_journal() {
    for (kind, initial, final_text, expected_resource, expected_operation) in [
        ("add", None, Some("created\n"), true, FileOperation::Created),
        (
            "update",
            Some("before\n"),
            Some("after\n"),
            true,
            FileOperation::Updated,
        ),
        (
            "delete",
            Some("before\n"),
            None,
            false,
            FileOperation::Deleted,
        ),
    ] {
        let mut fixture = snapshot_fixture();
        let root = Root::new();
        let mapper = mapper(&root);
        let path = root.0.join(format!("{kind}.md"));
        let absolute = path
            .to_str()
            .unwrap()
            .strip_prefix("\\\\?\\")
            .unwrap_or(path.to_str().unwrap());
        if let Some(text) = initial {
            fs::write(&path, text).unwrap();
        }
        let mut adapter = adapter(&fixture.owner, &mapper);
        let file = |method, status| {
            json!({"method":method,"params":{
                "threadId":"adapter-session","turnId":"turn-1","item":{
                    "type":"fileChange","id":"file-1","status":status,
                    "changes":[{"path":absolute,"kind":{"type":kind},"diff":"opaque"}]
                }
            }})
        };
        let IngressDisposition::Events(started) = adapter
            .ingest(
                &serde_json::to_vec(&file("item/started", "inProgress")).unwrap(),
                &mapper,
            )
            .unwrap()
        else {
            panic!("started file change");
        };
        assert!(started.is_empty());
        assert!(
            load_chat_page(&fixture.connection, project(), chat(), None)
                .unwrap()
                .turns
                .as_slice()
                .iter()
                .flat_map(|turn| turn.items.as_slice().iter())
                .all(|item| !matches!(item.content, ItemContent::FileChangeDiff { .. }))
        );
        match final_text {
            Some(text) => fs::write(&path, text).unwrap(),
            None => fs::remove_file(&path).unwrap(),
        }
        let IngressDisposition::Events(completed) = adapter
            .ingest(
                &serde_json::to_vec(&file("item/completed", "completed")).unwrap(),
                &mapper,
            )
            .unwrap()
        else {
            panic!("completed file change");
        };
        assert_eq!(completed.len(), 1);
        persist_item(&mut fixture, completed.into_iter().next().unwrap());
        let change = projected_change(&fixture.connection);
        assert_eq!(
            matches!(change.resource, SafeResourceLocation::Resource { .. }),
            expected_resource
        );
        assert_eq!(change.operation, expected_operation);
        if kind == "delete" {
            assert!(matches!(
                change.resource,
                SafeResourceLocation::Unavailable {
                    reason: UnavailableResourceReason::Missing,
                    display_resource: Some(display),
                    ..
                } if display.relative_path.as_str() == "delete.md"
            ));
        }
    }
}

#[test]
fn codex_interrupted_file_change_never_creates_a_journal_item() {
    let mut fixture = snapshot_fixture();
    let root = Root::new();
    let mapper = mapper(&root);
    let path = root.0.join("interrupted.md");
    fs::write(&path, "preexisting\n").unwrap();
    let mut adapter = adapter(&fixture.owner, &mapper);
    let started = json!({"method":"item/started","params":{
        "threadId":"adapter-session","turnId":"turn-1","item":{
            "type":"fileChange","id":"file-1","status":"inProgress",
            "changes":[{"path":path.to_str().unwrap(),"kind":{"type":"add"},"diff":"opaque"}]
        }
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &mapper)
        .unwrap()
    else {
        panic!("started file change");
    };
    assert!(events.is_empty());
    let interrupted = json!({"method":"turn/completed","params":{
        "threadId":"adapter-session","turn":{"id":"turn-1","status":"interrupted"}
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&interrupted).unwrap(), &mapper)
        .unwrap()
    else {
        panic!("interrupted turn");
    };
    let [IncomingEvent::Turn { state, .. }] = events.as_slice() else {
        panic!("turn terminal");
    };
    assert_eq!(*state, TurnState::Interrupted);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        AdapterInboundBatchCommand::new(
            fixture.owner.clone(),
            4096,
            4096,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                AdapterInboundEvent::Turn {
                    state: *state,
                    now_millis: 7,
                },
            ],
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        load_chat_page(&fixture.connection, project(), chat(), None)
            .unwrap()
            .turns
            .as_slice()
            .iter()
            .flat_map(|turn| turn.items.as_slice().iter())
            .all(|item| !matches!(item.content, ItemContent::FileChangeDiff { .. }))
    );
}
