use super::*;
#[cfg(windows)]
use std::{ffi::OsStr, fs};

#[cfg(windows)]
use crate::{
    acp::{
        bindings::standard_acp::OpenCodeBindingRegistration,
        contract::{
            ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
        },
        process::RequiredContainment,
    },
    project_registry::ProjectRootAuthority,
};

#[test]
fn registry_resolves_claude_record_only_when_injectable_admission_is_present() {
    // This discriminates because the registry must route the Claude binding
    // and invoke its real registration, rather than merely cataloguing an ID.
    let registry = BindingRegistry::default();
    assert!(
        matches!(registry.route(CLAUDE_BINDING_ID), Ok(record) if record.harness_id() == "claude-code")
    );
}

#[cfg(windows)]
#[test]
fn opencode_registry_resolution_constructs_an_exact_direct_native_launch() {
    let root =
        std::env::temp_dir().join(format!("sage-opencode-registry-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let shim = npm.join("opencode.cmd");
    let executable = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
    fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
    fs::write(&executable, b"native-opencode").expect("write runtime");
    fs::write(
        npm.join("node_modules")
            .join("opencode-ai")
            .join("package.json"),
        r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .expect("write manifest");
    let registration = OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21");
    let registry = BindingRegistry::new_with_opencode_for_test(
        CodexBindingRegistration::unavailable_for_test(),
        ClaudeBindingRegistration::absent_for_test(),
        registration,
    );
    let shim_marker = root.join("shim-ran");
    fs::write(
        &shim,
        format!(
            "@echo off\r\necho executed>\"{}\"\r\necho 1.18.21\r\n",
            shim_marker.display()
        ),
    )
    .expect("replace shim after admission");
    let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
    let (expected_binding_digest, expected_artifact_digest) = {
        let identity = admitted.identity();
        (identity.binding_digest, identity.artifact_digest.to_owned())
    };
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-opencode-registry").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-opencode-registry").expect("root"),
        &root,
    )
    .expect("project root");
    let mut runtime = admitted
        .into_factory(
            BindingProjectRoot::StandardAcp(project_root),
            crate::acp::mcp::McpAdmission::default(),
        )
        .construct(
            RuntimeEpoch::new(17).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-registry").expect("session"),
        )
        .expect("construct runtime");
    let spec = runtime.launch_spec().expect("launch spec");

    assert_eq!(spec.binding_id.as_bytes(), &expected_binding_digest[..16]);
    assert_eq!(spec.artifact.canonical_absolute_path, executable);
    assert_eq!(
        spec.artifact
            .sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        expected_artifact_digest
    );
    assert_eq!(spec.fixed_args.as_slice(), [OsStr::new("acp")]);
    let mut expected_environment: Vec<_> = [
        "APPDATA",
        "ComSpec",
        "LOCALAPPDATA",
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "WINDIR",
    ]
    .into_iter()
    .filter_map(|key| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(|value| (std::ffi::OsString::from(key), value))
    })
    .collect();
    let (policy_key, policy_content) = spec
        .environment
        .as_slice()
        .last()
        .expect("OpenCode policy environment");
    assert_eq!(policy_key, "OPENCODE_CONFIG_CONTENT");
    let policy_text = policy_content.to_str().expect("UTF-8 OpenCode policy");
    let policy: serde_json::Value = serde_json::from_str(policy_text).expect("policy JSON");
    let expected_policy = serde_json::json!({"permission":{
        "*":"ask", "bash":"ask", "edit":"ask", "read":"allow",
        "task":"ask", "webfetch":"ask", "write":"ask"
    }});
    assert_eq!(policy, expected_policy);
    assert_eq!(
        policy_text,
        serde_json::to_string(&expected_policy).unwrap()
    );
    expected_environment.push((
        std::ffi::OsString::from("OPENCODE_CONFIG_CONTENT"),
        std::ffi::OsString::from(policy_text),
    ));
    assert_eq!(spec.environment.as_slice(), expected_environment);
    assert_eq!(spec.required_guarantee, RequiredContainment::EnforcedTree);
    assert!(!shim_marker.exists());
    drop(spec);
    fs::remove_dir_all(root).expect("remove root");
}

#[cfg(windows)]
#[test]
fn opencode_factory_refuses_runtime_drift_immediately_before_launch() {
    let root = std::env::temp_dir().join(format!(
        "sage-opencode-registry-drift-{}",
        uuid::Uuid::new_v4()
    ));
    let npm = root.join("npm");
    let shim = npm.join("opencode.cmd");
    let executable = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
    fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
    fs::write(&executable, b"native-opencode").expect("write runtime");
    fs::write(
        npm.join("node_modules")
            .join("opencode-ai")
            .join("package.json"),
        r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .expect("write manifest");
    let registry = BindingRegistry::new_with_opencode_for_test(
        CodexBindingRegistration::unavailable_for_test(),
        ClaudeBindingRegistration::absent_for_test(),
        OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21"),
    );
    let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-opencode-registry-drift").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-opencode-registry-drift").expect("root"),
        &root,
    )
    .expect("project root");
    let mut runtime = admitted
        .into_factory(
            BindingProjectRoot::StandardAcp(project_root),
            crate::acp::mcp::McpAdmission::default(),
        )
        .construct(
            RuntimeEpoch::new(18).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-registry-drift").expect("session"),
        )
        .expect("construct runtime");
    fs::write(&executable, b"changed-opencode").expect("change runtime");

    assert!(matches!(
        runtime.launch_spec(),
        Err(crate::acp::bindings::adapter::AdapterRuntimeError::LaunchUnavailable)
    ));
    fs::remove_dir_all(root).expect("remove root");
}

#[cfg(windows)]
#[test]
fn admitted_opencode_offers_the_observed_mandatory_session_core() {
    let root = std::env::temp_dir().join(format!(
        "sage-opencode-mandatory-core-{}",
        uuid::Uuid::new_v4()
    ));
    let npm = root.join("npm");
    let shim = npm.join("opencode.cmd");
    let executable = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
    fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
    fs::write(&executable, b"native-opencode").expect("write runtime");
    fs::write(
        npm.join("node_modules")
            .join("opencode-ai")
            .join("package.json"),
        r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .expect("write manifest");
    let registry = BindingRegistry::new_with_opencode_for_test(
        CodexBindingRegistration::unavailable_for_test(),
        ClaudeBindingRegistration::absent_for_test(),
        OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21"),
    );
    let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
    let snapshot = admitted
        .supported_capabilities()
        .expect("an admitted OpenCode build projects the mandatory session core");

    assert!(snapshot.new_session);
    assert!(snapshot.prompt);
    assert!(snapshot.cancel);
    assert!(snapshot.close_session);
    assert!(!snapshot.resume_session);
    assert!(!snapshot.load_session);
    assert_eq!(snapshot.model_count, 0);
    assert_eq!(snapshot.config_count, 0);
    assert!(
        registry
            .route("opencode-acp")
            .expect("route OpenCode")
            .session_operations_enabled()
    );
    drop(admitted);
    fs::remove_dir_all(root).expect("remove root");
}
