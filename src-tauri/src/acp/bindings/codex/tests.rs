use serde_json::{Map, Value, json};

use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageRootId},
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
    lifecycle::{ServerRequestState, TurnState},
};

use super::hard_failure_tests::with_captured_diagnostics;
use super::incoming::UNPROJECTED_DIAGNOSTIC;
use super::*;
use super::{incoming::IngressError, request::RequestKind, state::AdapterError};

mod file_change_sequence_tests;
mod terminal_error_tests;

struct Mapper;

const PACKET_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/candidate-windows.json");
const SECURITY_POLICY_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/security-policy-v1.json");
const CONFIG_FIXTURE: &[u8] = include_bytes!("../../../../testdata/acp/codex/config-v1.toml");
const REPRODUCTION_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/package-reproduction-v1.json");
const PACKAGE_ROOT: &str = r"C:\SageHarnesses\Codex\0.146.0\package";
const ISOLATED_CODEX_HOME: &str = r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec";

#[test]
fn native_0146_service_gates_admit_only_exact_managed_or_observed_identity() {
    assert!(super::supports_native_0146_protocol(
        "codex-app-server-0.146.0-d3992fec"
    ));
    assert!(super::supports_native_0146_protocol(
        "codex-app-server-observed-0.146.0"
    ));
    for unknown in [
        "codex-app-server-0.145.0-7b0ab167",
        "codex-app-server-observed-0.145.0",
        "codex-app-server-observed-0.147.0",
        "codex-app-server-0.146.0-unknown",
    ] {
        assert!(!super::supports_native_0146_protocol(unknown), "{unknown}");
    }
}

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        (absolute_path == r"C:\repo\src\lib.rs")
            .then(|| {
                SafeResourceRef::new(SageRootId::parse("root-one").unwrap(), "src/lib.rs").unwrap()
            })
            .ok_or(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
}

fn adapter() -> CodexAdapter {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    CodexAdapter::from_metadata_packet(
        &packet,
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("session-one").unwrap(),
    )
}

fn packet_value() -> Value {
    serde_json::from_slice(PACKET_FIXTURE).unwrap()
}

fn security_policy_value() -> Value {
    serde_json::from_slice(SECURITY_POLICY_FIXTURE).unwrap()
}

fn reproduction_value() -> Value {
    serde_json::from_slice(REPRODUCTION_FIXTURE).unwrap()
}

fn relocate_package_root(packet: &mut Value, next_root: &str) {
    let replace = |path: &mut Value| {
        let current = path.as_str().unwrap();
        *path = json!(current.replacen(PACKAGE_ROOT, next_root, 1));
    };
    replace(&mut packet["entrypoint"]["path"]);
    for artifact in packet["supportingArtifacts"].as_array_mut().unwrap() {
        replace(&mut artifact["path"]);
    }
}

fn relocate_codex_home(packet: &mut Value, next_home: &str) {
    packet["codexHome"] = json!(next_home);
}

fn initialize_result(id: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": id,
        "result": {
            "userAgent": "sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
            "codexHome": "C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
            "platformFamily": "windows",
            "platformOs": "windows"
        }
    }))
    .unwrap()
}

fn negotiate(adapter: &mut CodexAdapter) {
    let request = adapter.initialize(900).unwrap();
    assert!(!request.bytes.ends_with(b"\n"));
    let IngressDisposition::Events(events) =
        adapter.ingest(&initialize_result(900), &Mapper).unwrap()
    else {
        panic!()
    };
    assert!(matches!(events[0], IncomingEvent::Initialized { .. }));
    assert!(!adapter.initialized().unwrap().ends_with(b"\n"));
}

fn ready(adapter: &mut CodexAdapter) {
    negotiate(adapter);
    adapter
        .start(1, &adapter.project_root(r"C:\repo".into()).unwrap(), None)
        .unwrap();
    let response = br#"{"id":1,"result":{"thread":{"id":"thread-1"}}}"#;
    assert!(matches!(
        adapter.ingest(response, &Mapper).unwrap(),
        IngressDisposition::Events(_)
    ));
}

fn observed_goal_ready(adapter: &mut CodexAdapter) {
    adapter.enable_observed_goal_for_test();
    adapter.initialize(900).unwrap();
    adapter.ingest(
        br#"{"id":900,"result":{"userAgent":"Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64) dumb (sage; 0.1.0)","codexHome":"C:\\Users\\test\\.codex","platformFamily":"windows","platformOs":"windows"}}"#,
        &Mapper,
    ).unwrap();
    adapter
        .start(1, &adapter.project_root(r"C:\repo".into()).unwrap(), None)
        .unwrap();
    adapter
        .ingest(
            br#"{"id":1,"result":{"thread":{"id":"thread-1"}}}"#,
            &Mapper,
        )
        .unwrap();
}

#[test]
fn managed_0146_sends_goal_get_after_session_start() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let request = adapter.goal_get(44).unwrap();
    let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
    assert_eq!(wire["method"], "thread/goal/get");
    assert_eq!(wire["params"]["threadId"], "thread-1");
    assert_eq!(adapter.goal_state().availability(), "loading");
}

#[test]
fn only_observed_existing_goal_can_emit_a_paused_status_mutation() {
    let mut managed = adapter();
    ready(&mut managed);
    assert!(managed.goal_pause(60).is_err());

    let mut observed = adapter();
    observed_goal_ready(&mut observed);
    assert!(observed.goal_pause(60).is_err());
    observed.goal_updated(&serde_json::json!({
        "objective":"Keep the native goal","status":"active","tokensUsed":3,"timeUsedSeconds":5
    }));
    let request = observed.goal_pause(60).unwrap();
    let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
    assert_eq!(wire["method"], "thread/goal/set");
    assert_eq!(
        wire["params"],
        serde_json::json!({"threadId":"thread-1","status":"paused"})
    );
    assert!(!observed.has_active_turn());
}

#[test]
fn clear_requires_an_observed_existing_goal_and_projects_only_confirmed_clear() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    assert!(adapter.goal_clear(61).is_err());
    adapter.goal_updated(&json!({
        "objective":"Keep the native goal","status":"active","tokensUsed":3,"timeUsedSeconds":5
    }));
    let request = adapter.goal_clear(61).unwrap();
    let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
    assert_eq!(wire["method"], "thread/goal/clear");
    assert_eq!(wire["params"], json!({"threadId":"thread-1"}));
    assert!(adapter.goal_clear(62).is_err());
    let refusal = adapter
        .ingest(br#"{"id":61,"result":{"cleared":false}}"#, &Mapper)
        .unwrap();
    assert_eq!(refusal, IngressDisposition::GoalMutationRejected);
    assert_eq!(
        adapter.goal_state().objective(),
        Some("Keep the native goal")
    );
    adapter.goal_clear(63).unwrap();
    let accepted = adapter
        .ingest(br#"{"id":63,"result":{"cleared":true}}"#, &Mapper)
        .unwrap();
    assert_eq!(accepted, IngressDisposition::GoalMutationApplied);
    assert_eq!(adapter.goal_state().availability(), "noGoal");
}

#[test]
fn newer_goal_notification_wins_over_late_pause_reply_and_refusal_is_nonfatal() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_updated(&json!({
        "objective":"Original","status":"active","tokensUsed":3,"timeUsedSeconds":5
    }));
    adapter.goal_pause(64).unwrap();
    adapter.goal_updated(&json!({
        "objective":"Newer","status":"blocked","tokensUsed":4,"timeUsedSeconds":6
    }));
    adapter.ingest(br#"{"id":64,"result":{"goal":{"objective":"Original","status":"paused","tokensUsed":3,"timeUsedSeconds":5}}}"#, &Mapper).unwrap();
    assert_eq!(adapter.goal_state().objective(), Some("Newer"));
    adapter.goal_pause(65).unwrap();
    let refusal = adapter
        .ingest(
            br#"{"id":65,"error":{"code":-1,"message":"denied"}}"#,
            &Mapper,
        )
        .unwrap();
    assert_eq!(refusal, IngressDisposition::GoalMutationRejected);
    assert_eq!(adapter.goal_state().objective(), Some("Newer"));
}

#[test]
fn malformed_pause_reply_is_refused_without_changing_the_observed_goal() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_updated(&json!({
        "objective":"Original","status":"active","tokensUsed":3,"timeUsedSeconds":5
    }));
    adapter.goal_pause(66).unwrap();
    let refusal = adapter.ingest(br#"{"id":66,"result":{"goal":{"objective":"Original","status":"active","tokensUsed":3,"timeUsedSeconds":5}}}"#, &Mapper).unwrap();
    assert_eq!(refusal, IngressDisposition::GoalMutationRejected);
    assert_eq!(adapter.goal_state().objective(), Some("Original"));
}

#[test]
fn newer_goal_notification_wins_over_late_get_response() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    let request = adapter.goal_get(44).expect("observed goal get");
    let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
    assert_eq!(wire["method"], "thread/goal/get");
    assert_eq!(wire["params"]["threadId"], "thread-1");
    adapter
        .ingest(
            br#"{"method":"thread/goal/updated","params":{"threadId":"thread-1","goal":{"objective":"newer goal","status":"active","tokensUsed":3,"timeUsedSeconds":5}}}"#,
            &Mapper,
        )
        .unwrap();
    adapter
        .ingest(
            br#"{"id":44,"result":{"goal":{"objective":"older goal","status":"paused","tokensUsed":1,"timeUsedSeconds":2}}}"#,
            &Mapper,
        )
        .unwrap();
    assert_eq!(adapter.goal_state().objective(), Some("newer goal"));
}

