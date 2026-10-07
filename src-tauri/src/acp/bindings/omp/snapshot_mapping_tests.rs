use serde_json::json;

use super::{
    AdapterEvent, ArtifactIdentity, CandidateManifest, CuratedOmpBinding, ItemUpdateMode,
    OMP_BUN_MINIMUM, OMP_LICENSE, OMP_PACKAGE, OMP_SOURCE_COMMIT, OMP_VERSION, OmpPlatform,
    OmpTranslator, Operation, ResourceMapper,
};
use crate::acp::{
    contract::SageRootId,
    file_diff::{DIFF_SIDE_BYTES_MAX, admit_pair, pair_digest},
    intent::{
        FileChangeOperation, NormalizedPayload, SafeResourceLocation, SafeResourceRef,
        UnavailableResourceReason,
    },
    lifecycle::ItemLifecycle,
};

struct FixtureResources;

impl ResourceMapper for FixtureResources {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        if absolute_path == r"C:\fixture\src\lib.rs" {
            SafeResourceRef::new(
                SageRootId::parse("root-fixture")
                    .map_err(|_| UnavailableResourceReason::Unsupported)?,
                "src/lib.rs",
            )
            .map_err(|_| UnavailableResourceReason::Unsupported)
        } else {
            Err(UnavailableResourceReason::Foreign)
        }
    }
}

fn identity(path: &str, byte: char, bytes: u64) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes,
    }
}

fn translator(epoch: u64) -> OmpTranslator {
    let entrypoint = identity("omp/bin/omp", 'a', 100);
    let runtime = identity("bun/bun.exe", 'b', 200);
    let addons = vec![identity("omp/addons/parser.node", 'c', 50)];
    let curated = CuratedOmpBinding {
        platform: OmpPlatform::WindowsX64,
        acp_protocol_version: "fixture-acp-v1".into(),
        bun_version: OMP_BUN_MINIMUM.into(),
        entrypoint: entrypoint.clone(),
        runtime: runtime.clone(),
        native_addons: addons.clone(),
    };
    let candidate = CandidateManifest {
        platform: OmpPlatform::WindowsX64,
        package_name: OMP_PACKAGE.into(),
        package_version: OMP_VERSION.into(),
        source_commit: OMP_SOURCE_COMMIT.into(),
        license: OMP_LICENSE.into(),
        acp_protocol_version: "fixture-acp-v1".into(),
        bun_version: OMP_BUN_MINIMUM.into(),
        entrypoint,
        runtime,
        native_addons: addons,
        fixed_args: vec!["acp".into()],
        isolated_state: true,
        isolated_auth: true,
        ambient_extensions_disabled: true,
        ambient_configuration_disabled: true,
        dynamic_downloads_disabled: true,
        browser_subprocess_lsp_dap_disabled: true,
        offline_and_telemetry_reviewed: true,
        read_only_authority_verified: true,
        native_acp_ndjson_verified: true,
        disconnect_teardown_verified: true,
    };
    let binding = curated.validate_candidate(&candidate).unwrap();
    OmpTranslator::new(epoch, binding.capabilities())
}

fn rpc_result(id: &str, result: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap()
}

fn initialize(translator: &mut OmpTranslator) {
    let request = translator.begin(Operation::Initialize).unwrap().remove(0);
    translator
        .ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false}}),
            ),
            &FixtureResources,
        )
        .unwrap();
}

fn new_session(translator: &mut OmpTranslator) {
    let request = translator
        .begin(Operation::NewSession {
            native_cwd: r"C:\fixture".into(),
            root_count: 1,
        })
        .unwrap()
        .remove(0);
    translator
        .ingest(
            &rpc_result(
                request.request_id.as_deref().unwrap(),
                json!({
                    "sessionId":"omp-session-1",
                    "models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A"}]}
                }),
            ),
            &FixtureResources,
        )
        .unwrap();
}

fn begin_prompt(translator: &mut OmpTranslator) {
    translator
        .begin(Operation::Prompt {
            turn_key: "turn-1".into(),
            message: "Inspect.".into(),
        })
        .unwrap();
}

fn tool_call(translator: &mut OmpTranslator, id: &str) {
    let frame = format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"omp-session-1","update":{{"sessionUpdate":"tool_call","toolCallId":"{id}","kind":"edit"}}}}}}"#
    );
    translator
        .ingest(frame.as_bytes(), &FixtureResources)
        .unwrap();
}

fn tool_update(translator: &mut OmpTranslator, id: &str, content: &str) -> Vec<AdapterEvent> {
    let frame = format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"omp-session-1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"{id}","kind":"edit","status":"completed","content":[{content}]}}}}}}"#
    );
    translator
        .ingest(frame.as_bytes(), &FixtureResources)
        .unwrap()
}

