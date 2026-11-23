use super::*;

#[test]
fn late_repository_busy_refusals_keep_the_operation_and_restart_advice() {
    use crate::repository_mutation_guard::RepositoryPause;
    for (pause, message) in [
        (
            RepositoryPause::ChangingFiles,
            "Can't start a turn while Sage is changing files in this repository.",
        ),
        (
            RepositoryPause::UntilRestart,
            "Can't start a turn while Sage is changing files in this repository. Agent turns in this repository stay paused until Sage restarts.",
        ),
    ] {
        let error = session_error(crate::acp::service::CodexSessionError::RepositoryBusy(
            pause,
        ));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "rejected", "message": message
            })
        );
    }
}

#[test]
fn discovery_transport_failures_preserve_the_safe_native_classification() {
    let error = codex_discovery_error(CodexDiscoveryRunError::Eof);

    assert!(matches!(
        error,
        AcpIpcError::Unavailable { message, .. }
            if message.as_str() == "Codex ended discovery transport; retry it."
    ));
}

#[test]
fn discovery_transport_subtypes_are_visible_without_native_diagnostics() {
    let error = codex_discovery_error(CodexDiscoveryRunError::LaunchIdentity);

    assert!(matches!(
        error,
        AcpIpcError::Unavailable { message, .. }
            if message.as_str() == "Codex discovery launch identity check failed; retry it."
    ));
}

#[test]
fn activation_bootstrap_failure_is_a_safe_recovery_subtype() {
    let error = session_error(crate::acp::service::CodexSessionError::Session(
        crate::acp::runtime::SessionCoordinatorError::BootstrapRequired,
    ));

    assert!(matches!(
        error,
        AcpIpcError::RecoveryRequired { message }
            if message.as_str()
                == "Native activation requires authoritative projection recovery."
    ));
}

#[test]
fn sign_in_refusal_has_a_stable_distinct_ipc_kind() {
    let error = session_error(crate::acp::service::CodexSessionError::SignInRequired);
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::json!({
            "kind": "signInRequired", "message": "The harness requires sign-in before it can continue."
        })
    );
}

#[test]
fn occupied_chat_session_head_is_a_safe_session_conflict_subtype() {
    let error = session_error(crate::acp::service::CodexSessionError::Session(
        crate::acp::runtime::SessionCoordinatorError::Journal(
            crate::acp::runtime::JournalError::Store(StoreError::Conflict {
                resource: "chatSessionHead",
            }),
        ),
    ));

    assert!(matches!(
        error,
        AcpIpcError::Conflict { resource: AcpConflictResource::Session, message }
            if message.as_str()
                == "This Chat already belongs to another harness session; start a new Chat."
    ));
}

#[test]
fn launch_identity_mismatch_is_a_safe_launch_subtype() {
    let error = session_error(crate::acp::service::CodexSessionError::LaunchFailure(
        sage_process::error::LaunchError::IdentityMismatch,
    ));

    assert!(matches!(
        error,
        AcpIpcError::Unavailable { message, .. }
            if message.as_str()
                == "Native launch failed safely while checking the harness identity; retry the session."
    ));
}

#[test]
fn a_failed_launch_names_the_step_that_failed() {
    for (error, expected) in [
        (
            // Regression: a harness that could not be prepared read as a
            // stale or foreign session identity.
            crate::acp::service::CodexSessionError::from_launch_preparation(
                crate::acp::bindings::adapter::AdapterRuntimeError::LaunchUnavailable,
            ),
            "Native launch failed safely while preparing the harness (its installed files or the project folder are missing or changed); retry the session.",
        ),
        (
            crate::acp::service::CodexSessionError::LaunchSetup(
                crate::acp::service::CodexLaunchSetupError::RunnerStartApprovalBridge,
            ),
            "Native launch failed safely while initializing the approval bridge (missing or mismatched handshake; no prompt sent); retry the session.",
        ),
        (
            crate::acp::service::CodexSessionError::LaunchSetup(
                crate::acp::service::CodexLaunchSetupError::RunnerStartSessionResume,
            ),
            "Native launch failed safely while resuming the harness session; retry the session.",
        ),
        (
            crate::acp::service::CodexSessionError::LaunchFailure(
                sage_process::error::LaunchError::ContainmentFailure,
            ),
            "Native launch failed safely while containing the harness process; retry the session.",
        ),
    ] {
        assert!(matches!(
            session_error(error),
            AcpIpcError::Unavailable { message, .. } if message.as_str() == expected
        ));
    }
}

#[test]
fn active_operation_setup_failure_is_a_safe_launch_subtype() {
    let error = session_error(crate::acp::service::CodexSessionError::LaunchSetup(
        crate::acp::service::CodexLaunchSetupError::ActiveOperation,
    ));

    assert!(matches!(
        error,
        AcpIpcError::Unavailable { message, .. }
            if message.as_str()
                == "Native launch failed safely while claiming the launch; retry the session."
    ));
}