#[test]
fn goal_clear_notification_wins_over_late_get_response() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_get(45).unwrap();
    adapter
        .ingest(
            br#"{"method":"thread/goal/cleared","params":{"threadId":"thread-1"}}"#,
            &Mapper,
        )
        .unwrap();
    adapter.ingest(
        br#"{"id":45,"result":{"goal":{"objective":"stale","status":"active","tokensUsed":1,"timeUsedSeconds":2}}}"#,
        &Mapper,
    ).unwrap();
    assert_eq!(adapter.goal_state().availability(), "noGoal");
}

#[test]
fn goal_get_null_hydrates_no_goal_without_creating_a_turn() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_get(46).unwrap();
    adapter
        .ingest(br#"{"id":46,"result":{"goal":null}}"#, &Mapper)
        .unwrap();
    assert_eq!(adapter.goal_state().availability(), "noGoal");
    assert!(!adapter.has_active_turn());
}

#[test]
fn malformed_native_goal_is_unavailable_without_tearing_down_the_chat() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_get(47).unwrap();
    assert!(
        adapter
            .ingest(br#"{"id":47,"result":{"goal":{"objective":""}}}"#, &Mapper)
            .is_ok()
    );
    assert_eq!(adapter.goal_state().availability(), "unavailable");
    assert!(
        adapter
            .ingest(
                br#"{"method":"thread/goal/updated","params":{"threadId":"thread-1","goal":{}}}"#,
                &Mapper
            )
            .is_ok()
    );
    assert_eq!(adapter.goal_state().availability(), "unavailable");
}

#[test]
fn malformed_goal_get_envelope_is_unavailable_without_tearing_down_the_chat() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_get(48).unwrap();
    assert!(
        adapter
            .ingest(br#"{"id":48,"result":null}"#, &Mapper)
            .is_ok()
    );
    assert_eq!(adapter.goal_state().availability(), "unavailable");
    assert!(!adapter.has_active_turn());
}

#[test]
fn unsafe_goal_counters_do_not_poison_the_chat_projection() {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    adapter.goal_get(49).unwrap();
    assert!(adapter
        .ingest(br#"{"id":49,"result":{"goal":{"objective":"counting","status":"active","tokensUsed":9007199254740992,"timeUsedSeconds":0}}}"#, &Mapper)
        .is_ok());
    assert_eq!(adapter.goal_state().availability(), "unavailable");
}

fn arm_turn(adapter: &mut CodexAdapter, request_id: u64, client_id: &str) {
    let prompt = Prompt::new("prompt", "inspect").unwrap();
    let client = RequiredText::new("client", client_id).unwrap();
    adapter.prompt(request_id, &prompt, &client).unwrap();
}

fn goal_ready_after_previous_turn() -> CodexAdapter {
    let mut adapter = adapter();
    observed_goal_ready(&mut adapter);
    arm_turn(&mut adapter, 50, "first-client");
    adapter
        .ingest(
            br#"{"id":50,"result":{"turn":{"id":"turn-previous"}}}"#,
            &Mapper,
        )
        .unwrap();
    adapter
        .ingest(br#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-previous","status":"completed"}}}"#, &Mapper)
        .unwrap();
    adapter.goal_updated(&serde_json::json!({
        "objective":"Continue bounded work","status":"active","tokensUsed":0,"timeUsedSeconds":0
    }));
    arm_turn(&mut adapter, 51, "second-client");
    adapter
}

#[test]
fn goal_provisional_turn_rejects_reuse_of_the_previous_native_turn_id() {
    let mut response_collision = goal_ready_after_previous_turn();
    assert!(matches!(
        response_collision.ingest(
            br#"{"id":51,"result":{"turn":{"id":"turn-previous"}}}"#,
            &Mapper,
        ),
        Err(AdapterError::Ingress(IngressError::Identity))
    ));

    let mut started_collision = goal_ready_after_previous_turn();
    started_collision
        .ingest(br#"{"id":51,"result":{"turn":{"id":"turn-new"}}}"#, &Mapper)
        .unwrap();
    assert!(matches!(
        started_collision.ingest(
            br#"{"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-previous"}}}"#,
            &Mapper,
        ),
        Err(AdapterError::Ingress(IngressError::Identity))
    ));
}

#[test]
fn lexically_validates_only_the_exact_windows_packet_and_keeps_launch_fail_closed() {
    let bytes = PACKET_FIXTURE;
    let packet = parse_packet_metadata(bytes).unwrap();
    assert_eq!(packet.metadata().binding_id, "codex-app-server");
    assert_eq!(packet.metadata().codex_runtime_version, "0.146.0");
    assert_eq!(
        packet.metadata().codex_source_commit,
        "e363b08c9175ac1cbe5893615dd2cb9ddf95043b"
    );
    assert_eq!(
        packet.metadata().app_server_schema_sha256,
        "d3992fec1398afdbec658da2c720c6993fbf3c1ce4900785694d2196679eddfc"
    );
    assert_eq!(packet.metadata().supporting_artifacts.len(), 7);
    let manifest_source = include_str!("manifest.rs");
    assert!(manifest_source.contains("verify_installed_authenticode"));
    assert!(manifest_source.contains("admit_installed_role"));
    assert!(manifest_source.contains("WinVerifyTrustCatalog"));
    let mut missing_artifact = packet_value();
    relocate_package_root(
        &mut missing_artifact,
        r"C:\SageHarnesses\Codex\missing-artifact-package",
    );
    let missing_artifact = parse_packet_metadata(&serde_json::to_vec(&missing_artifact).unwrap())
        .expect("a packet may name an absent package before physical admission");
    assert_eq!(
        missing_artifact.require_physical_admission(),
        Err(PhysicalAdmissionUnavailable::ArtifactNotAdmitted)
    );

    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    value["unexpected"] = json!(true);
    assert!(parse_packet_metadata(&serde_json::to_vec(&value).unwrap()).is_err());
    value.as_object_mut().unwrap().remove("unexpected");
    value["entrypoint"]["path"] = json!(r"codex.exe");
    assert!(parse_packet_metadata(&serde_json::to_vec(&value).unwrap()).is_err());

    for (pointer, replacement) in [
        ("/codexRuntimeVersion", json!("0.145.1")),
        ("/appServerSchemaSha256", json!("a".repeat(64))),
        ("/supportingArtifacts/0/sha256", json!("b".repeat(64))),
        ("/capabilityCeiling/prompt", json!(false)),
        ("/codexHome", json!(r"C:\Users\Username\.codex")),
    ] {
        let mut changed: Value = serde_json::from_slice(bytes).unwrap();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert!(parse_packet_metadata(&serde_json::to_vec(&changed).unwrap()).is_err());
    }

    let mut incomplete: Value = serde_json::from_slice(bytes).unwrap();
    incomplete["supportingArtifacts"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(parse_packet_metadata(&serde_json::to_vec(&incomplete).unwrap()).is_err());

    let mut oversized_home: Value = serde_json::from_slice(bytes).unwrap();
    oversized_home["codexHome"] = json!(format!(
        "C:\\{}\\CodexState\\codex-app-server-0.146.0-d3992fec",
        "a".repeat(32 * 1024)
    ));
    assert!(parse_packet_metadata(&serde_json::to_vec(&oversized_home).unwrap()).is_err());

    let mut mac: Value = serde_json::from_slice(bytes).unwrap();
    mac["platform"] = json!("macosAarch64");
    assert!(parse_packet_metadata(&serde_json::to_vec(&mac).unwrap()).is_err());
}

#[test]
fn packet_metadata_rejects_a_supporting_artifact_outside_the_single_package_root() {
    let mut packet = packet_value();
    let path = packet["supportingArtifacts"][0]["path"]
        .as_str()
        .unwrap()
        .replacen(PACKAGE_ROOT, r"D:\Foreign\Codex\package", 1);
    packet["supportingArtifacts"][0]["path"] = json!(path);
    assert!(parse_packet_metadata(&serde_json::to_vec(&packet).unwrap()).is_err());
}

#[test]
fn packet_metadata_rejects_equal_nested_and_parent_codex_home_overlaps() {
    let mut equal = packet_value();
    relocate_package_root(&mut equal, ISOLATED_CODEX_HOME);
    equal["codexHome"] = json!(ISOLATED_CODEX_HOME);
    assert!(parse_packet_metadata(&serde_json::to_vec(&equal).unwrap()).is_err());

    let mut home_inside_package = packet_value();
    home_inside_package["codexHome"] = json!(format!(
        r"{PACKAGE_ROOT}\CodexState\codex-app-server-0.146.0-d3992fec"
    ));
    assert!(parse_packet_metadata(&serde_json::to_vec(&home_inside_package).unwrap()).is_err());

    let mut package_inside_home = packet_value();
    relocate_package_root(
        &mut package_inside_home,
        &format!(r"{ISOLATED_CODEX_HOME}\package"),
    );
    package_inside_home["codexHome"] = json!(ISOLATED_CODEX_HOME);
    assert!(parse_packet_metadata(&serde_json::to_vec(&package_inside_home).unwrap()).is_err());
}

#[test]
fn security_policy_accepts_only_the_pinned_isolated_state_and_environment() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();

    for (pointer, replacement, error) in [
        (
            "/codexHome",
            json!(r"C:\Users\Username\.codex"),
            SecurityPolicyError::Identity,
        ),
        (
            "/state/inheritAmbientConfig",
            json!(true),
            SecurityPolicyError::State,
        ),
        (
            "/state/maximumLiveProcessTrees",
            json!(2),
            SecurityPolicyError::State,
        ),
        (
            "/environment/inheritAmbient",
            json!(true),
            SecurityPolicyError::Environment,
        ),
        (
            "/environment/maximumEntries",
            json!(7),
            SecurityPolicyError::Environment,
        ),
        (
            "/requestPolicy/turnStart/sandboxPolicy/networkAccess",
            json!(true),
            SecurityPolicyError::Request,
        ),
    ] {
        let mut changed = security_policy_value();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            parse_security_policy(&serde_json::to_vec(&changed).unwrap(), &packet),
            Err(error),
            "pointer {pointer}"
        );
    }

    let mut ambient_secret = security_policy_value();
    ambient_secret["environment"]["allowedSecretKeys"] = json!(["OPENAI_API_KEY"]);
    assert_eq!(
        parse_security_policy(&serde_json::to_vec(&ambient_secret).unwrap(), &packet),
        Err(SecurityPolicyError::Environment)
    );

    let mut allowlist_drift = security_policy_value();
    allowlist_drift["environment"]["allowedKeys"]
        .as_array_mut()
        .unwrap()
        .push(json!("PATH"));
    assert_eq!(
        parse_security_policy(&serde_json::to_vec(&allowlist_drift).unwrap(), &packet),
        Err(SecurityPolicyError::Environment)
    );
}

#[test]
fn security_policy_rejects_malformed_unknown_and_exact_plus_one_bytes() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    assert_eq!(
        parse_security_policy(b"{", &packet),
        Err(SecurityPolicyError::Malformed)
    );

    let mut unknown = security_policy_value();
    unknown["endpointPolicy"] = json!({"status": "unresolved"});
    assert_eq!(
        parse_security_policy(&serde_json::to_vec(&unknown).unwrap(), &packet),
        Err(SecurityPolicyError::Malformed)
    );

    assert_eq!(
        parse_security_policy(&vec![b' '; policy::SECURITY_POLICY_BYTES + 1], &packet),
        Err(SecurityPolicyError::PolicyBytes)
    );
}

#[test]
fn security_policy_freezes_auth_storage_network_and_retention_stop_limits() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();

    for (pointer, replacement, error) in [
        (
            "/auth/loginMode",
            json!("apiKey"),
            SecurityPolicyError::Auth,
        ),
        (
            "/auth/automaticLoginStart",
            json!(true),
            SecurityPolicyError::Auth,
        ),
        (
            "/storage/maximumAllocatedBytes",
            json!(536_870_913_u64),
            SecurityPolicyError::Storage,
        ),
        (
            "/storage/maximumRegularFiles",
            json!(2_049),
            SecurityPolicyError::Storage,
        ),
        (
            "/storage/temporaryResidueRetentionHours",
            json!(25),
            SecurityPolicyError::Storage,
        ),
        (
            "/network/customEndpointsAllowed",
            json!(true),
            SecurityPolicyError::Network,
        ),
        (
            "/network/proxy",
            json!("system"),
            SecurityPolicyError::Network,
        ),
        (
            "/network/offline",
            json!(true),
            SecurityPolicyError::Network,
        ),
        (
            "/network/otelTraceExporter",
            json!("otlpHttp"),
            SecurityPolicyError::Network,
        ),
        (
            "/consent/deleteDataRequiresExplicitConsent",
            json!(false),
            SecurityPolicyError::Consent,
        ),
    ] {
        let mut changed = security_policy_value();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            parse_security_policy(&serde_json::to_vec(&changed).unwrap(), &packet),
            Err(error),
            "pointer {pointer}"
        );
    }
}

