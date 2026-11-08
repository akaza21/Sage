use serde_json::{Value, json};

use super::{CodexAccountSnapshot, CodexDiscoveryError, CodexDiscoveryProtocol, ProtocolProgress};
use crate::acp::{
    bindings::codex::{Platform, parse_packet_metadata},
    contract::RuntimeEpoch,
    transport::{Frame, IncrementalFrameDecoder},
};

fn protocol() -> CodexDiscoveryProtocol {
    let packet = parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .expect("pinned packet");
    CodexDiscoveryProtocol::new(&packet, [9; 32], RuntimeEpoch::new(7).unwrap())
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8 * 1024) {
        let candidate = decoder.push_chunk(chunk).unwrap();
        assert!(frame.is_none());
        frame = candidate;
    }
    frame.unwrap()
}

fn initialize_result(id: u64) -> Frame {
    frame(json!({"id":id,"result":{
        "userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) terminal (sage; 0.1.0)",
        "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
        "platformFamily":"windows",
        "platformOs":"windows"
    }}))
}

fn model(id: &str, name: &str, is_default: bool) -> Value {
    json!({
        "id": id,
        "model": id,
        "upgrade": null,
        "upgradeInfo": null,
        "availabilityNux": null,
        "displayName": name,
        "description": "Observed model",
        "hidden": false,
        "supportedReasoningEfforts": [],
        "defaultReasoningEffort": "medium",
        "inputModalities": ["text"],
        "supportsPersonality": false,
        "additionalSpeedTiers": [],
        "serviceTiers": [],
        "defaultServiceTier": null,
        "isDefault": is_default
    })
}

fn advance_to_account(protocol: &mut CodexDiscoveryProtocol) -> u64 {
    let initialize = protocol.initialize_request().unwrap();
    let initialize_id = initialize.id();
    let initialized = protocol
        .accept_initialize(&initialize_result(initialize_id))
        .unwrap();
    let value: Value = serde_json::from_slice(initialized.payload()).unwrap();
    assert_eq!(value, json!({"method":"initialized"}));
    protocol.account_request().unwrap().id()
}

#[test]
fn discovery_model_efforts_keep_strict_fields_and_collection_limit() {
    for efforts in [
        json!([{"reasoningEffort":"high","description":"Deep","extra":true}]),
        json!([{"reasoningEffort":"high","description":"Deep\0"}]),
        json!((0..33).map(|index| json!({"reasoningEffort":format!("effort-{index}"),"description":"Deep"})).collect::<Vec<_>>()),
    ] {
        let mut protocol = protocol();
        let account_id = advance_to_account(&mut protocol);
        protocol.accept_account(&frame(json!({"id":account_id,"result":{
            "account":{"type":"chatgpt","email":null,"planType":"plus"},"requiresOpenaiAuth":true
        }}))).unwrap();
        let request = protocol.model_request().unwrap();
        let mut entry = model("model", "Model", true);
        entry["supportedReasoningEfforts"] = efforts;
        assert_eq!(protocol.accept_model_page(&frame(json!({"id":request.id(),"result":{
            "data":[entry],"nextCursor":null
        }}))).unwrap_err(), CodexDiscoveryError::ModelCatalog);
    }
}

#[test]
fn discovery_requests_follow_the_pinned_handshake_and_page_32_contract() {
    let mut protocol = protocol();
    let initialize = protocol.initialize_request().unwrap();
    let initialize_value: Value = serde_json::from_slice(initialize.payload()).unwrap();
    assert_eq!(initialize_value["method"], "initialize");
    assert_eq!(initialize_value["params"]["clientInfo"]["name"], "sage");
    let initialized = protocol
        .accept_initialize(&initialize_result(initialize.id()))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(initialized.payload()).unwrap(),
        json!({"method":"initialized"})
    );

    let account = protocol.account_request().unwrap();
    let account_value: Value = serde_json::from_slice(account.payload()).unwrap();
    assert_eq!(account_value["method"], "account/read");
    // rust-v0.146.0 authority:
    // app-server-protocol/src/protocol/v2/account.rs:484-491 defines the
    // default-false flag, and protocol/common.rs:2942-2955 proves that false
    // serializes as an empty params object rather than `refreshToken:false`.
    assert_eq!(account_value["params"], json!({}));
    protocol
        .accept_account(&frame(json!({"id":account.id(),"result":{
            "account":{"type":"chatgpt","email":"private@example.test","planType":"plus"},
            "requiresOpenaiAuth":true
        }})))
        .unwrap();

    let models = protocol.model_request().unwrap();
    let models_value: Value = serde_json::from_slice(models.payload()).unwrap();
    assert_eq!(models_value["method"], "model/list");
    assert_eq!(
        models_value["params"],
        json!({"cursor":null,"limit":32,"includeHidden":false})
    );
    assert_eq!(protocol.platform(), Platform::WindowsX86_64);
}

