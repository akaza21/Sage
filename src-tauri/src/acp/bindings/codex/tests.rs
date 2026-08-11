use serde_json::{Value, json};

use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageRootId},
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef},
    lifecycle::ServerRequestState,
};

use super::*;
use super::{incoming::IngressError, state::AdapterError};

struct Mapper;

const PACKET_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/candidate-windows.json");
const SECURITY_POLICY_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/security-policy-v1.json");
const REPRODUCTION_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/package-reproduction-v1.json");
const PACKAGE_ROOT: &str = r"C:\SageHarnesses\Codex\0.145.0\package";
const ISOLATED_CODEX_HOME: &str = r"C:\SageHarnesses\CodexState\codex-app-server-0.145.0-7b0ab167";

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
            "userAgent": "sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
            "codexHome": "C:\\SageHarnesses\\CodexState\\codex-app-server-0.145.0-7b0ab167",
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

fn arm_turn(adapter: &mut CodexAdapter, request_id: u64, client_id: &str) {
    let prompt = Prompt::new("prompt", "inspect").unwrap();
    let client = RequiredText::new("client", client_id).unwrap();
    adapter.prompt(request_id, &prompt, &client).unwrap();
}

#[test]
fn lexically_validates_only_the_exact_windows_packet_and_keeps_launch_fail_closed() {
    let bytes = PACKET_FIXTURE;
    let packet = parse_packet_metadata(bytes).unwrap();
    assert_eq!(packet.metadata().binding_id, "codex-app-server");
    assert_eq!(packet.metadata().codex_runtime_version, "0.145.0");
    assert_eq!(
        packet.metadata().codex_source_commit,
        "25af12f7e61572b0bc18ddb1008be543b91519b0"
    );
    assert_eq!(
        packet.metadata().app_server_schema_sha256,
        "7b0ab1679be705644a1b6c3f486f15e2f90e5f900486d90073b1dc5f0cf4c62c"
    );
    assert_eq!(packet.metadata().supporting_artifacts.len(), 7);
    assert_eq!(
        packet.require_physical_admission(),
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
        "C:\\{}\\CodexState\\codex-app-server-0.145.0-7b0ab167",
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
        r"{PACKAGE_ROOT}\CodexState\codex-app-server-0.145.0-7b0ab167"
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
            json!(6),
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
    changed["codexHome"] = json!(r"C:\SageHarnesses\CodexState\codex-app-server-0.145.0-7b0ab167");
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
    relocate_package_root(&mut changed, r"C:\SageHarnesses\Codex\0.145.0\package");
    relocate_codex_home(
        &mut changed,
        r"C:\SageHarnesses\CodexState\codex-app-server-0.145.0-7b0ab167",
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
#[ignore = "requires exact SAGE_CODEX_0145_PACKAGE_ROOT and SAGE_CODEX_0145_STATE_ROOT managed roots"]
fn exact_registry_package_and_state_satisfy_managed_prerequisite_without_launch_authority() {
    let package_root = std::env::var("SAGE_CODEX_0145_PACKAGE_ROOT")
        .expect("set SAGE_CODEX_0145_PACKAGE_ROOT to the extracted 0.145.0-win32-x64 package");
    let mut packet = packet_value();
    relocate_package_root(&mut packet, &package_root);
    let state_root = std::env::var("SAGE_CODEX_0145_STATE_ROOT")
        .expect("set SAGE_CODEX_0145_STATE_ROOT to the exact existing isolated state root");
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
fn builds_only_fixed_read_only_requests_and_single_root_ceiling() {
    let mut candidate = adapter();
    negotiate(&mut candidate);
    let root = candidate.project_root(r"C:\repo".into()).unwrap();
    let start = candidate.start(1, &root, None).unwrap();
    let value: Value = serde_json::from_slice(&start.bytes).unwrap();
    assert_eq!(value["method"], "thread/start");
    assert_eq!(value["params"]["sandbox"], "readOnly");
    assert_eq!(value["params"]["approvalPolicy"], "never");
    assert!(value["params"].get("runtimeWorkspaceRoots").is_none());
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
    assert_eq!(resume["params"]["sandbox"], "readOnly");
    let caps = candidate.effective_capabilities().unwrap();
    assert!(caps.new_session && caps.resume_session && caps.prompt && caps.cancel);
    assert!(caps.multi_root_extension_schema_id.is_none());
    assert!(!caps.native_session_fork && !caps.active_turn_steer);
    assert!(candidate.project_root(r"repo".into()).is_err());
    assert!(candidate.project_root(r"C:\repo\bad:name".into()).is_err());
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
    let missing = br#"{"id":1,"result":{"userAgent":"sage/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"}}"#;
    assert!(matches!(
        incomplete.ingest(missing, &Mapper),
        Err(AdapterError::Ingress(IngressError::CapabilityUnavailable))
    ));

    let mut negotiated = adapter();
    negotiate(&mut negotiated);
    let capabilities = negotiated.effective_capabilities().unwrap();
    assert!(capabilities.new_session && capabilities.prompt && capabilities.cancel);
    assert!(capabilities.collaboration_event_projection);
    assert!(!capabilities.native_session_fork && !capabilities.active_turn_steer);
    assert_eq!(
        capabilities.protocol_version.as_str(),
        "codex-app-server-0.145.0-7b0ab167"
    );

    for (pointer, replacement) in [
        (
            "/result/userAgent",
            json!("sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"),
        ),
        (
            "/result/userAgent",
            json!("Codex Desktop/0.145.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)"),
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
            .advance_permission_decline("approval-1", state)
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
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&unknown).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    let IncomingEvent::Unsupported(NormalizedPayload::Unsupported { external_kind, .. }) =
        &events[0]
    else {
        panic!()
    };
    assert_eq!(external_kind.as_str(), "[path]");

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
    let IngressDisposition::Events(events) = adapter
        .ingest(&serde_json::to_vec(&unknown).unwrap(), &Mapper)
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(&events[0], IncomingEvent::Unsupported(_)));
}

#[test]
fn unsupported_server_request_requires_runtime_teardown() {
    let mut adapter = adapter();
    ready(&mut adapter);
    arm_turn(&mut adapter, 2, "client-unsupported");
    let turn = json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}});
    adapter
        .ingest(&serde_json::to_vec(&turn).unwrap(), &Mapper)
        .unwrap();
    let request = json!({"id":"unsafe-1","method":"future/effectfulRequest","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"item-1"
    }});
    assert!(matches!(
        adapter.ingest(&serde_json::to_vec(&request).unwrap(), &Mapper),
        Err(AdapterError::Ingress(
            IngressError::UnsupportedServerRequest
        ))
    ));
}

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
            "last":{"inputTokens":3,"outputTokens":2}
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
                output_tokens: 2
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