#[test]
fn exact_codex_config_is_required_byte_for_byte() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let policy = parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();
    verify_codex_config(CONFIG_FIXTURE, &policy).unwrap();

    let mut changed = CONFIG_FIXTURE.to_vec();
    changed[0] ^= 1;
    assert_eq!(
        verify_codex_config(&changed, &policy),
        Err(ConfigAuthorityError::Config)
    );
    assert_eq!(
        verify_codex_config(&vec![b' '; 4_097], &policy),
        Err(ConfigAuthorityError::ConfigBytes)
    );
}

#[cfg(windows)]
#[test]
fn launch_environment_derives_required_windows_values_and_node_from_the_live_machine() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let policy = parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();
    let config = verify_codex_config(CONFIG_FIXTURE, &policy).unwrap();
    let system_windows = authority::system_windows_directory_for_test().unwrap();
    let lower_system_windows =
        std::path::PathBuf::from(system_windows.to_str().unwrap().to_ascii_lowercase());
    let environment = build_codex_launch_environment(
        &packet,
        &policy,
        &config,
        &system_windows,
        &lower_system_windows,
    )
    .unwrap();
    let entries = environment.entries();
    assert_eq!(entries.len(), 10);
    assert_eq!(
        entries[0],
        ("CODEX_HOME".into(), ISOLATED_CODEX_HOME.into())
    );
    assert_eq!(entries[1].0, "PATH");
    assert!(
        std::path::Path::new(&entries[1].1)
            .join("node.exe")
            .is_file()
    );
    assert_eq!(entries[2].0, "ComSpec");
    assert!(std::path::Path::new(&entries[2].1).is_file());
    assert_eq!(entries[3].0, "PATHEXT");
    assert!(!entries[3].1.is_empty());
    assert_eq!(entries[4].0, "SystemRoot");
    assert_eq!(
        entries[5],
        (
            "TEMP".into(),
            r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec\tmp".into(),
        )
    );
    assert_eq!(
        entries[6],
        (
            "TMP".into(),
            r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec\tmp".into(),
        )
    );
    for (index, key) in [(7, "USERPROFILE"), (8, "APPDATA"), (9, "WINDIR")] {
        assert_eq!(entries[index].0, key);
        assert!(!entries[index].1.is_empty());
    }
    assert_eq!(entries[4].1, system_windows.to_str().unwrap());
    assert!(entries[4].1.eq_ignore_ascii_case(&entries[9].1));
    assert!(std::path::Path::new(&entries[4].1).is_dir());
    assert!(environment.entries().iter().all(|(key, _)| !matches!(
        key.as_str(),
        "OPENAI_API_KEY" | "HTTP_PROXY" | "HTTPS_PROXY" | "SSL_CERT_FILE"
    )));

    let private_windows = system_windows.join("per-user-private-windows");
    assert!(matches!(
        build_codex_launch_environment(
            &packet,
            &policy,
            &config,
            &system_windows,
            &private_windows,
        ),
        Err(EnvironmentAuthorityError::SystemRoot)
    ));
    assert!(matches!(
        build_codex_launch_environment(
            &packet,
            &policy,
            &config,
            &private_windows,
            &private_windows,
        ),
        Err(EnvironmentAuthorityError::SystemRoot)
    ));
}

#[cfg(not(windows))]
#[test]
fn codex_launch_environment_is_unavailable_off_windows() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let policy = parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();
    let config = verify_codex_config(CONFIG_FIXTURE, &policy).unwrap();
    assert!(matches!(
        build_codex_launch_environment(
            &packet,
            &policy,
            &config,
            std::path::Path::new("/does/not/exist"),
            std::path::Path::new("/also/absent"),
        ),
        Err(EnvironmentAuthorityError::PlatformUnavailable)
    ));
}

#[test]
fn retention_protects_credentials_and_sessions_and_only_reclaims_exact_old_residue() {
    let marker = ResidueMarker::parse("0123456789abcdef0123456789abcdef").unwrap();
    for path in [
        "auth.json",
        "config.toml",
        "state_5.sqlite-wal",
        r"sessions\2026\08\11\rollout.jsonl",
        r"archived_sessions\rollout.jsonl",
    ] {
        assert_eq!(
            classify_codex_state_path(std::path::Path::new(path), &marker, 1_000),
            RetentionDecision::Protected
        );
    }
    let exact = std::path::Path::new(r"tmp\sage-login-0123456789abcdef0123456789abcdef.tmp");
    assert_eq!(
        classify_codex_state_path(exact, &marker, 23),
        RetentionDecision::Preserve
    );
    assert_eq!(
        classify_codex_state_path(exact, &marker, 24),
        RetentionDecision::ReclaimableResidue
    );
    assert_eq!(
        classify_codex_state_path(
            std::path::Path::new(r"tmp\sage-login-fedcba9876543210fedcba9876543210.tmp"),
            &marker,
            1_000,
        ),
        RetentionDecision::Preserve
    );
    assert_eq!(
        classify_codex_state_path(std::path::Path::new("unknown.cache"), &marker, 1_000),
        RetentionDecision::Preserve
    );
}

#[cfg(windows)]
struct TestCodexStateRoot {
    parent: std::path::PathBuf,
    home: std::path::PathBuf,
    cleaned: bool,
}

#[cfg(windows)]
impl TestCodexStateRoot {
    fn new() -> Self {
        let parent =
            std::env::temp_dir().join(format!("sage-codex-authority-{}", uuid::Uuid::new_v4()));
        let home = parent
            .join("CodexState")
            .join("codex-app-server-0.146.0-d3992fec");
        std::fs::create_dir_all(home.join("sessions")).unwrap();
        std::fs::write(home.join("config.toml"), CONFIG_FIXTURE).unwrap();
        Self {
            parent,
            home,
            cleaned: false,
        }
    }

    fn packet_and_policy(
        &self,
    ) -> (
        manifest::MetadataValidatedPacket,
        policy::SecurityPolicyPrerequisite,
        authority::VerifiedCodexConfig,
    ) {
        let home = self.home.to_str().unwrap();
        let mut packet = packet_value();
        relocate_codex_home(&mut packet, home);
        let packet = parse_packet_metadata(&serde_json::to_vec(&packet).unwrap()).unwrap();
        let mut policy = security_policy_value();
        policy["codexHome"] = json!(home);
        let policy = parse_security_policy(&serde_json::to_vec(&policy).unwrap(), &packet).unwrap();
        let config = verify_codex_config(CONFIG_FIXTURE, &policy).unwrap();
        (packet, policy, config)
    }