#[test]
fn account_snapshot_distinguishes_anonymous_states_without_identity_data() {
    let mut signed_in = protocol();
    let account_id = advance_to_account(&mut signed_in);
    let account = signed_in
        .accept_account(&frame(json!({"id":account_id,"result":{
            "account":{"type":"chatgpt","email":"private@example.test","planType":"enterprise"},
            "requiresOpenaiAuth":true
        }})))
        .unwrap();
    assert_eq!(account, CodexAccountSnapshot::ManagedChatgpt);
    let serialized = serde_json::to_string(&account).unwrap();
    assert!(!serialized.contains("private@example.test"));
    assert!(!serialized.contains("enterprise"));

    let mut signed_out = protocol();
    let account_id = advance_to_account(&mut signed_out);
    assert_eq!(
        signed_out
            .accept_account(&frame(json!({"id":account_id,"result":{
                "account":null,"requiresOpenaiAuth":true
            }})))
            .unwrap(),
        CodexAccountSnapshot::SignedOut
    );

    let mut provider_managed = protocol();
    let account_id = advance_to_account(&mut provider_managed);
    assert_eq!(
        provider_managed
            .accept_account(&frame(json!({"id":account_id,"result":{
                "account":null,"requiresOpenaiAuth":false
            }})))
            .unwrap(),
        CodexAccountSnapshot::ProviderManaged
    );
    let model_request = provider_managed.model_request().unwrap();
    let model_request: Value = serde_json::from_slice(model_request.payload()).unwrap();
    assert_eq!(model_request["method"], "model/list");

    let mut api_key = protocol();
    let account_id = advance_to_account(&mut api_key);
    assert_eq!(
        api_key
            .accept_account(&frame(json!({"id":account_id,"result":{
                "account":{"type":"apiKey"},"requiresOpenaiAuth":true
            }})))
            .unwrap(),
        CodexAccountSnapshot::ApiKey
    );

    for rejected in [
        json!({"account":{"type":"apiKey","key":"never-retain"},"requiresOpenaiAuth":true}),
        json!({"account":{"type":"amazonBedrock"},"requiresOpenaiAuth":true}),
    ] {
        let mut candidate = protocol();
        let account_id = advance_to_account(&mut candidate);
        assert_eq!(
            candidate
                .accept_account(&frame(json!({"id":account_id,"result":rejected})))
                .unwrap_err(),
            CodexDiscoveryError::AccountPolicy
        );
    }

    let mut extended = protocol();
    let account_id = advance_to_account(&mut extended);
    assert_eq!(
        extended
            .accept_account(&frame(json!({"id":account_id,"result":{
                "account":{"type":"chatgpt","email":"private@example.test","planType":"plus","token":"never-retain"},
                "requiresOpenaiAuth":true
            }})))
            .unwrap_err(),
        CodexDiscoveryError::Protocol
    );
}

#[test]
fn model_pages_preserve_observed_order_and_default_only_after_a_complete_catalog() {
    let mut protocol = protocol();
    let account_id = advance_to_account(&mut protocol);
    protocol
        .accept_account(&frame(json!({"id":account_id,"result":{
            "account":{"type":"chatgpt","email":null,"planType":"plus"},
            "requiresOpenaiAuth":true
        }})))
        .unwrap();
    let first = protocol.model_request().unwrap();
    assert!(matches!(
        protocol
            .accept_model_page(&frame(json!({"id":first.id(),"result":{
                "data":[model("model-b","Model B",false)],"nextCursor":"next"
            }})))
            .unwrap(),
        ProtocolProgress::More
    ));
    let second = protocol.model_request().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(second.payload()).unwrap()["params"]["cursor"],
        "next"
    );
    let ProtocolProgress::Complete(snapshot) = protocol
        .accept_model_page(&frame(json!({"id":second.id(),"result":{
            "data":[model("model-a","Model A",true)],"nextCursor":null
        }})))
        .unwrap()
    else {
        panic!("catalog should complete");
    };
    assert_eq!(
        snapshot
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["model-b", "model-a"]
    );
    assert_eq!(
        snapshot.default_model_id.as_ref().unwrap().as_str(),
        "model-a"
    );
}

