use std::{fs, path::PathBuf};

use super::{
    admission::{StandardAcpArtifact, StandardAcpArtifactProvenance, StandardAcpCandidate, admit},
    environment::passthrough,
    handshake::{StandardAcpHandshake, StandardAcpHandshakeError},
    launch::{AcpLaunchAuthority, ClaudeLaunch},
    opencode::candidate_from_global_npm_shim,
    profile::{StandardAcpLaunch, profile_for},
};
use crate::{
    acp::{
        bindings::adapter::AdapterRuntimeError,
        contract::{ProjectGeneration, SageProjectId, SageRootId},
        process::BoundedEnvironment,
    },
    project_registry::ProjectRootAuthority,
};

fn artifact(path: PathBuf) -> StandardAcpArtifact {
    let sha256 = crate::harness_discovery::sha256_file(&path).expect("hash artifact");
    let byte_len = fs::metadata(&path).expect("artifact metadata").len();
    StandardAcpArtifact {
        path,
        sha256,
        byte_len,
        provenance: StandardAcpArtifactProvenance::Direct,
    }
}

fn write_opencode_manifest(npm: &std::path::Path, version: &str, binary: &str) {
    fs::write(
        npm.join("node_modules")
            .join("opencode-ai")
            .join("package.json"),
        format!(
            r#"{{"name":"opencode-ai","version":"{version}","bin":{{"opencode":"{binary}"}}}}"#
        ),
    )
    .expect("write package manifest");
}

fn npm_artifact(npm: &std::path::Path, relative: &str) -> StandardAcpArtifact {
    let path = npm.join(relative);
    StandardAcpArtifact {
        sha256: crate::harness_discovery::sha256_file(&path).expect("hash artifact"),
        byte_len: fs::metadata(&path).expect("artifact metadata").len(),
        path,
        provenance: StandardAcpArtifactProvenance::NpmGlobal {
            root: npm.to_path_buf(),
            relative: PathBuf::from(relative),
        },
    }
}

fn reader(
    entries: &'static [(&'static str, &'static str)],
) -> impl Fn(&str) -> Option<std::ffi::OsString> {
    move |key| {
        entries
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| std::ffi::OsString::from(*value))
    }
}

#[test]
fn gemini_passes_through_the_project_selection_a_workspace_account_needs() {
    let profile = profile_for("gemini-cli").expect("Gemini profile");
    let resolved = passthrough(
        profile,
        reader(&[
            ("GOOGLE_CLOUD_PROJECT", "sage-dev"),
            ("GEMINI_API_KEY", "key"),
            ("UNRELATED", "value"),
        ]),
    );
    assert_eq!(
        resolved,
        vec![
            (
                std::ffi::OsString::from("GOOGLE_CLOUD_PROJECT"),
                std::ffi::OsString::from("sage-dev")
            ),
            (
                std::ffi::OsString::from("GEMINI_API_KEY"),
                std::ffi::OsString::from("key")
            ),
        ]
    );
}

#[test]
fn claude_reuses_its_existing_login_instead_of_passing_through_credentials() {
    let profile = profile_for("claude-code").expect("Claude profile");
    assert!(profile.passthrough_env.is_empty());
    let resolved = passthrough(
        profile,
        reader(&[
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:8317"),
            ("ANTHROPIC_AUTH_TOKEN", "token"),
        ]),
    );
    assert!(resolved.is_empty());
}

#[test]
fn an_unset_passthrough_variable_is_absent_rather_than_empty() {
    let profile = profile_for("gemini-cli").expect("Gemini profile");
    let resolved = passthrough(profile, reader(&[("GOOGLE_CLOUD_PROJECT", "")]));
    assert!(resolved.is_empty());
}