    fn cleanup(mut self) {
        std::fs::remove_dir_all(&self.parent).unwrap();
        assert!(!self.parent.exists(), "Codex authority test left residue");
        self.cleaned = true;
    }
}

#[cfg(windows)]
impl Drop for TestCodexStateRoot {
    fn drop(&mut self) {
        if !self.cleaned && self.parent.exists() {
            let _ = std::fs::remove_dir_all(&self.parent);
        }
    }
}

#[cfg(windows)]
fn inspect_test_state(
    root: &TestCodexStateRoot,
) -> Result<authority::CodexAuthorityPrerequisite, StateAuthorityError> {
    let (packet, policy, config) = root.packet_and_policy();
    let reproduced = reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);
    inspect_codex_authority(&packet, policy, config, &reproduced, managed)
}

#[cfg(windows)]
#[test]
fn physical_state_authority_accepts_bounded_state_and_leaves_no_residue() {
    let root = TestCodexStateRoot::new();
    std::fs::write(root.home.join("auth.json"), b"opaque credential fixture").unwrap();
    std::fs::write(root.home.join("sessions").join("rollout.jsonl"), b"session").unwrap();
    let (packet, policy, config) = root.packet_and_policy();
    let reproduced = reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);
    let authority = inspect_codex_authority(&packet, policy, config, &reproduced, managed).unwrap();
    authority.revalidate(&packet, &reproduced).unwrap();
    drop(authority);
    root.cleanup();
}

#[cfg(windows)]
#[test]
fn physical_state_authority_rejects_hardlinks_alternate_streams_and_sparse_excess() {
    let hardlinks = TestCodexStateRoot::new();
    let first = hardlinks.home.join("sessions").join("first.jsonl");
    std::fs::write(&first, b"session").unwrap();
    std::fs::hard_link(&first, hardlinks.home.join("sessions").join("second.jsonl")).unwrap();
    assert!(matches!(
        inspect_test_state(&hardlinks),
        Err(StateAuthorityError::Alias)
    ));
    hardlinks.cleanup();

    let streams = TestCodexStateRoot::new();
    let ordinary = streams.home.join("history.jsonl");
    std::fs::write(&ordinary, b"history").unwrap();
    std::fs::write(format!("{}:secret", ordinary.display()), b"hidden").unwrap();
    assert!(matches!(
        inspect_test_state(&streams),
        Err(StateAuthorityError::Stream)
    ));
    streams.cleanup();

    let directory_streams = TestCodexStateRoot::new();
    let directory = directory_streams.home.join("sessions");
    std::fs::write(format!("{}:secret", directory.display()), b"hidden").unwrap();
    assert!(matches!(
        inspect_test_state(&directory_streams),
        Err(StateAuthorityError::Stream)
    ));
    directory_streams.cleanup();

    let sparse = TestCodexStateRoot::new();
    std::fs::File::create(sparse.home.join("large.sparse"))
        .unwrap()
        .set_len(134_217_729)
        .unwrap();
    assert!(matches!(
        inspect_test_state(&sparse),
        Err(StateAuthorityError::FileBytes)
    ));
    sparse.cleanup();
}

#[cfg(windows)]
#[test]
fn physical_state_authority_rejects_namespace_race_and_cleans_it() {
    let root = TestCodexStateRoot::new();
    let raced = root.home.join("appeared-during-inspection");
    authority::inject_before_state_recheck(move || {
        std::fs::write(raced, b"race").unwrap();
    });
    assert!(matches!(
        inspect_test_state(&root),
        Err(StateAuthorityError::ChangedDuringInspection)
    ));
    root.cleanup();
}

#[cfg(windows)]
#[test]
fn physical_state_authority_rejects_root_substitution_after_retaining_its_handle() {
    let root = TestCodexStateRoot::new();
    let original = root.home.clone();
    let displaced = root.parent.join("displaced-root");
    authority::inject_after_state_root_open(move || {
        std::fs::rename(&original, &displaced).unwrap();
        std::fs::create_dir_all(original.join("sessions")).unwrap();
        std::fs::write(original.join("config.toml"), CONFIG_FIXTURE).unwrap();
    });
    assert!(matches!(
        inspect_test_state(&root),
        Err(StateAuthorityError::ChangedDuringInspection)
    ));
    root.cleanup();
}

#[cfg(not(windows))]
#[test]
fn physical_state_authority_is_unavailable_before_disk_off_windows() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let policy = parse_security_policy(SECURITY_POLICY_FIXTURE, &packet).unwrap();
    let config = verify_codex_config(CONFIG_FIXTURE, &policy).unwrap();
    let reproduced = reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);
    assert!(matches!(
        inspect_codex_authority(&packet, policy, config, &reproduced, managed),
        Err(StateAuthorityError::PlatformUnavailable)
    ));
}

#[test]
fn reproduction_requirements_bind_exact_registry_and_unverified_boundaries() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    parse_reproduction_requirements(REPRODUCTION_FIXTURE, &packet).unwrap();

    for (pointer, replacement, error) in [
        (
            "/registry/tarballSha512Hex",
            json!("a".repeat(128)),
            ReproductionError::Registry,
        ),
        (
            "/registry/npmSignature/verificationStatus",
            json!("verified"),
            ReproductionError::VerificationBoundary,
        ),
        (
            "/packageMetadata/licenseClosureStatus",
            json!("complete"),
            ReproductionError::VerificationBoundary,
        ),
        (
            "/managedStaging/aclOwnership",
            json!("verified"),
            ReproductionError::VerificationBoundary,
        ),
    ] {
        let mut changed = reproduction_value();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            parse_reproduction_requirements(&serde_json::to_vec(&changed).unwrap(), &packet),
            Err(error),
            "pointer {pointer}"
        );
    }
}

#[test]
fn reproduction_requirements_reject_malformed_unknown_and_exact_plus_one_bytes() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    assert_eq!(
        parse_reproduction_requirements(b"{", &packet),
        Err(ReproductionError::Malformed)
    );
    let mut unknown = reproduction_value();
    unknown["registry"]["ambientRegistryFallback"] = json!(true);
    assert_eq!(
        parse_reproduction_requirements(&serde_json::to_vec(&unknown).unwrap(), &packet),
        Err(ReproductionError::Malformed)
    );
    assert_eq!(
        parse_reproduction_requirements(&vec![b' '; reproduction::REPRODUCTION_BYTES + 1], &packet),
        Err(ReproductionError::EvidenceBytes)
    );
}

#[test]
fn reproduction_prerequisite_rejects_an_inspection_for_a_different_packet_root() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let requirements = parse_reproduction_requirements(REPRODUCTION_FIXTURE, &packet).unwrap();
    let inspection = PhysicallyInspectedPackage::mismatched_fixture();
    assert!(matches!(
        bind_reproduced_package(requirements, &packet, inspection),
        Err(ReproductionError::PhysicalBinding)
    ));
}