#[test]
fn model_catalog_rejects_modalities_outside_the_pinned_enum() {
    let mut protocol = protocol();
    let account_id = advance_to_account(&mut protocol);
    protocol
        .accept_account(&frame(json!({"id":account_id,"result":{
            "account":{"type":"chatgpt","email":null,"planType":"plus"},
            "requiresOpenaiAuth":true
        }})))
        .unwrap();
    let request = protocol.model_request().unwrap();
    let mut invalid = model("future-model", "Future Model", false);
    invalid["inputModalities"] = json!(["text", "video"]);

    assert_eq!(
        protocol
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[invalid],"nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::ModelCatalog
    );
}

#[test]
fn model_catalog_rejects_page_count_identity_default_hidden_and_empty_violations() {
    fn ready() -> CodexDiscoveryProtocol {
        let mut protocol = protocol();
        let account_id = advance_to_account(&mut protocol);
        protocol
            .accept_account(&frame(json!({"id":account_id,"result":{
                "account":{"type":"chatgpt","email":null,"planType":"plus"},
                "requiresOpenaiAuth":true
            }})))
            .unwrap();
        protocol
    }

    let mut empty = ready();
    let request = empty.model_request().unwrap();
    assert_eq!(
        empty
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[],"nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::EmptyCatalog
    );

    for malformed_result in [
        json!({"data":[model("missing-cursor","Missing cursor",false)]}),
        json!({"data":[model("extra-field","Extra field",false)],"nextCursor":null,"raw":"never-retain"}),
    ] {
        let mut exact = ready();
        let request = exact.model_request().unwrap();
        assert_eq!(
            exact
                .accept_model_page(&frame(json!({
                    "id":request.id(),"result":malformed_result
                })))
                .unwrap_err(),
            CodexDiscoveryError::Protocol
        );
    }

    let mut hidden = ready();
    let request = hidden.model_request().unwrap();
    let mut hidden_model = model("hidden", "Hidden", false);
    hidden_model["hidden"] = json!(true);
    assert_eq!(
        hidden
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[hidden_model],"nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::ModelCatalog
    );

    let mut oversized = ready();
    let request = oversized.model_request().unwrap();
    let mut oversized_model = model("oversized", "Oversized", false);
    oversized_model["description"] = json!("x".repeat(8_193));
    assert_eq!(
        oversized
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[oversized_model],"nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::ModelCatalog
    );

    let mut duplicate = ready();
    let first = duplicate.model_request().unwrap();
    duplicate
        .accept_model_page(&frame(json!({"id":first.id(),"result":{
            "data":[model("same","First",true)],"nextCursor":"again"
        }})))
        .unwrap();
    let second = duplicate.model_request().unwrap();
    assert_eq!(
        duplicate
            .accept_model_page(&frame(json!({"id":second.id(),"result":{
                "data":[model("same","Second",false)],"nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::ModelCatalog
    );

    let mut defaults = ready();
    let request = defaults.model_request().unwrap();
    assert_eq!(
        defaults
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[model("one","One",true),model("two","Two",true)],
                "nextCursor":null
            }})))
            .unwrap_err(),
        CodexDiscoveryError::ModelCatalog
    );

    let mut looped = ready();
    let request = looped.model_request().unwrap();
    looped
        .accept_model_page(&frame(json!({"id":request.id(),"result":{
            "data":[model("one","One",false)],"nextCursor":"cursor-a"
        }})))
        .unwrap();
    let request = looped.model_request().unwrap();
    assert_eq!(
        looped
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[model("two","Two",false)],"nextCursor":"cursor-a"
            }})))
            .unwrap_err(),
        CodexDiscoveryError::CursorLoop
    );

    let mut fifth = ready();
    for page in 0..4 {
        let request = fifth.model_request().unwrap();
        let next = format!("cursor-{page}");
        let result = fifth.accept_model_page(&frame(json!({"id":request.id(),"result":{
            "data":[model(&format!("model-{page}"),"Model",false)],
            "nextCursor":next
        }})));
        if page < 3 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err(), CodexDiscoveryError::PageLimit);
        }
    }
}