#[test]
fn every_profile_offers_remediation_and_passes_through_only_its_own_variables() {
    for profile in super::PROFILES {
        assert!(!profile.remediation.is_empty());
        assert!(profile.remediation.len() <= 512);
        let mut seen = std::collections::BTreeSet::new();
        for key in profile.passthrough_env {
            assert!(!key.is_empty());
            assert!(seen.insert(*key), "{} repeats {key}", profile.harness_id);
        }
    }
}

#[test]
fn gemini_profile_truthfully_advertises_filesystem_capabilities() {
    let profile = profile_for("gemini-cli").expect("Gemini profile");
    assert!(profile.client_capabilities.read_text_file);
    assert!(!profile.client_capabilities.write_text_file);
    assert_eq!(profile.fixed_args, ["--experimental-acp"]);
}

#[test]
fn opencode_profile_advertises_only_implemented_client_file_capabilities() {
    let profile = profile_for("opencode").expect("OpenCode profile");
    assert!(!profile.client_capabilities.read_text_file);
    assert!(profile.client_capabilities.write_text_file);
}

#[test]
fn cursor_profile_carries_its_sign_in_guidance() {
    let profile = profile_for("cursor-agent").expect("Cursor profile");
    assert_eq!(
        profile.auth_hint().expect("Cursor sign-in").command,
        "cursor-agent login"
    );
    assert_eq!(profile.fixed_args, ["acp"]);
}