#[test]
fn reproduction_prerequisite_rejects_an_inspection_for_a_different_codex_home() {
    let inspected_packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let inspection = PhysicallyInspectedPackage::matching_fixture(&inspected_packet);
    let mut changed = packet_value();
    changed["codexHome"] = json!(r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec");
    let changed_packet = parse_packet_metadata(&serde_json::to_vec(&changed).unwrap()).unwrap();
    let requirements =
        parse_reproduction_requirements(REPRODUCTION_FIXTURE, &changed_packet).unwrap();

    assert!(matches!(
        bind_reproduced_package(requirements, &changed_packet, inspection),
        Err(ReproductionError::PhysicalBinding)
    ));
}

#[test]
fn managed_staging_prerequisite_rejects_a_different_package_and_state_binding() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let reproduced = reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);

    let mut changed = packet_value();
    relocate_package_root(&mut changed, r"C:\SageHarnesses\Codex\0.146.0\package");
    relocate_codex_home(
        &mut changed,
        r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec",
    );
    let changed_packet = parse_packet_metadata(&serde_json::to_vec(&changed).unwrap()).unwrap();
    let changed_reproduced = reproduction::ReproducedPackagePrerequisite::fixture(&changed_packet);

    assert_eq!(
        managed.revalidate(&changed_packet, &changed_reproduced),
        Err(ManagedStagingError::Binding)
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires exact SAGE_CODEX_0146_PACKAGE_ROOT and SAGE_CODEX_0146_STATE_ROOT managed roots"]
fn exact_registry_package_and_state_satisfy_managed_prerequisite_without_launch_authority() {
    let package_root = std::env::var("SAGE_CODEX_0146_PACKAGE_ROOT")
        .expect("set SAGE_CODEX_0146_PACKAGE_ROOT to the extracted 0.146.0-win32-x64 package");
    let mut packet = packet_value();
    relocate_package_root(&mut packet, &package_root);
    let state_root = std::env::var("SAGE_CODEX_0146_STATE_ROOT")
        .expect("set SAGE_CODEX_0146_STATE_ROOT to the exact existing isolated state root");
    relocate_codex_home(&mut packet, &state_root);
    let packet = parse_packet_metadata(&serde_json::to_vec(&packet).unwrap()).unwrap();
    let requirements = parse_reproduction_requirements(REPRODUCTION_FIXTURE, &packet).unwrap();
    let configured_root =
        ConfiguredManagedPackageRoot::new(std::path::PathBuf::from(package_root)).unwrap();
    let inspection = inspect_managed_package(&packet, &configured_root).unwrap();
    let reproduced = bind_reproduced_package(requirements, &packet, inspection).unwrap();
    let managed_staging = inspect_managed_staging(&packet, &reproduced).unwrap();
    let mut security_policy = security_policy_value();
    security_policy["codexHome"] = json!(state_root);
    let security_policy =
        parse_security_policy(&serde_json::to_vec(&security_policy).unwrap(), &packet).unwrap();

    assert!(matches!(
        CodexRuntime::from_metadata_packet(
            &packet,
            &security_policy,
            &reproduced,
            &managed_staging,
            [7; 32],
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("real-package-reproduction").unwrap(),
        ),
        Err(crate::acp::bindings::adapter::AdapterRuntimeError::LaunchUnavailable)
    ));
}

#[test]
fn codex_0146_uses_captured_read_only_spellings_per_request_shape() {
    let mut candidate = adapter();
    negotiate(&mut candidate);
    let root = candidate.project_root(r"C:\repo".into()).unwrap();
    let start = candidate.start(1, &root, None).unwrap();
    let value: Value = serde_json::from_slice(&start.bytes).unwrap();
    assert_eq!(value["method"], "thread/start");
    assert_eq!(value["params"]["sandbox"], "read-only");
    assert_eq!(value["params"]["approvalPolicy"], "never");
    assert_eq!(value["params"]["ephemeral"], false);
    assert!(value["params"].get("runtimeWorkspaceRoots").is_none());
    assert_eq!(
        value["params"]["dynamicTools"],
        json!([{
            "type": "function",
            "name": "sage_browse",
            "description": "Open a Sage Preview URL. Only loopback HTTP(S) URLs with an explicit port are allowed.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["url"],
                "properties": {"url": {"type": "string", "maxLength": 2048}}
            }
        }])
    );
    assert!(value.to_string().contains("sage_browse"));
    assert!(!value.to_string().contains("sage.read_text"));
    assert!(!value.to_string().contains("sage.apply_file"));
    candidate
        .ingest(
            br#"{"id":1,"result":{"thread":{"id":"thread-1"}}}"#,
            &Mapper,
        )
        .unwrap();
    let prompt = Prompt::new("prompt", "inspect").unwrap();
    let client = RequiredText::new("client", "client-read-only").unwrap();
    let turn = candidate.prompt(2, &prompt, &client).unwrap();
    let turn: Value = serde_json::from_slice(&turn.bytes).unwrap();
    assert_eq!(turn["params"]["approvalPolicy"], "never");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(turn["params"]["sandboxPolicy"]["networkAccess"], false);

    let mut resume_adapter = adapter();
    negotiate(&mut resume_adapter);
    let thread = RequiredText::new("thread", "thread-resume").unwrap();
    let resume = resume_adapter.resume(3, &thread, &root, None).unwrap();
    let resume: Value = serde_json::from_slice(&resume.bytes).unwrap();
    assert_eq!(resume["params"]["approvalPolicy"], "never");
    assert_eq!(resume["params"]["sandbox"], "read-only");
    let caps = candidate.effective_capabilities().unwrap();
    assert!(caps.new_session && caps.resume_session && caps.prompt && caps.cancel);
    assert!(caps.multi_root_extension_schema_id.is_none());
    assert!(!caps.native_session_fork && caps.active_turn_steer);
    assert!(candidate.project_root(r"repo".into()).is_err());
    assert!(candidate.project_root(r"C:\repo\bad:name".into()).is_err());
}

#[test]
fn pinned_codex_0146_admits_ask_and_workspace_write_before_turn_start() {
    for mode in [
        crate::settings_store::SessionAccessMode::Ask,
        crate::settings_store::SessionAccessMode::WorkspaceWrite,
    ] {
        let mut candidate = adapter();
        negotiate(&mut candidate);
        let root = candidate.project_root(r"C:\repo".into()).unwrap();
        candidate.start(1, &root, None).unwrap();
        candidate
            .ingest(
                br#"{"id":1,"result":{"thread":{"id":"thread-1"}}}"#,
                &Mapper,
            )
            .unwrap();
        let result = candidate.prompt_with_access_mode(
            2,
            &Prompt::new("prompt", "inspect").unwrap(),
            &RequiredText::new("client", "client-access-mode").unwrap(),
            mode,
        );
        let request = result.unwrap();
        let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
        assert_eq!(wire["method"], "turn/start");
        assert_eq!(wire["params"]["sandboxPolicy"]["networkAccess"], false);
        assert_eq!(
            wire["params"]["approvalPolicy"],
            if mode == crate::settings_store::SessionAccessMode::Ask {
                "on-request"
            } else {
                "never"
            }
        );
    }
}

#[test]
fn pinned_codex_0146_dispatches_saved_ask_as_read_only() {
    use crate::settings_store::SessionAccessMode;

    let mut candidate = adapter();
    ready(&mut candidate);
    let requested = SessionAccessMode::Ask;
    let turn = candidate.prompt_with_access_mode(
        2,
        &Prompt::new("prompt", "inspect").unwrap(),
        &RequiredText::new("client", "client-ask").unwrap(),
        crate::acp::bindings::access_policy::effective_mode(
            "codex-app-server",
            "0.146.0",
            Some(requested),
            true,
        )
        .unwrap(),
    );
    assert!(turn.is_ok());
    let wire: Value = serde_json::from_slice(&turn.unwrap().bytes).unwrap();
    assert_eq!(wire["method"], "turn/start");
    assert_eq!(wire["params"]["approvalPolicy"], "never");
    assert_eq!(wire["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(requested, SessionAccessMode::Ask);
}

#[test]
fn request_ledger_is_bounded_and_rejects_duplicate_ids() {
    let mut adapter = adapter();
    negotiate(&mut adapter);
    adapter.models(1, None).unwrap();
    assert!(adapter.models(1, None).is_err());
    for id in 2..=16 {
        adapter.models(id, None).unwrap();
    }
    assert!(adapter.models(17, None).is_err());
}

#[test]
fn initialize_requires_the_exact_runtime_home_and_windows_identity() {
    let mut incomplete = adapter();
    assert!(matches!(
        incomplete.effective_capabilities(),
        Err(AdapterError::CapabilityUnavailable)
    ));
    incomplete.initialize(1).unwrap();
    let missing = br#"{"id":1,"result":{"userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"}}"#;
    assert!(matches!(
        incomplete.ingest(missing, &Mapper),
        Err(AdapterError::Ingress(IngressError::CapabilityUnavailable))
    ));

    let mut negotiated = adapter();
    negotiate(&mut negotiated);
    let capabilities = negotiated.effective_capabilities().unwrap();
    assert!(capabilities.new_session && capabilities.prompt && capabilities.cancel);
    assert!(capabilities.collaboration_event_projection);
    assert!(!capabilities.native_session_fork && capabilities.active_turn_steer);
    assert_eq!(
        capabilities.protocol_version.as_str(),
        "codex-app-server-0.146.0-d3992fec"
    );

    for (pointer, replacement) in [
        (
            "/result/userAgent",
            json!("sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"),
        ),
        (
            "/result/userAgent",
            json!("Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"),
        ),
        ("/result/codexHome", json!(r"C:\Users\Username\.codex")),
        ("/result/platformFamily", json!("unix")),
        ("/result/platformOs", json!("linux")),
    ] {
        let mut rejected = adapter();
        rejected.initialize(2).unwrap();
        let mut response: Value = serde_json::from_slice(&initialize_result(2)).unwrap();
        *response.pointer_mut(pointer).unwrap() = replacement;
        assert!(matches!(
            rejected.ingest(&serde_json::to_vec(&response).unwrap(), &Mapper),
            Err(AdapterError::Ingress(IngressError::CapabilityUnavailable))
        ));
    }

    let mut extra = adapter();
    extra.initialize(3).unwrap();
    let mut response: Value = serde_json::from_slice(&initialize_result(3)).unwrap();
    response["result"]["capabilities"] = json!({"turnStart": true});
    assert!(matches!(
        extra.ingest(&serde_json::to_vec(&response).unwrap(), &Mapper),
        Err(AdapterError::Ingress(IngressError::CapabilityUnavailable))
    ));
}

#[test]
fn observed_codex_0146_initialize_accepts_its_cli_identity_and_home() {
    let mut result = Map::from_iter([
        (
            "userAgent".into(),
            json!("sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"),
        ),
        ("codexHome".into(), json!(r"C:\Users\Username\.codex")),
        ("platformFamily".into(), json!("windows")),
        ("platformOs".into(), json!("windows")),
    ]);

    assert!(super::state::capabilities::valid_initialize_result(
        &result,
        "codex-cli 0.146.0",
        "",
        super::state::capabilities::InitializeIdentity::Observed,
    ));
    result.insert(
        "userAgent".into(),
        json!("sage/0.146.0 (Windows 10.0; x86_64)"),
    );
    assert!(!super::state::capabilities::valid_initialize_result(
        &result,
        "codex-cli 0.146.0",
        "",
        super::state::capabilities::InitializeIdentity::Observed,
    ));
}

#[test]
fn the_user_agent_codex_0146_actually_sends_is_accepted_and_forgeries_are_not() {
    let observed = Map::from_iter([
        (
            "userAgent".into(),
            json!("Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64) dumb (sage; 0.1.0)"),
        ),
        (
            "codexHome".into(),
            json!(r"C:\Users\Username\AppData\Local\Temp\sage-codex-home-DoXG8r"),
        ),
        ("platformFamily".into(), json!("windows")),
        ("platformOs".into(), json!("windows")),
    ]);
    assert!(super::state::capabilities::valid_initialize_result(
        &observed,
        "codex-cli 0.146.0",
        "",
        super::state::capabilities::InitializeIdentity::Observed,
    ));

    for forged in [
        "Codex Desktop/0.145.0 (Windows 10.0.26200; x86_64) dumb (sage; 0.1.0)",
        "Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64) dumb (notsage; 0.1.0)",
        "Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64) dumb",
        "/0.146.0 (Windows 10.0.26200; x86_64) dumb (sage; 0.1.0)",
        "Codex Desktop/0.146.0 (Windows notaversion; x86_64) dumb (sage; 0.1.0)",
        "Codex Desktop/0.146.0 (Windows 10.0.26200; x86_64)  (sage; 0.1.0)",
    ] {
        let mut drifted = observed.clone();
        drifted.insert("userAgent".into(), json!(forged));
        assert!(
            !super::state::capabilities::valid_initialize_result(
                &drifted,
                "codex-cli 0.146.0",
                "",
                super::state::capabilities::InitializeIdentity::Observed,
            ),
            "forged user agent accepted: {forged}"
        );
    }
}

#[test]
fn only_one_turn_start_can_be_pending_or_active_and_failed_egress_rolls_back() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-one");
    let prompt = Prompt::new("prompt", "second").unwrap();
    let client = RequiredText::new("client", "client-two").unwrap();
    assert!(matches!(
        adapter.prompt(3, &prompt, &client),
        Err(AdapterError::TurnAlreadyActive)
    ));
    adapter.reject_before_write(2).unwrap();
    adapter.prompt(3, &prompt, &client).unwrap();

    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    assert!(matches!(
        adapter.prompt(4, &prompt, &client),
        Err(AdapterError::TurnAlreadyActive)
    ));
}

#[test]
fn structured_turn_plan_preserves_step_states_without_a_tool_row() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-plan");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let update = json!({"method":"turn/plan/updated","params":{
        "threadId":"thread-1","turnId":"turn-1","plan":[
            {"step":"Inspect","status":"completed"},
            {"step":"Patch","status":"inProgress"}
        ]
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&update).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(events.as_slice(), [IncomingEvent::Item {
        payload: NormalizedPayload::PlanEntries { entries }, tool_state: None, ..
    }] if entries.as_slice().len() == 2
        && entries.as_slice()[0].state == crate::acp::intent::NormalizedPlanState::Completed
        && entries.as_slice()[1].state == crate::acp::intent::NormalizedPlanState::InProgress));
}

