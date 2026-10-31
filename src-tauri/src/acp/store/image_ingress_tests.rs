use super::super::{extended_tests::*, *};
use crate::acp::{
    bindings::{codex, standard_acp},
    contract::{ClientIntentId, SafeText},
    intent::{
        ImageMediaType, ImageSource, NormalizedPayload, SafeResourceRef, UnavailableResourceReason,
    },
    lifecycle::ItemLifecycle,
    projection::ItemContent,
};
use rusqlite::Connection;
use serde_json::json;

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

struct NoResources;

impl standard_acp::translator::ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

impl codex::ResourceMapper for NoResources {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-image-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn claude_acp_image_chunk_survives_sqlite_chat_reload() {
    let mut translator = standard_acp::translator::StandardAcpTranslator::new(
        standard_acp::profile_for("claude-code").unwrap(),
        "image-session",
    )
    .unwrap();
    translator.begin_prompt("image-prompt").unwrap();
    let events = translator
        .translate_notification_value(
            json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"image-session","update":{
                    "sessionUpdate":"agent_message_chunk","content":{
                        "type":"image","mimeType":"image/png","data":PNG
                    }
                }
            }}),
            &NoResources,
        )
        .unwrap();
    let payload = events
        .into_iter()
        .find_map(|event| match event {
            standard_acp::translator::AdapterEvent::Item { payload, .. } => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_reloaded_image(payload);
}

#[test]
fn codex_image_generation_survives_sqlite_chat_reload() {
    let item = json!({"type":"imageGeneration","id":"image-one","status":"completed",
        "result":format!("data:image/png;base64,{PNG}")});
    let (payload, tool_state, lifecycle) = codex::normalize_transcript_item(
        item.as_object().unwrap(),
        ItemLifecycle::Completed,
        &NoResources,
    )
    .unwrap()
    .unwrap();
    assert!(tool_state.is_none());
    assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
    assert_reloaded_image(payload);
}

fn assert_reloaded_image(payload: NormalizedPayload) {
    let NormalizedPayload::Image { data, .. } = &payload else {
        panic!("expected admitted image");
    };
    let expected_url = format!("data:image/png;base64,{}", data.as_str());
    let directory = TestDir::new();
    let path = directory.0.join("sage.db");
    let mut database = crate::acp::migration::open_or_migrate(&path).unwrap();
    let session = setup_image_turn(&mut database);
    let command = image_command(session, payload);
    let stored = apply_item_ingress(&mut database, command.clone()).unwrap();
    assert_eq!(
        apply_item_ingress(&mut database, command)
            .unwrap()
            .entity
            .disposition,
        MutationDisposition::Duplicate
    );
    drop(database);
    let reloaded = crate::acp::migration::open_or_migrate(&path).unwrap();
    let page = load_chat_page(
        &reloaded,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    page.validate_page_bounds().unwrap();
    let items = page.turns.as_slice()[0].items.as_slice();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, stored.item_id);
    let ItemContent::Image {
        media_type,
        data_url,
        source,
    } = &items[0].content
    else {
        panic!("expected reloaded image");
    };
    assert_eq!(*media_type, ImageMediaType::Png);
    assert_eq!(data_url.as_str(), expected_url);
    assert_eq!(*source, ImageSource::Agent);
}

#[test]
fn bounded_large_image_survives_sqlite_chat_reload() {
    let mut encoded = "AAAA".repeat(65_510);
    encoded.replace_range(..12, "iVBORw0KGgoA");
    let payload =
        crate::acp::image_admit::admit_image_parts(ImageMediaType::Png, &encoded).unwrap();
    let bytes = serde_json::to_vec(&payload).unwrap().len();
    assert!(bytes > 260_000);
    assert!(bytes <= crate::acp::limits::LIMITS.wire.normalized_item_bytes as usize);
    assert_reloaded_image(payload);
}

#[test]
fn sqlite_image_ingress_rejects_payload_over_normalized_item_cap() {
    let mut database =
        crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:")).unwrap();
    let session = setup_image_turn(&mut database);
    let payload = NormalizedPayload::Image {
        media_type: ImageMediaType::Png,
        data: SafeText::new("data", "AAAA".repeat(70_000)).unwrap(),
        source: ImageSource::Agent,
    };
    assert!(matches!(
        apply_item_ingress(&mut database, image_command(session, payload)),
        Err(StoreError::Corrupt("normalized item digest"))
    ));
    let page = load_chat_page(
        &database,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert!(page.turns.as_slice()[0].items.as_slice().is_empty());
}

fn image_command(session_id: SageAgentSessionId, payload: NormalizedPayload) -> ItemIngressCommand {
    let payload_json = serde_json::to_string(&payload).unwrap();
    ItemIngressCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id,
        turn_id: SageTurnId::parse("turn-image").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        external_item_id: crate::acp::intent::ExternalId::new("external", "image-one").unwrap(),
        activity_id: None,
        payload,
        payload_digest: super::super::facts::hex_digest(payload_json.as_bytes()),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        now_millis: 5,
    }
}

fn setup_image_turn(connection: &mut Connection) -> SageAgentSessionId {
    connection.execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Project',0,0,0);
         INSERT INTO project_roots VALUES('root-one','project-one','C:/project','c:/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,0);
         INSERT INTO acp_project_usage VALUES('project-one',1,0,0,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1 WHERE singleton=1;"
    ).unwrap();
    let started = start_session(connection, start(0, "start-image")).unwrap();
    activate_session(
        connection,
        ActivateSessionCommand {
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            expected_projection_revision: ProjectionRevision::new(1).unwrap(),
            client_intent_id: ClientIntentId::parse("activate-image").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "d".repeat(64),
            now_millis: 2,
        },
    )
    .unwrap();
    complete_activation(
        connection,
        CompleteActivationCommand {
            model_context: Default::default(),
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: crate::acp::intent::ExternalId::new("external", "image-session")
                .unwrap(),
            capabilities_json: capabilities(),
            now_millis: 3,
        },
    )
    .unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-image','project-one','chat-one',?1,1,0,'image-intent','running','correlated',0,4,4,NULL)",
        [started.session_id.as_str()]).unwrap();
    started.session_id
}
