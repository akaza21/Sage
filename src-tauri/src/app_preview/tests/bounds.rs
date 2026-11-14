use super::super::{
    discovery::{PreviewDiscoveryRequest, validate_discovery_request},
    lifecycle,
    projection::{PreviewPhase, PreviewProjection, PreviewProjectionOwner},
    url::parse_allowed_preview_url,
};
use super::discovery_request;
use sage_acp_protocol::contract::{
    EntityRevision, ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
};
use sage_core::limits::PREVIEW_LIMITS;

#[test]
fn every_preview_phase_edge_is_explicitly_admitted_or_rejected() {
    use PreviewPhase::{
        Active, Discovering, Exited, HealthChecking, HealthFailed, Idle, Starting, Stopped,
        Stopping, WaitingForOrigin,
    };
    let phases = [
        Idle,
        Discovering,
        Starting,
        WaitingForOrigin,
        HealthChecking,
        Active,
        HealthFailed,
        Exited,
        Stopping,
        Stopped,
    ];
    let admitted = [
        (Idle, Discovering),
        (Idle, Starting),
        (Discovering, Idle),
        (Starting, WaitingForOrigin),
        (Starting, HealthFailed),
        (Starting, Exited),
        (Starting, Stopping),
        (WaitingForOrigin, HealthChecking),
        (WaitingForOrigin, HealthFailed),
        (WaitingForOrigin, Exited),
        (WaitingForOrigin, Stopping),
        (HealthChecking, Active),
        (HealthChecking, HealthFailed),
        (HealthChecking, Exited),
        (HealthChecking, Stopping),
        (Active, HealthChecking),
        (Active, Stopping),
        (Active, Exited),
        (HealthFailed, Discovering),
        (HealthFailed, Starting),
        (Exited, Discovering),
        (Exited, Starting),
        (Stopping, Stopped),
        (Stopped, Discovering),
        (Stopped, Starting),
    ];
    for from in phases {
        for to in phases {
            assert_eq!(
                from.allows(to),
                admitted.contains(&(from, to)),
                "unexpected transition verdict for {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn preview_projection_is_discriminated_and_revision_is_lossless() {
    let projection = PreviewProjection::initial(preview_owner()).unwrap();
    let value = serde_json::to_value(projection.event()).unwrap();
    assert_eq!(value["kind"], "preview");
    assert_eq!(value["preview"]["previewRevision"], "1");
    assert!(value.get("projectionRevision").is_none());
    assert!(serde_json::from_value::<crate::acp::ipc::ProjectionChangedV1>(value).is_err());
}

#[test]
fn preview_revision_accepts_exact_maximum_and_rejects_exhaustion() {
    let projection = PreviewProjection::at_revision(
        preview_owner(),
        EntityRevision::new(PREVIEW_LIMITS.revision_max).unwrap(),
    )
    .unwrap();
    assert!(
        projection
            .advance(PreviewPhase::Discovering, None, None)
            .is_err()
    );
}

#[test]
fn preview_url_accepts_exact_byte_limit_and_rejects_plus_one() {
    let prefix = "http://127.0.0.1:5173/";
    let exact = format!(
        "{prefix}{}",
        "a".repeat(PREVIEW_LIMITS.url_bytes as usize - prefix.len())
    );
    assert!(parse_allowed_preview_url(&exact).is_ok());
    assert!(parse_allowed_preview_url(&format!("{exact}a")).is_err());
}

#[test]
fn discovery_identity_accepts_exact_transition_bound_and_rejects_mismatch_or_plus_one() {
    let exact = discovery_request(
        "project-preview",
        PREVIEW_LIMITS.transition_epoch_max,
        PREVIEW_LIMITS.transition_epoch_max,
    );
    assert!(validate_discovery_request(&exact).is_ok());
    assert!(
        validate_discovery_request(&PreviewDiscoveryRequest {
            expected_transition_epoch: PREVIEW_LIMITS.transition_epoch_max - 1,
            ..exact.clone()
        })
        .is_err()
    );
    assert!(
        validate_discovery_request(&PreviewDiscoveryRequest {
            expected_transition_epoch: PREVIEW_LIMITS.transition_epoch_max + 1,
            ..exact
        })
        .is_err()
    );
}

#[test]
fn every_preview_cap_has_an_exact_and_plus_one_boundary() {
    for limit in [
        PREVIEW_LIMITS.identifier_bytes,
        PREVIEW_LIMITS.error_bytes,
        PREVIEW_LIMITS.transition_epoch_max,
        PREVIEW_LIMITS.url_candidates,
        PREVIEW_LIMITS.browse_sessions_per_chat,
        PREVIEW_LIMITS.browse_chats_per_project,
        PREVIEW_LIMITS.browse_sessions_global,
        PREVIEW_LIMITS.browse_history,
        PREVIEW_LIMITS.browse_listeners,
        PREVIEW_LIMITS.frontend_listeners,
        PREVIEW_LIMITS.startup_deadline_ms,
        PREVIEW_LIMITS.startup_idle_deadline_ms,
        PREVIEW_LIMITS.health_deadline_ms,
        PREVIEW_LIMITS.health_probe_timeout_ms,
        PREVIEW_LIMITS.health_retry_ms,
        PREVIEW_LIMITS.process_termination_deadline_ms,
    ] {
        assert!(lifecycle::test_deadline_millis_admitted(limit, limit));
        assert!(!lifecycle::test_deadline_millis_admitted(limit + 1, limit));
    }
    boundary(
        super::super::discovery::test_depth_admitted,
        PREVIEW_LIMITS.discovery_depth,
    );
    boundary(
        super::super::discovery::test_package_count_admitted,
        PREVIEW_LIMITS.packages_per_root,
    );
    boundary(
        super::super::discovery::test_directory_count_admitted,
        PREVIEW_LIMITS.directories_per_root,
    );
    boundary(
        super::super::discovery::test_manifest_bytes_admitted,
        PREVIEW_LIMITS.manifest_bytes,
    );
    boundary(
        super::super::discovery::test_target_count_admitted,
        PREVIEW_LIMITS.targets_per_project,
    );
    boundary(
        super::super::discovery::test_directory_entries_admitted,
        PREVIEW_LIMITS.directory_entries,
    );
    boundary(
        super::super::discovery::test_script_bytes_admitted,
        PREVIEW_LIMITS.script_bytes,
    );
    boundary(
        lifecycle::test_startup_output_bytes_admitted,
        PREVIEW_LIMITS.startup_output_bytes,
    );
    boundary(
        lifecycle::test_channel_slots_admitted,
        PREVIEW_LIMITS.startup_output_channel,
    );
    boundary(
        lifecycle::test_drainer_count_admitted,
        PREVIEW_LIMITS.output_drainers_per_child,
    );
}

fn boundary(admitted: fn(u64) -> bool, limit: u64) {
    assert!(admitted(limit));
    assert!(!admitted(limit + 1));
}

fn preview_owner() -> PreviewProjectionOwner {
    PreviewProjectionOwner {
        project_id: SageProjectId::parse("project-preview").unwrap(),
        project_generation: ProjectGeneration::new(7).unwrap(),
        transition_epoch: 7,
        chat_id: SageChatId::parse("chat-preview").unwrap(),
        session_id: SageAgentSessionId::parse("session-preview").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
    }
}