#[test]
fn pinned_error_notification_validates_identity_and_preserves_retryability() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-error");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();

    let error = |thread: &str, turn: &str| {
        json!({"method":"error","params":{
            "threadId":thread,
            "turnId":turn,
            "willRetry":true,
            "error":{"message":"retrying error"}
        }})
    };
    let IngressDisposition::Events(events) = adapter
        .ingest(
            &serde_json::to_vec(&error("thread-1", "turn-1")).unwrap(),
            &Mapper,
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [IncomingEvent::Error(NormalizedPayload::Error {
            kind: None,
            code,
            message,
            retryable: true,
        })] if code.as_str() == "codex" && message.as_str() == "retrying error"
    ));

    for foreign in [
        error("foreign-thread", "turn-1"),
        error("thread-1", "foreign-turn"),
    ] {
        assert!(matches!(
            adapter.ingest(&serde_json::to_vec(&foreign).unwrap(), &Mapper),
            Err(AdapterError::Ingress(IngressError::Identity))
        ));
    }
    let delta = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","delta":"still active"
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&delta).unwrap(), &Mapper),
        Ok(IngressDisposition::Events(_))
    ));
}

#[test]
fn malformed_pinned_error_notifications_fail_closed() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-error-malformed");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();

    for params in [
        json!({"threadId":"thread-1","turnId":"turn-1","willRetry":true}),
        json!({"threadId":"thread-1","turnId":"turn-1","willRetry":true,"error":{}}),
        json!({"threadId":"thread-1","turnId":"turn-1","willRetry":true,"error":{"message":false}}),
        json!({"threadId":"thread-1","turnId":"turn-1","error":{"message":"missing retry"}}),
        json!({"threadId":"thread-1","turnId":"turn-1","willRetry":"true","error":{"message":"wrong retry"}}),
    ] {
        let frame = json!({"method":"error","params":params});
        assert!(matches!(
            adapter.ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper),
            Err(AdapterError::Ingress(IngressError::Malformed))
        ));
    }
    let delta = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","delta":"still active"
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&delta).unwrap(), &Mapper),
        Ok(IngressDisposition::Events(_))
    ));
}

#[test]
fn notification_first_turn_start_error_fails_active_turn_or_is_late() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-notification-first");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let response_error = br#"{"id":2,"error":{"message":"start rejected"}}"#;
    let IngressDisposition::Events(events) = adapter.ingest(response_error, &Mapper).unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        events.as_slice(),
        [
            IncomingEvent::Error(_),
            IncomingEvent::Turn {
                state: TurnState::Failed,
                ..
            }
        ]
    ));
    assert!(matches!(
        adapter.ingest(response_error, &Mapper),
        Err(AdapterError::Ingress(IngressError::Uncorrelated))
    ));

    arm_turn(&mut adapter, 3, "client-terminal-first");
    let next_started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-2","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&next_started).unwrap(), &Mapper)
        .unwrap();
    let completed = json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-2","status":"failed"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&completed).unwrap(), &Mapper)
        .unwrap();
    assert_eq!(
        adapter
            .ingest(br#"{"id":3,"error":{"message":"late rejection"}}"#, &Mapper)
            .unwrap(),
        IngressDisposition::DuplicateTerminal
    );
}

#[test]
fn response_first_turn_start_rejects_previous_terminal_without_corrupting_correlations() {
    let mut adapter = adapter();
    ready(&mut adapter);

    arm_turn(&mut adapter, 2, "client-delayed-response");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let completed = json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&completed).unwrap(), &Mapper)
        .unwrap();

    arm_turn(&mut adapter, 3, "client-new-response-first");
    assert!(matches!(
        adapter.ingest(
            br#"{"id":3,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#,
            &Mapper,
        ),
        Err(AdapterError::Ingress(IngressError::Identity))
    ));
    assert_eq!(adapter.pending_kind(2), Some(RequestKind::TurnStart));
    assert_eq!(adapter.pending_kind(3), Some(RequestKind::TurnStart));

    assert_eq!(
        adapter
            .ingest(
                br#"{"id":2,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#,
                &Mapper,
            )
            .unwrap(),
        IngressDisposition::DuplicateTerminal
    );
    assert!(matches!(
        adapter
            .ingest(
                br#"{"id":3,"result":{"turn":{"id":"turn-2","status":"inProgress"}}}"#,
                &Mapper,
            )
            .unwrap(),
        IngressDisposition::Events(_)
    ));
}

#[test]
fn delayed_turn_start_response_stays_terminal_after_tombstone_eviction() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let started = |turn: &str| {
        json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":turn,"status":"inProgress"}
        }})
    };
    let completed = |turn: &str| {
        json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":turn,"status":"completed"}
        }})
    };

    arm_turn(&mut adapter, 2, "client-delayed-oldest");
    for frame in [started("turn-1"), completed("turn-1")] {
        adapter
            .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
            .unwrap();
    }

    for (request_id, turn_id) in [(3, "turn-2"), (4, "turn-3")] {
        arm_turn(&mut adapter, request_id, &format!("client-{turn_id}"));
        adapter
            .ingest(&serde_json::to_vec(&started(turn_id)).unwrap(), &Mapper)
            .unwrap();
        let response = json!({
            "id": request_id,
            "result": {"turn":{"id":turn_id,"status":"inProgress"}}
        });
        assert_eq!(
            adapter
                .ingest(&serde_json::to_vec(&response).unwrap(), &Mapper)
                .unwrap(),
            IngressDisposition::IgnoredLate
        );
        adapter
            .ingest(&serde_json::to_vec(&completed(turn_id)).unwrap(), &Mapper)
            .unwrap();
    }

    assert_eq!(
        adapter
            .ingest(
                br#"{"id":2,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#,
                &Mapper,
            )
            .unwrap(),
        IngressDisposition::DuplicateTerminal
    );
}

#[test]
fn pinned_lifecycle_stream_normalizes_and_late_delta_is_ignored() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-fixture");
    let frames: Vec<Value> = serde_json::from_slice(include_bytes!(
        "../../../../testdata/acp/codex/pinned-events.json"
    ))
    .unwrap();
    for frame in &frames {
        let encoded = serde_json::to_vec(frame).unwrap();
        adapter.ingest(&encoded, &Mapper).unwrap();
    }
    let duplicate = serde_json::to_vec(&frames[3]).unwrap();
    assert_eq!(
        adapter.ingest(&duplicate, &Mapper).unwrap(),
        IngressDisposition::DuplicateTerminal
    );
    let late = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","delta":"late"
    }});
    assert_eq!(
        adapter
            .ingest(&serde_json::to_vec(&late).unwrap(), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
    let late_item = json!({"method":"item/started","params":{
        "threadId":"thread-1","turnId":"turn-1",
        "item":{"type":"agentMessage","id":"late-item","text":"late"}
    }});
    assert_eq!(
        adapter
            .ingest(&serde_json::to_vec(&late_item).unwrap(), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
}

#[test]
fn item_terminal_identity_is_turn_scoped_and_retains_one_previous_tombstone() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-turn-one");
    let started = |turn: &str| {
        json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":turn,"status":"inProgress"}
        }})
    };
    let item = |turn: &str, text: &str| {
        json!({"method":"item/completed","params":{
            "threadId":"thread-1","turnId":turn,
            "item":{"type":"agentMessage","id":"reused-item","text":text}
        }})
    };
    let complete = |turn: &str| {
        json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":turn,"status":"completed"}
        }})
    };

    adapter
        .ingest(&serde_json::to_vec(&started("turn-1")).unwrap(), &Mapper)
        .unwrap();
    adapter
        .ingest(
            br#"{"id":2,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#,
            &Mapper,
        )
        .unwrap();
    for frame in [item("turn-1", "one"), complete("turn-1")] {
        adapter
            .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
            .unwrap();
    }
    arm_turn(&mut adapter, 3, "client-turn-two");
    adapter
        .ingest(&serde_json::to_vec(&started("turn-2")).unwrap(), &Mapper)
        .unwrap();
    let second = adapter
        .ingest(
            &serde_json::to_vec(&item("turn-2", "two")).unwrap(),
            &Mapper,
        )
        .unwrap();
    let IngressDisposition::Events(events) = second else {
        panic!()
    };
    let IncomingEvent::Item { id, .. } = &events[0] else {
        panic!()
    };
    assert_eq!(id.turn_id.as_ref().unwrap().as_str(), "turn-2");
    assert_eq!(
        id.turn_client_message_id.as_ref().unwrap().as_str(),
        "client-turn-two"
    );

    let late_old = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"old-new-item","delta":"late"
    }});
    assert_eq!(
        adapter
            .ingest(&serde_json::to_vec(&late_old).unwrap(), &Mapper)
            .unwrap(),
        IngressDisposition::IgnoredLate
    );
    assert_eq!(
        adapter
            .ingest(
                &serde_json::to_vec(&item("turn-1", "one")).unwrap(),
                &Mapper
            )
            .unwrap(),
        IngressDisposition::DuplicateTerminal
    );
}