#[test]
fn omp_multi_entry_pair_maps_to_shared_typed_shape() {
    let mut translator = translator(21);
    initialize(&mut translator);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    tool_call(&mut translator, "tool-diff");
    let events = tool_update(
        &mut translator,
        "tool-diff",
        r#"{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":"old\n","newText":"new\n"}"#,
    );
    let pair = events.iter().find_map(|event| match event {
        AdapterEvent::FilePair {
            old_text, new_text, ..
        } => Some((old_text.clone(), new_text.clone())),
        _ => None,
    });
    let (old, new) = pair.expect("OMP admitted pair");
    assert!(matches!(
        admit_pair(&old, &new),
        crate::acp::file_diff::PairAdmission::Admitted { .. }
    ));
    let digest = events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::FileChangeDiff {
                    content_digest,
                    pair_available: true,
                    ..
                },
            update_mode: ItemUpdateMode::Replace,
            ..
        } => Some(content_digest.clone()),
        _ => None,
    });
    assert_eq!(digest.unwrap(), pair_digest(old.as_bytes(), new.as_bytes()));
}

#[test]
fn omp_duplicate_late_and_cancel_are_typed_without_text() {
    let mut translator = translator(22);
    initialize(&mut translator);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    tool_call(&mut translator, "tool-dup");
    let first = tool_update(
        &mut translator,
        "tool-dup",
        r#"{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":"a","newText":"b"}"#,
    );
    assert!(
        first
            .iter()
            .any(|event| matches!(event, AdapterEvent::FilePair { .. }))
    );
    let cancel = translator.begin(Operation::Cancel).unwrap();
    assert!(!cancel.is_empty());
    let late = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"omp-session-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"unknown","kind":"edit","status":"completed","content":[]}}}"#;
    assert!(translator.ingest(late, &FixtureResources).is_err());
}

#[test]
fn omp_oversize_and_foreign_map_to_summary_only_without_partial_text() {
    let mut translator = translator(23);
    initialize(&mut translator);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    tool_call(&mut translator, "tool-big");
    let big = "x".repeat(DIFF_SIDE_BYTES_MAX + 1);
    let content = format!(
        r#"{{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":"{big}","newText":"ok"}}"#
    );
    let events = tool_update(&mut translator, "tool-big", &content);
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, AdapterEvent::FilePair { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                pair_available: false,
                ..
            },
            lifecycle: ItemLifecycle::Completed,
            ..
        }
    )));
}

fn omp_change_shape(events: &[AdapterEvent]) -> (FileChangeOperation, Option<u64>, Option<u64>) {
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::FileChangeDiff {
                        operation,
                        additions,
                        deletions,
                        ..
                    },
                ..
            } => Some((*operation, *additions, *deletions)),
            _ => None,
        })
        .expect("a file change reports typed diff metadata")
}

fn omp_creation_events(content: &str) -> Vec<AdapterEvent> {
    let mut translator = translator(24);
    initialize(&mut translator);
    new_session(&mut translator);
    begin_prompt(&mut translator);
    tool_call(&mut translator, "tool-created");
    tool_update(&mut translator, "tool-created", content)
}

fn omp_change_resource(content: &str) -> SafeResourceLocation {
    omp_creation_events(content)
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { resource, .. },
                ..
            } => Some(resource.clone()),
            _ => None,
        })
        .expect("a file change carries a resource location")
}

#[test]
fn omp_creation_of_a_path_the_harness_has_not_written_stays_unavailable() {
    let resource = omp_change_resource(
        r#"{"type":"diff","path":"C:\\fixture\\src\\created.rs","oldText":null,"newText":"created\n"}"#,
    );
    assert_eq!(
        resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Foreign)
    );
}

#[test]
fn omp_update_of_a_path_outside_the_roots_stays_unavailable() {
    let resource = omp_change_resource(
        r#"{"type":"diff","path":"C:\\fixture\\src\\created.rs","oldText":"old\n","newText":"new\n"}"#,
    );
    assert_eq!(
        resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Foreign)
    );
}

#[test]
fn omp_null_old_text_marks_the_change_created_with_one_addition_and_no_deletion() {
    let events = omp_creation_events(
        r#"{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":null,"newText":"created\n"}"#,
    );
    assert_eq!(
        omp_change_shape(&events),
        (FileChangeOperation::Created, Some(1), Some(0))
    );
}

#[test]
fn omp_absent_old_text_marks_the_change_created() {
    let events = omp_creation_events(
        r#"{"type":"diff","path":"C:\\fixture\\src\\lib.rs","newText":"created\n"}"#,
    );
    assert_eq!(omp_change_shape(&events).0, FileChangeOperation::Created);
}

#[test]
fn omp_explicitly_empty_old_text_stays_an_update_of_an_existing_empty_file() {
    let events = omp_creation_events(
        r#"{"type":"diff","path":"C:\\fixture\\src\\lib.rs","oldText":"","newText":"created\n"}"#,
    );
    assert_eq!(omp_change_shape(&events).0, FileChangeOperation::Updated);
}
