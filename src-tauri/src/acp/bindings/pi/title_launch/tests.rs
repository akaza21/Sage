use super::*;
use crate::acp::process::{
    ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, ProcessDeadlines, RequiredContainment,
    TreeLimits,
};
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId, SageRootId};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

#[test]
fn captured_launch_preserves_account_and_model_and_refuses_changed_authority() {
    let _process = super::super::title_request::tests::PROCESS_LOCK
        .lock()
        .unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../work")
        .join(format!("pi-title-authority-{}", uuid::Uuid::new_v4()));
    let agent = root.join(".pi/agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("auth.json"),
        br#"{"fixture":{"type":"api_key","key":"fixture-only"}}"#,
    )
    .unwrap();
    let settings = br#"{"retry":{"enabled":true},"compaction":{"enabled":true}}"#;
    std::fs::write(agent.join("settings.json"), settings).unwrap();
    let node = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(node.status.success());
    let node = PathBuf::from(String::from_utf8(node.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let mut environment: Vec<_> = ["PATH", "SystemRoot"]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
        .collect();
    environment.extend([
        (OsString::from(home_key), root.clone().into_os_string()),
        (
            OsString::from("TITLE_FIXTURE_SCENARIO"),
            OsString::from("success"),
        ),
        (
            OsString::from("TITLE_FIXTURE_PROFILE"),
            agent.clone().into_os_string(),
        ),
    ]);
    let spec = LaunchSpec {
        binding_id: uuid::Uuid::nil(),
        runtime_epoch: 1,
        artifact: ArtifactExpectation::new(uuid::Uuid::nil(), node, [0; 32], 1).unwrap(),
        fixed_args: BoundedOsArgs::new(vec![
            OsString::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/acp/bindings/pi/title_request/fixture.mjs"
            )),
            OsString::from("--session"),
            root.join("main-session.jsonl").into_os_string(),
        ])
        .unwrap(),
        environment: BoundedEnvironment::new(environment).unwrap(),
        project_root: crate::project_registry::ProjectRootAuthority::for_test(
            SageProjectId::parse("project-title").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-title").unwrap(),
            &root,
        )
        .unwrap(),
        required_guarantee: RequiredContainment::TrustedCooperativeTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        #[cfg(windows)]
        test_after_artifact_verified: None,
    };
    let model = super::super::ModelSummary {
        provider: "fixture".into(),
        id: "same-model".into(),
        name: "Model".into(),
        context_window: None,
    };
    let launch = TitleLaunch::capture(&spec).unwrap();
    let changed = TitleLaunch::capture(&spec).unwrap();
    let history = root.join("main-session.jsonl");
    let mut main_command = Command::new(&spec.artifact.canonical_absolute_path);
    main_command
        .args(spec.fixed_args.as_slice())
        .env_clear()
        .envs(spec.environment.as_slice().iter().cloned())
        .env("TITLE_FIXTURE_SCENARIO", "main");
    let mut main =
        super::super::title_request::TitleRequest::start(main_command, "original main prompt")
            .unwrap();
    assert!(finish(&mut main).is_some());
    drop(main);
    let original = std::fs::read(&history).unwrap();
    assert!(String::from_utf8_lossy(&original).contains("original main prompt"));
    let mut request = super::super::title_request::TitleRequest::start(
        launch.command(&model).unwrap(),
        "first exchange",
    )
    .unwrap();
    let result = finish(&mut request);
    assert_eq!(result.as_deref(), Some("Check Sage chat titles"));
    drop(request);
    // The same captured authority must support a later admitted title try.
    let mut retry = super::super::title_request::TitleRequest::start(
        launch.command(&model).unwrap(),
        "first exchange after foreground preemption",
    )
    .unwrap();
    assert_eq!(
        finish(&mut retry).as_deref(),
        Some("Check Sage chat titles")
    );
    drop(retry);
    assert_eq!(std::fs::read(&history).unwrap(), original);
    assert_eq!(
        std::fs::read(agent.join("settings.json")).unwrap(),
        settings
    );
    std::fs::write(
        agent.join("auth.json"),
        br#"{"fixture":{"type":"api_key","key":"changed-account"}}"#,
    )
    .unwrap();
    assert!(changed.command(&model).is_err());
    drop(spec);
    std::fs::remove_dir_all(root).unwrap();
}

fn finish(request: &mut super::super::title_request::TitleRequest) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = request.poll() {
            return match result {
                super::super::title_request::TitleOutcome::Generated(title) => Some(title),
                super::super::title_request::TitleOutcome::Failed
                | super::super::title_request::TitleOutcome::Preempted => None,
            };
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(5));
    }
}