#[test]
fn turn_completed_keeps_the_active_turn_correlation() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-completed-scope");
    adapter
        .ingest(
            &serde_json::to_vec(&json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }}))
            .unwrap(),
            &Mapper,
        )
        .unwrap();
    let completed = adapter
        .ingest(
            &serde_json::to_vec(&json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }}))
            .unwrap(),
            &Mapper,
        )
        .unwrap();
    let IngressDisposition::Events(events) = completed else {
        panic!("completed turn must emit events")
    };
    let IncomingEvent::Turn { id, state } = &events[0] else {
        panic!("completed turn must emit a Turn event")
    };
    assert_eq!(*state, TurnState::Completed);
    assert_eq!(id.turn_id.as_ref().unwrap().as_str(), "turn-1");
    assert_eq!(
        id.turn_client_message_id.as_ref().unwrap().as_str(),
        "client-completed-scope"
    );
}

#[test]
fn conflicting_terminal_and_foreign_scope_fail_closed() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-conflict");
    let started = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let overlapping = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-2","status":"inProgress"}}});
    assert!(
        adapter
            .ingest(&serde_json::to_vec(&overlapping).unwrap(), &Mapper)
            .is_err()
    );
    let complete = |status: &str| json!({"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":status}}});
    adapter
        .ingest(
            &serde_json::to_vec(&complete("completed")).unwrap(),
            &Mapper,
        )
        .unwrap();
    assert!(
        adapter
            .ingest(&serde_json::to_vec(&complete("failed")).unwrap(), &Mapper)
            .is_err()
    );
    let foreign = json!({"method":"item/started","params":{"threadId":"foreign","turnId":"turn-1","item":{"type":"agentMessage","id":"item","text":"x"}}});
    assert!(
        adapter
            .ingest(&serde_json::to_vec(&foreign).unwrap(), &Mapper)
            .is_err()
    );
}

#[test]
fn turn_started_rejection_diagnostic_classifies_only_safe_adapter_state() {
    use super::state::TurnStartedRejection;

    let mut foreign_thread = adapter();
    ready(&mut foreign_thread);
    assert_eq!(
        TurnStartedRejection::from_adapter_error(
            &foreign_thread.expect_thread("foreign-thread").unwrap_err()
        ),
        TurnStartedRejection::ForeignThread
    );

    let mut overlapping_turn = adapter();
    ready(&mut overlapping_turn);
    arm_turn(&mut overlapping_turn, 2, "client-overlapping");
    overlapping_turn
        .observe_turn_started("turn-1")
        .expect("first turn starts");
    assert_eq!(
        TurnStartedRejection::from_adapter_error(
            &overlapping_turn.observe_turn_started("turn-2").unwrap_err()
        ),
        TurnStartedRejection::OverlappingTurn
    );

    let mut missing_correlation = adapter();
    ready(&mut missing_correlation);
    assert_eq!(
        TurnStartedRejection::from_adapter_error(
            &missing_correlation
                .observe_turn_started("uncorrelated-turn")
                .unwrap_err()
        ),
        TurnStartedRejection::MissingCorrelation
    );
    assert_eq!(
        TurnStartedRejection::ForeignThread.as_str(),
        "foreignThread"
    );
    assert_eq!(
        TurnStartedRejection::OverlappingTurn.as_str(),
        "overlappingTurn"
    );
    assert_eq!(
        TurnStartedRejection::MissingCorrelation.as_str(),
        "missingCorrelation"
    );
}

#[test]
fn permission_request_produces_only_a_decline_and_no_raw_authority() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-permission");
    let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    let request = json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1",
        "command":"dangerous secret command","cwd":"C:\\Users\\secret"
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    let IncomingEvent::Permission {
        payload, denial, ..
    } = &events[0]
    else {
        panic!()
    };
    assert!(matches!(
        payload,
        NormalizedPayload::PermissionRequest { .. }
    ));
    assert_eq!(
        serde_json::to_value(payload).unwrap()["payload"]["offeredOptionKinds"],
        json!(["allow_once", "allow_always", "reject_once"])
    );
    let response: Value = serde_json::from_slice(&denial.bytes).unwrap();
    assert_eq!(response["result"]["decision"], "decline");
    assert!(
        !serde_json::to_string(payload)
            .unwrap()
            .contains("dangerous")
    );
    assert!(!serde_json::to_string(payload).unwrap().contains("Users"));

    assert_eq!(
        adapter
            .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
            .unwrap(),
        IngressDisposition::DuplicateServerRequest { denial: None }
    );

    for state in [
        ServerRequestState::DeclinePending,
        ServerRequestState::DeclineCommitted,
        ServerRequestState::ResponseQueued,
        ServerRequestState::ResolvedDeclined,
    ] {
        adapter
            .advance_permission_decline("string:approval-1", state)
            .unwrap();
    }
    let duplicate = adapter
        .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
        .unwrap();
    assert!(matches!(
        duplicate,
        IngressDisposition::DuplicateServerRequest { denial: Some(bytes) }
            if serde_json::from_slice::<Value>(&bytes).unwrap()["result"]["decision"] == "decline"
    ));

    let conflicting = json!({"id":"approval-1","method":"item/fileChange/requestApproval","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-2"
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&conflicting).unwrap(), &Mapper),
        Err(AdapterError::Ingress(
            IngressError::PermissionTeardownRequired
        ))
    ));
}

#[test]
fn command_approval_receipt_never_projects_untrusted_command_text() {
    for (index, command) in [
        "API_KEY=hunter2",
        "MY_SECRET=hunter2",
        "--auth-token=abc123",
        "--password abc123",
        "Bearer abc123",
        "Basic abc123",
        "sk-abc123",
        "ghp_abc123",
        "xoxb-abc123",
        "https://user:pass@example.com/",
        "$env:API_KEY='hunter2'",
        "Get-\u{202e}Content",
    ]
    .into_iter()
    .enumerate()
    {
        let mut adapter = adapter();
        ready(&mut adapter);
        arm_turn(&mut adapter, 2, "client-permission");
        let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
        adapter
            .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
            .unwrap();
        let request = json!({"id":format!("approval-{index}"),"method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","command":command
        }});
        let IngressDisposition::Events(events) = adapter
            .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
            .unwrap()
        else {
            panic!("events")
        };
        let IncomingEvent::Permission {
            payload: NormalizedPayload::PermissionRequest { summary, .. },
            ..
        } = &events[0]
        else {
            panic!("permission")
        };
        assert_eq!(summary.as_str(), "Run a command", "{command}");
    }
}

#[test]
fn command_approval_carries_codex_reason_sanitized_and_only_when_given() {
    for (index, (reason, expected)) in [
        (
            Some("May I retry the network check outside the sandbox?"),
            Some("May I retry the network check outside the sandbox?"),
        ),
        (
            Some("Use sk-abc123 to call it"),
            Some("Use [redacted] to call it"),
        ),
        (Some("   "), None),
        (None, None),
    ]
    .into_iter()
    .enumerate()
    {
        let mut adapter = adapter();
        ready(&mut adapter);
        arm_turn(&mut adapter, 2, "client-permission");
        let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
        adapter
            .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
            .unwrap();
        let mut params = json!({
            "threadId":"thread-1","turnId":"turn-1","itemId":"item-1","command":"curl.exe -s"
        });
        if let Some(reason) = reason {
            params["reason"] = json!(reason);
        }
        let request = json!({"id":format!("approval-{index}"),"method":"item/commandExecution/requestApproval","params":params});
        let IngressDisposition::Events(events) = adapter
            .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
            .unwrap()
        else {
            panic!("events")
        };
        let IncomingEvent::Permission {
            payload:
                NormalizedPayload::PermissionRequest {
                    summary,
                    reason: projected,
                    category,
                    ..
                },
            ..
        } = &events[0]
        else {
            panic!("permission")
        };
        assert_eq!(summary.as_str(), "Run a command");
        assert_eq!(*category, crate::acp::intent::ToolCategory::Execute);
        assert_eq!(projected.as_ref().map(|value| value.as_str()), expected);
    }
}

