use super::*;

struct CreatedFileMapper(PathBuf);

impl ResourceMapper for CreatedFileMapper {
    fn map_existing(
        &self,
        path: &str,
    ) -> Result<
        sage_acp_protocol::intent::SafeResourceRef,
        sage_acp_protocol::intent::UnavailableResourceReason,
    > {
        if path != self.0.to_str().expect("target path") || !self.0.is_file() {
            return Err(sage_acp_protocol::intent::UnavailableResourceReason::Unsupported);
        }
        sage_acp_protocol::intent::SafeResourceRef::new(
            SageRootId::parse("root-created-file").expect("root id"),
            "captured.txt",
        )
        .map_err(|_| sage_acp_protocol::intent::UnavailableResourceReason::Unsupported)
    }
}

#[test]
fn captured_created_file_is_published_only_after_approved_write_and_completion() {
    let root = TempRoot(temp_root().join(format!(
        "sage-opencode-created-file-{}",
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir_all(&root.0).expect("create root");
    let target = root.0.join("captured.txt");
    let mapper = CreatedFileMapper(target.clone());
    let path = target.to_str().expect("target path");
    let escaped = serde_json::to_string(path).expect("escaped path");
    let fixture: Value = serde_json::from_str(
        &include_str!("../../testdata/opencode_real_allow_write_20260929.json")
            .replace("__TARGET__", &escaped[1..escaped.len() - 1]),
    )
    .expect("captured fixture");
    let mut runtime = ready_runtime(&root, 61, "created-file", OPENCODE_OBSERVED_VERSION);
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "create captured file").expect("prompt"),
            client_message_id: RequiredText::new("client", "created-file").expect("client"),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        })
        .expect("prompt command");
    runtime
        .resolve_egress(prompt.into_token(), EgressResolution::Written)
        .expect("prompt written");
    for name in ["toolStart", "toolProgress"] {
        let batch = runtime
            .ingest(&frame(fixture[name].clone()), &mapper)
            .expect("tool update");
        assert!(!has_file_change(batch.events()));
    }
    let permission = runtime
        .ingest(&frame(fixture["permission"].clone()), &mapper)
        .expect("permission");
    assert!(
        permission
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
    );
    assert!(!has_file_change(permission.events()));
    let approval = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: scoped_permission_id("created-file", 61),
            decision: sage_acp_protocol::permissions::PermissionDecision::AllowOnce,
        })
        .expect("approval");
    runtime
        .resolve_egress(approval.into_token(), EgressResolution::Written)
        .expect("approval written");
    let written = runtime
        .ingest(&frame(fixture["clientWrite"].clone()), &mapper)
        .expect("client write");
    assert_eq!(
        egress_value(&written.egress()[0])["result"],
        serde_json::json!({})
    );
    assert!(!has_file_change(written.events()));
    assert_eq!(
        std::fs::read_to_string(&target).expect("file"),
        "RAW APPROVED PROBE"
    );
    let completed = runtime
        .ingest(&frame(fixture["toolComplete"].clone()), &mapper)
        .expect("tool completion");
    assert!(completed.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Item {
            payload: NormalizedPayload::FileChangeDiff {
                operation: sage_acp_protocol::intent::FileChangeOperation::Created,
                additions: Some(1),
                deletions: Some(0),
                ..
            },
            ..
        }
    )));
    assert!(completed.events().iter().any(|event| matches!(
        event,
        AdapterEvent::FilePair { old_text, new_text, .. }
            if old_text.is_empty() && new_text == "RAW APPROVED PROBE"
    )));
    let duplicate = runtime
        .ingest(&frame(fixture["toolComplete"].clone()), &mapper)
        .expect("duplicate completion");
    assert!(!has_file_change(duplicate.events()));
}

fn has_file_change(events: &[AdapterEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { .. },
                ..
            } | AdapterEvent::FilePair { .. }
        )
    })
}

#[test]
fn differing_or_refused_client_write_never_projects_the_proposed_created_diff() {
    for (label, differing) in [("different-write", true), ("refused-write", false)] {
        let root = TempRoot(temp_root().join(format!(
            "sage-opencode-created-negative-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let target = root.0.join("captured.txt");
        let mapper = CreatedFileMapper(target.clone());
        let path = target.to_str().expect("target path");
        let escaped = serde_json::to_string(path).expect("escaped path");
        let mut fixture: Value = serde_json::from_str(
            &include_str!("../../testdata/opencode_real_allow_write_20260929.json")
                .replace("__TARGET__", &escaped[1..escaped.len() - 1]),
        )
        .expect("captured fixture");
        let mut runtime = ready_runtime(&root, 62, label, OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "create captured file").expect("prompt"),
                client_message_id: RequiredText::new("client", label).expect("client"),
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime
            .ingest(&frame(fixture["toolStart"].clone()), &mapper)
            .expect("tool start");
        runtime
            .ingest(&frame(fixture["toolProgress"].clone()), &mapper)
            .expect("tool progress");
        runtime
            .ingest(&frame(fixture["permission"].clone()), &mapper)
            .expect("permission");
        let approval = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id(label, 62),
                decision: sage_acp_protocol::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approval");
        runtime
            .resolve_egress(approval.into_token(), EgressResolution::Written)
            .expect("approval written");
        if differing {
            fixture["clientWrite"]["params"]["content"] = serde_json::json!("DIFFERENT");
        } else {
            fixture["clientWrite"]["params"]["path"] =
                serde_json::json!(root.0.join("wrong.txt").to_str().expect("wrong path"));
        }
        let write = runtime
            .ingest(&frame(fixture["clientWrite"].clone()), &mapper)
            .expect("client write response");
        if differing {
            assert_eq!(write.egress().len(), 1);
            assert_eq!(std::fs::read_to_string(&target).expect("file"), "DIFFERENT");
        } else {
            assert!(egress_value(&write.egress()[0]).get("error").is_some());
            assert!(!target.exists());
        }
        let completed = runtime
            .ingest(&frame(fixture["toolComplete"].clone()), &mapper)
            .expect("tool completion");
        assert!(!has_file_change(completed.events()), "{label}");
    }
}
