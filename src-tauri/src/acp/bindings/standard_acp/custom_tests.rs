use std::{ffi::OsStr, path::PathBuf, sync::Arc};

use super::super::StandardAcpRuntimeFactory;
use super::*;
use crate::{
    acp::{
        bindings::runtime::RuntimeFactory,
        contract::{
            ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
        },
    },
    project_registry::ProjectRootAuthority,
};

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(label: &str) -> TempRoot {
    let root = crate::test_paths::temp_dir()
        .join(format!("sage-custom-acp-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("temp root");
    TempRoot(root.canonicalize().expect("canonical root"))
}

fn agent(executable: &std::path::Path, args: &[&str], env: &[&str]) -> CustomAcpAgent {
    CustomAcpAgent {
        id: "custom-acp-2".into(),
        name: "My agent".into(),
        executable: executable.to_string_lossy().into_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        env: env.iter().map(|name| (*name).to_owned()).collect(),
    }
}

fn registration() -> CustomAcpRegistration {
    CustomAcpRegistration::startup(2).expect("slot two")
}

#[test]
fn a_slot_is_pending_until_settings_apply_then_admits_a_regular_file() {
    let root = temp("admit");
    let executable = root.0.join("agent.exe");
    std::fs::write(&executable, b"synthetic agent").unwrap();
    let registration = registration();
    assert_eq!(registration.profile().harness_id, "custom-acp-2");
    assert_eq!(
        registration.unavailable(),
        Some(CustomAcpUnavailable::Pending)
    );
    registration.apply(Some(&agent(&executable, &["--acp"], &[])));
    let binding = registration.admitted().expect("admitted");
    assert_eq!(binding.harness_version(), "");
    assert_eq!(binding.extension().args, ["--acp"]);
    assert_eq!(registration.display_name().as_deref(), Some("My agent"));
    registration.apply(None);
    assert_eq!(
        registration.unavailable(),
        Some(CustomAcpUnavailable::NotConfigured)
    );
}

#[test]
fn a_missing_file_or_a_directory_is_never_admitted() {
    let root = temp("refuse");
    let registration = registration();
    registration.apply(Some(&agent(&root.0.join("absent.exe"), &[], &[])));
    assert_eq!(
        registration.unavailable(),
        Some(CustomAcpUnavailable::ExecutableInvalid)
    );
    registration.apply(Some(&agent(&root.0, &[], &[])));
    assert_eq!(
        registration.unavailable(),
        Some(CustomAcpUnavailable::ExecutableInvalid)
    );
    assert!(registration.refresh().is_err());
}

#[test]
fn arguments_and_passed_names_are_part_of_the_binding_identity() {
    let root = temp("digest");
    let executable = root.0.join("agent.exe");
    std::fs::write(&executable, b"synthetic agent").unwrap();
    let digest = |args: &[&str], env: &[&str]| {
        let registration = registration();
        registration.apply(Some(&agent(&executable, args, env)));
        registration.admitted().expect("admitted").binding_digest()
    };
    let plain = digest(&[], &[]);
    assert_ne!(plain, digest(&["--acp"], &[]));
    assert_ne!(plain, digest(&[], &["AGENT_KEY"]));
    assert_ne!(digest(&["AGENT_KEY"], &[]), digest(&[], &["AGENT_KEY"]));
    assert_eq!(plain, digest(&[], &[]));
}

#[test]
fn an_executable_rebuilt_in_place_is_admitted_again_by_its_new_digest() {
    let root = temp("rebuild");
    let executable = root.0.join("agent.exe");
    std::fs::write(&executable, b"first build").unwrap();
    let registration = registration();
    registration.apply(Some(&agent(&executable, &[], &[])));
    let first = registration.refresh().expect("first").binding_digest();
    std::fs::write(&executable, b"second build, longer").unwrap();
    let second = registration
        .refresh()
        .expect("re-admitted")
        .binding_digest();
    assert_ne!(first, second);
    std::fs::remove_file(&executable).unwrap();
    assert_eq!(
        registration.refresh().err(),
        Some(CustomAcpUnavailable::ExecutableInvalid)
    );
}

#[test]
fn the_launch_runs_the_file_directly_with_the_configured_arguments_and_names() {
    let root = temp("launch");
    let executable = root.0.join("agent.exe");
    std::fs::write(&executable, b"synthetic agent").unwrap();
    let registration = Arc::new(registration());
    registration.apply(Some(&agent(
        &executable,
        &["--acp", "--quiet"],
        &["SAGE_CUSTOM_ACP_TEST_ABSENT_NAME"],
    )));
    let binding = registration.admitted().expect("admitted");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse("project-custom-acp").unwrap(),
        ProjectGeneration::ZERO,
        SageRootId::parse("root-custom-acp").unwrap(),
        &root.0,
    )
    .expect("project root");
    let mut runtime = Box::new(StandardAcpRuntimeFactory::from_admitted_binding(
        registration,
        binding,
        project_root,
    ))
    .construct(
        RuntimeEpoch::new(3).unwrap(),
        SageAgentSessionId::parse("session-custom-acp").unwrap(),
    )
    .expect("runtime");
    let spec = runtime.launch_spec().expect("launch spec");
    assert_eq!(spec.artifact.canonical_absolute_path, executable);
    assert_eq!(
        spec.fixed_args.as_slice(),
        [OsStr::new("--acp"), OsStr::new("--quiet")]
    );
    assert!(
        spec.environment
            .as_slice()
            .iter()
            .all(|(key, _)| key != "SAGE_CUSTOM_ACP_TEST_ABSENT_NAME"),
        "an unset passed-through name adds nothing"
    );
}

#[test]
fn a_passed_through_name_carries_the_value_read_at_launch() {
    let profile = CUSTOM_PROFILES.first().expect("profile");
    let extension = StandardAcpLaunchExtension {
        args: Vec::new(),
        passthrough_env: vec!["AGENT_KEY".into(), "UNSET_NAME".into()],
    };
    let entries =
        super::super::runtime::launch_environment_for_test(profile, &extension, |key| match key {
            "AGENT_KEY" => Some("value-at-launch".into()),
            "SystemRoot" => Some(r"C:\Windows".into()),
            _ => None,
        });
    assert!(
        entries
            .iter()
            .any(|(key, value)| key == "AGENT_KEY" && value == "value-at-launch")
    );
    assert!(entries.iter().all(|(key, _)| key != "UNSET_NAME"));
}