#[test]
fn unresolved_permission_requests_are_bounded_at_exact_and_plus_one() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-permission-cap");
    let turn = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    for index in 0..8 {
        let request = json!({"id":format!("approval-{index}"),
        "method":"item/commandExecution/requestApproval","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":format!("item-{index}")
        }});
        assert!(matches!(
            adapter
                .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
                .unwrap(),
            IngressDisposition::Events(_)
        ));
    }
    let over = json!({"id":"approval-over",
    "method":"item/commandExecution/requestApproval","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-over"
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&over).unwrap(), &Mapper),
        Err(AdapterError::Ingress(
            IngressError::PermissionTeardownRequired
        ))
    ));
}

#[test]
fn paths_and_secrets_are_redacted_and_file_paths_require_native_mapping() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-redaction");
    let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    let delta = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1",
        "delta":"read C:\\Users\\secret\\token and Bearer abc"
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&delta).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    let IncomingEvent::Item { payload, .. } = &events[0] else {
        panic!()
    };
    let encoded = serde_json::to_string(payload).unwrap();
    assert!(!encoded.contains("Users"));
    assert!(!encoded.contains("abc"));

    let spaced = json!({"method":"item/agentMessage/delta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-2",
        "delta":"  keep\t(C:\\Users\\private\\x) `/home/private/x` file:///C:/private/x\n"
    }});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&spaced).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    let IncomingEvent::Item {
        payload: NormalizedPayload::Text { text },
        ..
    } = &events[0]
    else {
        panic!()
    };
    assert!(text.as_str().starts_with("  keep\t"));
    assert!(text.as_str().ends_with('\n'));
    assert_eq!(text.as_str().matches("[path]").count(), 3);
    assert!(!text.as_str().contains("private"));

    let unknown = json!({"method":"`C:\\Users\\private\\method`","params":{}});
    let (disposition, captured) = with_captured_diagnostics(|| {
        adapter.ingest(&serde_json::to_vec(&unknown).unwrap(), &Mapper)
    });
    assert!(matches!(disposition, Ok(IngressDisposition::IgnoredLate)));
    assert!(captured.contains(UNPROJECTED_DIAGNOSTIC));
    assert!(captured.contains("[path]"));
    assert!(!captured.contains("private"));

    let file = json!({"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-1","item":{
        "type":"fileChange","id":"file-1","status":"completed","changes":[{"path":"C:\\foreign\\x.rs","kind":{"type":"update"},"diff":"secret"}]
    }}});
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&file).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        &events[0],
        IncomingEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                resource: crate::acp::intent::SafeResourceLocation::Unavailable {
                    reason: crate::acp::intent::UnavailableResourceReason::Foreign,
                    ..
                },
                summary,
                ..
            },
            ..
        } if !summary.as_str().contains("secret")
    ));
}

#[test]
fn malformed_oversized_and_unknown_inputs_are_bounded() {
    let mut adapter = adapter();
    assert!(adapter.ingest(b"{", &Mapper).is_err());
    assert!(adapter.ingest(&vec![b'x'; 1_048_577], &Mapper).is_err());
    let unknown = json!({"method":"future/event","params":{}});
    let (disposition, captured) = with_captured_diagnostics(|| {
        adapter.ingest(&serde_json::to_vec(&unknown).unwrap(), &Mapper)
    });
    assert!(matches!(disposition, Ok(IngressDisposition::IgnoredLate)));
    assert!(captured.contains(UNPROJECTED_DIAGNOSTIC));
}

#[test]
fn known_status_updates_and_user_echoes_do_not_create_unsupported_transcript_items() {
    let mut adapter = adapter();
    for method in [
        "account/updated",
        "account/rateLimits/updated",
        "skills/changed",
    ] {
        let account = json!({"method":method,"params":{"account":"ignored"}});
        assert!(matches!(
            adapter.ingest(&serde_json::to_vec(&account).unwrap(), &Mapper),
            Ok(IngressDisposition::IgnoredLate)
        ));
    }

    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-user-echo");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    for method in ["item/started", "item/completed"] {
        let echo = json!({"method":method,"params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"userMessage","id":"echo-1","text":"already local"
            }
        }});
        assert!(matches!(
            adapter.ingest(&serde_json::to_vec(&echo).unwrap(), &Mapper),
            Ok(IngressDisposition::Events(events)) if events.is_empty()
        ));
    }
}

#[test]
fn a_spoofed_dynamic_tool_call_requires_runtime_teardown() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-unsupported");
    let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    let request = json!({"id":"unsafe-1","method":"item/tool/call","params":{
        "threadId":"thread-1","turnId":"turn-1","callId":"call-1","tool":"_sage/browse",
        "arguments":{"url":"http://127.0.0.1:1/"}
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&request).unwrap(), &Mapper),
        Err(AdapterError::Ingress(
            IngressError::UnsupportedServerRequest
        ))
    ));
}

#[test]
fn codex_dynamic_browse_call_returns_only_admitted_loopback_text() {
    let mut candidate = adapter();
    ready(&mut candidate);
    arm_turn(&mut candidate, 2, "client-dynamic-browse");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    candidate
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let request = json!({"id":"browse-request-1","method":"item/tool/call","params":{
        "threadId":"thread-1","turnId":"turn-1","callId":"browse-call-1",
        "tool":"sage_browse","arguments":{"url":"http://127.0.0.1:43124/proof"}
    }});
    let IngressDisposition::Events(events) = candidate
        .ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    let [IncomingEvent::HostToolReply { bytes }] = events.as_slice() else {
        panic!()
    };
    assert_eq!(
        serde_json::from_slice::<Value>(bytes).unwrap(),
        json!({"id":"browse-request-1","result":{
            "success":true,
            "contentItems":[{"type":"inputText","text":"http://127.0.0.1:43124/proof"}]
        }})
    );

    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-dynamic-browse-foreign");
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    let foreign = json!({"id":"browse-request-2","method":"item/tool/call","params":{
        "threadId":"thread-1","turnId":"turn-1","callId":"browse-call-2",
        "tool":"sage_browse","arguments":{"url":"https://example.com/"}
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&foreign).unwrap(), &Mapper),
        Err(AdapterError::Ingress(IngressError::Malformed))
    ));
}

mod p01_tests;

#[test]
fn all_outgoing_lifecycle_methods_are_exact_and_bounded() {
    let mut candidate = adapter();
    let root = candidate.project_root(r"C:\repo".into()).unwrap();
    let external = ExternalId::new("external", "thread-1").unwrap();
    let initialize = candidate.initialize(1).unwrap();
    let initialize_value = serde_json::from_slice::<Value>(&initialize.bytes).unwrap();
    assert_eq!(initialize_value["method"], "initialize");
    assert_eq!(initialize_value["params"]["clientInfo"]["name"], "sage");
    assert_eq!(initialize_value["params"]["clientInfo"]["title"], "Sage");
    assert_eq!(
        initialize_value["params"]["clientInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        initialize_value["params"]["capabilities"]["experimentalApi"],
        true
    );
    assert!(!initialize.bytes.ends_with(b"\n"));
    candidate.ingest(&initialize_result(1), &Mapper).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&candidate.initialized().unwrap()).unwrap()["method"],
        "initialized"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &candidate.resume(2, &external, &root, None).unwrap().bytes
        )
        .unwrap()["method"],
        "thread/resume"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&candidate.read(3, &external).unwrap().bytes).unwrap()["method"],
        "thread/read"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&candidate.models(4, None).unwrap().bytes).unwrap()["method"],
        "model/list"
    );
    let model_response = br#"{"id":4,"result":{"data":[{"id":"gpt-5","displayName":"GPT-5","isDefault":true}],"nextCursor":"page-2"}}"#;
    let IngressDisposition::Events(model_events) =
        candidate.ingest(model_response, &Mapper).unwrap()
    else {
        panic!()
    };
    let IncomingEvent::Models {
        models,
        next_cursor,
    } = &model_events[0]
    else {
        panic!()
    };
    assert_eq!(models.len(), 1);
    assert_eq!(next_cursor.as_ref().unwrap().as_str(), "page-2");
    let cursor = RequiredText::new("cursor", "page-2").unwrap();
    let next_page = candidate.models(5, Some(&cursor)).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&next_page.bytes).unwrap()["params"]["cursor"],
        "page-2"
    );

    let mut active = adapter();
    ready(&mut active);
    let prompt = Prompt::new("prompt", "hello").unwrap();
    let client = RequiredText::new("client", "client-1").unwrap();
    let request = active.prompt(2, &prompt, &client).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&request.bytes).unwrap()["method"],
        "turn/start"
    );
    let response = br#"{"id":2,"result":{"turn":{"id":"turn-1","status":"inProgress"}}}"#;
    active.ingest(response, &Mapper).unwrap();
    let usage = json!({"method":"thread/tokenUsage/updated","params":{
        "threadId":"thread-1","turnId":"turn-1","tokenUsage":{
            "total":{"inputTokens":10,"outputTokens":4},
            "last":{"inputTokens":3,"outputTokens":2},
            "modelContextWindow":272000
        }
    }});
    let IngressDisposition::Events(usage_events) = active
        .ingest(&serde_json::to_vec(&usage).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        &usage_events[0],
        IncomingEvent::Usage {
            payload: NormalizedPayload::Usage {
                input_tokens: 3,
                output_tokens: 2,
                context_window: Some(272_000),
            },
            ..
        }
    ));
    assert_eq!(
        serde_json::from_slice::<Value>(&active.cancel(3).unwrap().bytes).unwrap()["method"],
        "turn/interrupt"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&active.close(4).unwrap().bytes).unwrap()["method"],
        "thread/unsubscribe"
    );
}