#[test]
fn node_profile_refuses_a_candidate_without_an_entrypoint() {
    let root = std::env::temp_dir().join(format!("sage-standard-acp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    let runtime_path = root.join("node.exe");
    fs::write(&runtime_path, b"runtime").expect("write runtime");
    let candidate = StandardAcpCandidate {
        runtime: artifact(runtime_path),
        entrypoint: None,
        package_manifest: None,
        reported_version: "0.1.22".into(),
    };
    let profile = profile_for("gemini-cli").expect("Gemini profile");
    assert_eq!(profile.launch, StandardAcpLaunch::NodeEntrypoint);
    assert!(admit(profile, candidate).is_err());
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn admission_refuses_a_runtime_whose_recorded_size_does_not_match() {
    let root = std::env::temp_dir().join(format!("sage-standard-acp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    let runtime_path = root.join("opencode.exe");
    fs::write(&runtime_path, b"runtime").expect("write runtime");
    let mut runtime = artifact(runtime_path);
    runtime.byte_len = runtime.byte_len.saturating_add(1);
    let candidate = StandardAcpCandidate {
        runtime,
        entrypoint: None,
        package_manifest: None,
        reported_version: "1.18.21".into(),
    };

    assert!(
        admit(
            profile_for("opencode").expect("OpenCode profile"),
            candidate
        )
        .is_err()
    );
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn a_profile_without_a_manifest_contract_refuses_a_candidate_that_supplies_one() {
    let root = std::env::temp_dir().join(format!("sage-standard-acp-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    fs::create_dir_all(npm.join("node_modules").join("opencode-ai")).expect("create layout");
    let runtime_path = root.join("node.exe");
    let entrypoint_path = root.join("cli.js");
    fs::write(&runtime_path, b"runtime").expect("write runtime");
    fs::write(&entrypoint_path, b"entrypoint").expect("write entrypoint");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let profile = profile_for("gemini-cli").expect("Gemini profile");
    assert!(profile.package_manifest.is_none());
    let candidate = StandardAcpCandidate {
        runtime: artifact(runtime_path),
        entrypoint: Some(artifact(entrypoint_path)),
        package_manifest: Some(npm_artifact(&npm, "node_modules/opencode-ai/package.json")),
        reported_version: "0.1.22".into(),
    };

    assert!(matches!(
        admit(profile, candidate),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn opencode_candidate_comes_only_from_the_native_executable_under_its_global_npm_shim() {
    let root = std::env::temp_dir().join(format!("sage-opencode-layout-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    fs::write(&runtime, b"native-opencode").expect("write runtime");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");

    let candidate = candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate");

    assert_eq!(candidate.runtime.path, runtime);
    assert_eq!(candidate.entrypoint, None);
    assert_eq!(candidate.runtime.byte_len, b"native-opencode".len() as u64);
    assert_eq!(
        profile_for("opencode")
            .expect("OpenCode profile")
            .fixed_args,
        ["acp"]
    );
    assert!(
        admit(
            profile_for("opencode").expect("OpenCode profile"),
            candidate
        )
        .is_ok()
    );
    assert!(candidate_from_global_npm_shim(&shim, "1.18.20".into()).is_none());
    write_opencode_manifest(&npm, "1.18.21", "./bin/other.exe");
    assert!(candidate_from_global_npm_shim(&shim, "1.18.21".into()).is_none());
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    fs::remove_file(&runtime).expect("remove runtime");
    assert!(candidate_from_global_npm_shim(&shim, "1.18.21".into()).is_none());
    fs::write(&runtime, b"native-opencode").expect("restore runtime");
    fs::remove_file(&shim).expect("remove shim");
    assert!(candidate_from_global_npm_shim(&shim, "1.18.21".into()).is_none());
    fs::write(&shim, b"shim").expect("restore shim");
    #[cfg(windows)]
    {
        let reparse_target = npm.join("outside-opencode.exe");
        fs::write(&reparse_target, b"outside").expect("write reparse target");
        fs::remove_file(&runtime).expect("remove runtime for reparse");
        if std::os::windows::fs::symlink_file(&reparse_target, &runtime).is_ok() {
            assert!(candidate_from_global_npm_shim(&shim, "1.18.21".into()).is_none());
            fs::remove_file(&runtime).expect("remove runtime reparse");
        }
        fs::write(&runtime, b"native-opencode").expect("restore runtime after reparse");
        let shim_target = npm.join("outside-opencode.cmd");
        fs::write(&shim_target, b"outside").expect("write shim reparse target");
        fs::remove_file(&shim).expect("remove shim for reparse");
        if std::os::windows::fs::symlink_file(&shim_target, &shim).is_ok() {
            assert!(candidate_from_global_npm_shim(&shim, "1.18.21".into()).is_none());
            fs::remove_file(&shim).expect("remove shim reparse");
        }
    }
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn admitted_opencode_binding_rejects_runtime_or_manifest_drift() {
    let root = std::env::temp_dir().join(format!("sage-opencode-drift-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    fs::write(&runtime, b"native-opencode").expect("write runtime");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let binding = admit(
        profile_for("opencode").expect("OpenCode profile"),
        candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate"),
    )
    .expect("binding");

    fs::write(&runtime, b"changed-opencode").expect("change runtime");
    assert!(binding.revalidate().is_err());
    fs::write(&runtime, b"native-opencode").expect("restore runtime");
    write_opencode_manifest(&npm, "1.18.22", "./bin/opencode.exe");
    assert!(binding.revalidate().is_err());
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn hand_built_opencode_candidates_cannot_bypass_the_manifest_contract() {
    let root =
        std::env::temp_dir().join(format!("sage-opencode-manifest-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    fs::write(&runtime, b"native-opencode").expect("write runtime");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let profile = profile_for("opencode").expect("OpenCode profile");
    let candidate = candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate");

    let mut missing = candidate.clone();
    missing.package_manifest = None;
    assert!(matches!(
        admit(profile, missing),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));

    let mut forged_hash = candidate.clone();
    forged_hash
        .package_manifest
        .as_mut()
        .expect("manifest")
        .sha256 = "a".repeat(64);
    assert!(matches!(
        admit(profile, forged_hash),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));

    let mut forged_size = candidate.clone();
    forged_size
        .package_manifest
        .as_mut()
        .expect("manifest")
        .byte_len = 1;
    assert!(matches!(
        admit(profile, forged_size),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));

    let mut wrong_folder = candidate.clone();
    wrong_folder
        .package_manifest
        .as_mut()
        .expect("manifest")
        .provenance = StandardAcpArtifactProvenance::NpmGlobal {
        root: npm.clone(),
        relative: PathBuf::from("node_modules/other/package.json"),
    };
    assert!(admit(profile, wrong_folder).is_err());

    for (name, version, binary) in [
        ("other", "1.18.21", "./bin/opencode.exe"),
        ("opencode-ai", "1.18.21", "./bin/other.exe"),
        ("opencode-ai", "1.18.22", "./bin/opencode.exe"),
    ] {
        fs::write(
            npm.join("node_modules")
                .join("opencode-ai")
                .join("package.json"),
            format!(r#"{{"name":"{name}","version":"{version}","bin":{{"opencode":"{binary}"}}}}"#),
        )
        .expect("write forged package manifest");
        let mut forged = candidate.clone();
        forged.package_manifest = Some(npm_artifact(&npm, "node_modules/opencode-ai/package.json"));
        assert!(matches!(
            admit(profile, forged),
            Err(super::StandardAcpAdmissionError::Manifest)
        ));
    }
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn hand_built_opencode_candidates_cannot_substitute_the_manifest_runtime() {
    let root = std::env::temp_dir().join(format!(
        "sage-opencode-runtime-link-{}",
        uuid::Uuid::new_v4()
    ));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    let sibling_runtime = npm
        .join("node_modules")
        .join("other")
        .join("bin")
        .join("opencode.exe");
    let other_npm = root.join("other-npm");
    let other_runtime = other_npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    for path in [&runtime, &sibling_runtime, &other_runtime] {
        fs::create_dir_all(path.parent().expect("runtime parent")).expect("create layout");
        fs::write(path, b"native-opencode").expect("write runtime");
    }
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let profile = profile_for("opencode").expect("OpenCode profile");
    let candidate = candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate");

    let mut direct_runtime = candidate.clone();
    direct_runtime.runtime = artifact(runtime.clone());
    assert!(matches!(
        admit(profile, direct_runtime),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));

    let mut sibling_package_runtime = candidate.clone();
    sibling_package_runtime.runtime = npm_artifact(&npm, "node_modules/other/bin/opencode.exe");
    assert!(matches!(
        admit(profile, sibling_package_runtime),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));

    let mut other_global_runtime = candidate;
    other_global_runtime.runtime =
        npm_artifact(&other_npm, "node_modules/opencode-ai/bin/opencode.exe");
    assert!(matches!(
        admit(profile, other_global_runtime),
        Err(super::StandardAcpAdmissionError::Manifest)
    ));
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn admitted_direct_binding_owns_its_artifact_identity_and_revalidates_before_launch() {
    let root = std::env::temp_dir().join(format!("sage-standard-acp-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    fs::write(&runtime, b"runtime").expect("write runtime");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let binding = admit(
        profile_for("opencode").expect("OpenCode profile"),
        candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate"),
    )
    .expect("admit binding");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-standard-acp-admitted").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-standard-acp-admitted").expect("root"),
        &root,
    )
    .expect("project root");
    let spec = AcpLaunchAuthority::from_admitted_binding(
        &binding,
        10,
        BoundedEnvironment::new(Vec::new()).expect("environment"),
    )
    .expect("authority")
    .into_launch_spec(project_root)
    .expect("launch spec");

    assert_eq!(spec.artifact.canonical_absolute_path, runtime);
    assert_ne!(spec.binding_id, spec.artifact.artifact_id);
    assert_eq!(spec.fixed_args.as_slice(), [std::ffi::OsStr::new("acp")]);
    drop(spec);
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn generic_runtime_maps_standard_auth_refusal_without_a_harness_branch() {
    let profile = profile_for("cursor-agent").expect("Cursor profile");
    let mut runtime = StandardAcpHandshake::new(profile);
    runtime.initialize().expect("initialize request");
    runtime
        .accept_initialize(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "protocolVersion": 1 }
        }))
        .expect("initialize response");
    runtime
        .new_session("C:/Sage-build/acp-probe")
        .expect("session request");
    assert_eq!(
        runtime.accept_new_session(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "code": -32000, "message": "Authentication required" }
        })),
        Err(StandardAcpHandshakeError::SignInRequired)
    );
}

#[test]
fn launch_authority_preserves_node_entrypoint_then_profile_arguments() {
    let root =
        std::env::temp_dir().join(format!("sage-standard-acp-launch-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    let runtime = root.join("node.exe");
    let entrypoint = root.join("adapter.js");
    fs::write(&runtime, b"runtime").expect("write runtime");
    fs::write(&entrypoint, b"entrypoint").expect("write entrypoint");
    let binding = admit(
        profile_for("gemini-cli").expect("Gemini profile"),
        StandardAcpCandidate {
            runtime: artifact(runtime.clone()),
            entrypoint: Some(artifact(entrypoint.clone())),
            package_manifest: None,
            reported_version: "0.1.22".into(),
        },
    )
    .expect("binding");
    let authority = AcpLaunchAuthority::from_admitted_binding(
        &binding,
        7,
        BoundedEnvironment::new(Vec::new()).expect("environment"),
    )
    .expect("launch authority");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-standard-acp-launch").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-standard-acp-launch").expect("root"),
        &root,
    )
    .expect("project root");
    let spec = authority
        .into_launch_spec(project_root)
        .expect("launch spec");
    assert_eq!(spec.runtime_epoch, 7);
    assert_eq!(spec.artifact.canonical_absolute_path, runtime);
    assert_eq!(
        spec.fixed_args
            .as_slice()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec![
            entrypoint.to_string_lossy().into_owned(),
            "--experimental-acp".into()
        ]
    );
    drop(spec);
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn launch_authority_preserves_direct_profile_arguments_without_an_entrypoint() {
    let root =
        std::env::temp_dir().join(format!("sage-standard-acp-launch-{}", uuid::Uuid::new_v4()));
    let npm = root.join("npm");
    let runtime = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    fs::write(&runtime, b"runtime").expect("write runtime");
    write_opencode_manifest(&npm, "1.18.21", "./bin/opencode.exe");
    let binding = admit(
        profile_for("opencode").expect("OpenCode profile"),
        candidate_from_global_npm_shim(&shim, "1.18.21".into()).expect("candidate"),
    )
    .expect("binding");
    let authority = AcpLaunchAuthority::from_admitted_binding(
        &binding,
        8,
        BoundedEnvironment::new(Vec::new()).expect("environment"),
    )
    .expect("launch authority");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-standard-acp-direct").expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-standard-acp-direct").expect("root"),
        &root,
    )
    .expect("project root");
    let spec = authority
        .into_launch_spec(project_root)
        .expect("launch spec");
    assert_eq!(spec.runtime_epoch, 8);
    assert_eq!(spec.artifact.canonical_absolute_path, runtime);
    assert_eq!(
        spec.fixed_args
            .as_slice()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["acp"]
    );
    drop(spec);
    fs::remove_dir_all(root).expect("remove root");
}

#[test]
fn launch_authority_rejects_malformed_or_non_ascii_digests_without_panicking() {
    for digest in [
        "a".repeat(63),
        "g".repeat(64),
        format!("a{}a", "é".repeat(31)),
    ] {
        let result = std::panic::catch_unwind(|| {
            AcpLaunchAuthority::for_claude(ClaudeLaunch {
                binding_digest: [11; 32],
                runtime_epoch: 9,
                runtime_path: PathBuf::from("C:/sage/node.exe"),
                runtime_sha256: digest,
                runtime_byte_len: 0,
                entrypoint: PathBuf::from("C:/sage/adapter.js"),
                environment: BoundedEnvironment::new(Vec::new()).expect("environment"),
            })
        });
        assert!(matches!(
            result,
            Ok(Err(AdapterRuntimeError::LaunchUnavailable))
        ));
    }
}