#[test]
fn unknown_response_server_request_rpc_error_and_wrong_id_fail_closed() {
    for value in [
        json!({"method":"future/event","params":{}}),
        json!({"id":"server-1","method":"future/request","params":{}}),
        json!({"id":1,"error":{"code":-1,"message":"private C:\\path"}}),
        json!({"id":99,"result":{}}),
    ] {
        let mut protocol = protocol();
        protocol.initialize_request().unwrap();
        assert_eq!(
            protocol.accept_initialize(&frame(value)).err().unwrap(),
            CodexDiscoveryError::Protocol
        );
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires an installed, signed-in Codex CLI"]
fn exact_pinned_binary_discovery_hook_proves_bounded_zero_process_cleanup() {
    use crate::{
        acp::{
            bindings::codex::{
                CodexBindingRegistration, CodexDiscoveryDeadline, CodexProjectRootAuthority,
                discover_admitted,
            },
            contract::{ProjectGeneration, RuntimeEpoch, SageProjectId, SageRootId},
        },
        project_registry::ProjectRootAuthority,
    };

    let registration = CodexBindingRegistration::admitted_for_test();
    let binding = registration.refresh().expect("exact pinned test admission");
    let root = std::env::current_dir().expect("test checkout");
    let authority = CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            SageProjectId::parse("project-exact-discovery").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-exact-discovery").unwrap(),
            &root,
        )
        .unwrap(),
    );
    let discovery = discover_admitted(
        binding,
        authority,
        RuntimeEpoch::new(9_001).unwrap(),
        CodexDiscoveryDeadline::new().unwrap(),
        |_| Ok(()),
    )
    .expect("exact binary discovery and zero-process teardown");

    assert!(!discovery.content.models.is_empty());
    discovery.revalidate().expect("authority remains current");
}

/// What the composer's model menu shows for the desktop restart replay: Sage
/// reads the case's own `model/list` frame with its real discovery protocol
/// and keeps every model the session lists.
#[cfg(feature = "acp-fixture")]
#[test]
fn a_replay_case_offers_every_model_its_session_lists() {
    let case = "codex-config-resume";
    let reply = |payload: &[u8]| {
        let request: Value = serde_json::from_slice(payload).unwrap();
        frame(crate::acp::fixture::native_case_discovery_reply(case, &request).unwrap())
    };
    let mut protocol = protocol();
    let initialize = protocol.initialize_request().unwrap();
    // The desktop replay is checked as an observed build, which accepts any
    // absolute Codex home; this protocol is the pinned packet's, which
    // expects its own.
    let mut initialized = crate::acp::fixture::native_case_discovery_reply(
        case,
        &serde_json::from_slice(initialize.payload()).unwrap(),
    )
    .unwrap();
    initialized["result"]["codexHome"] = parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap()
    .codex_home()
    .to_string_lossy()
    .into_owned()
    .into();
    protocol.accept_initialize(&frame(initialized)).unwrap();
    let account = protocol.account_request().unwrap();
    let models = protocol.model_request().unwrap();
    assert_eq!(
        protocol.accept_account(&reply(account.payload())).unwrap(),
        CodexAccountSnapshot::ProviderManaged
    );
    let ProtocolProgress::Complete(content) = protocol
        .accept_model_page(&reply(models.payload()))
        .unwrap()
    else {
        panic!("one page lists the whole catalogue");
    };
    let listed: Vec<_> = content
        .models
        .iter()
        .map(|model| (model.id.as_str(), model.name.as_str(), model.is_default))
        .collect();
    assert_eq!(
        listed,
        [
            ("fixture-model", "Synthetic replay", true),
            ("fixture-model-deep", "Synthetic replay deep", false)
        ]
    );
}
