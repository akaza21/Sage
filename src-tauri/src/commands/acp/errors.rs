use crate::acp::{
    bindings::codex::CodexDiscoveryRunError,
    contract::SafeText,
    intent::{
        AcpCapacityResource, AcpConflictResource, AcpIpcError, AcpUnavailableReason, IntentError,
    },
    page_admission::PageAdmissionError,
    store::StoreError,
};

pub(super) fn dispatch_error(error: crate::acp::service::SessionDispatchError) -> AcpIpcError {
    match error {
        crate::acp::service::SessionDispatchError::Binding => binding_unavailable(),
        crate::acp::service::SessionDispatchError::Operation(error) => session_error(error),
    }
}

pub(super) fn session_error(error: crate::acp::service::CodexSessionError) -> AcpIpcError {
    tracing::warn!(
        acp_failure = ?error,
        "ACP session request failed"
    );
    match error {
        crate::acp::service::CodexSessionError::Binding(_) => binding_unavailable(),
        crate::acp::service::CodexSessionError::RewindUnavailable => {
            AcpIpcError::RecoveryRequired {
                message: safe_message(
                    "The pending undo could not be applied. The session was not resumed; the rewind remains pending.",
                ),
            }
        }
        crate::acp::service::CodexSessionError::Intent => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The native session identity is stale or foreign."),
        },
        crate::acp::service::CodexSessionError::SteerOutcomeUnknown => AcpIpcError::Conflict {
            resource: AcpConflictResource::SteerOutcomeUnknown,
            message: safe_message(
                "Steer acknowledgement timed out. The message may have been delivered; the session is still live. Retry the queued message explicitly.",
            ),
        },
        crate::acp::service::CodexSessionError::Capacity => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: sage_core::limits::LIMITS.process.operational_live_trees,
            message: safe_message("The native live runtime capacity is occupied."),
        },
        crate::acp::service::CodexSessionError::CapacityAnswering => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: sage_core::limits::LIMITS.process.operational_live_trees,
            message: safe_message(
                "Every live agent is busy, and an idle one is still answering a request; try again in a moment.",
            ),
        },
        crate::acp::service::CodexSessionError::SignInRequired => AcpIpcError::SignInRequired {
            message: safe_message("The harness requires sign-in before it can continue."),
        },
        crate::acp::service::CodexSessionError::Recovery => AcpIpcError::RecoveryRequired {
            message: safe_message("The native runtime requires authoritative recovery."),
        },
        crate::acp::service::CodexSessionError::LaunchFailure(error) => launch_failed(error.step()),
        crate::acp::service::CodexSessionError::LaunchSetup(error) => launch_failed(error.step()),
        crate::acp::service::CodexSessionError::Session(
            crate::acp::runtime::SessionCoordinatorError::BootstrapRequired,
        ) => AcpIpcError::RecoveryRequired {
            message: safe_message("Native activation requires authoritative projection recovery."),
        },
        crate::acp::service::CodexSessionError::Session(
            crate::acp::runtime::SessionCoordinatorError::Journal(
                crate::acp::runtime::JournalError::Store(StoreError::Conflict {
                    resource: "chatSessionHead",
                }),
            ),
        ) => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message(
                "This Chat already belongs to another harness session; start a new Chat.",
            ),
        },
        crate::acp::service::CodexSessionError::Session(_) => AcpIpcError::RecoveryRequired {
            message: safe_message(
                "Native activation coordination failed safely; reload authoritative history.",
            ),
        },
    }
}

pub(super) fn codex_discovery_error(error: CodexDiscoveryRunError) -> AcpIpcError {
    match error {
        CodexDiscoveryRunError::Unavailable => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::PlatformUnsupported,
            message: safe_message("Codex discovery is unsupported on this platform."),
        },
        CodexDiscoveryRunError::BindingUnavailable => binding_unavailable(),
        CodexDiscoveryRunError::CapabilityProjectionUnavailable => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message(
                "Codex capabilities could not be safely projected; retry discovery.",
            ),
        },
        CodexDiscoveryRunError::InvalidScope => AcpIpcError::InvalidRequest {
            message: safe_message("The Codex discovery scope is invalid."),
        },
        CodexDiscoveryRunError::Busy => AcpIpcError::Capacity {
            resource: AcpCapacityResource::Queue,
            limit: 1,
            message: safe_message("Codex discovery is already running; retry it."),
        },
        CodexDiscoveryRunError::Drift => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex or the selected Project root changed; retry discovery."),
        },
        CodexDiscoveryRunError::Cleanup => AcpIpcError::RecoveryRequired {
            message: safe_message("Codex discovery could not prove process cleanup; restart Sage."),
        },
        CodexDiscoveryRunError::Protocol => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex rejected the discovery protocol; retry it."),
        },
        CodexDiscoveryRunError::Timeout => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery timed out; retry it."),
        },
        CodexDiscoveryRunError::Eof => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex ended discovery transport; retry it."),
        },
        CodexDiscoveryRunError::Transport => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery transport failed; retry it."),
        },
        CodexDiscoveryRunError::Framing => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery transport framing failed; retry it."),
        },
        CodexDiscoveryRunError::Overflow => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery transport overflowed; retry it."),
        },
        CodexDiscoveryRunError::ReaderDisconnected => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery transport reader disconnected; retry it."),
        },
        CodexDiscoveryRunError::Write => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery transport write failed; retry it."),
        },
        CodexDiscoveryRunError::Launch => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery process launch failed; retry it."),
        },
        CodexDiscoveryRunError::LaunchIdentity => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery launch identity check failed; retry it."),
        },
    }
}

pub(super) fn binding_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::BindingUnavailable,
        message: safe_message("No curated ACP harness binding is admitted in this build."),
    }
}

pub(super) fn page_admission_error(error: PageAdmissionError) -> AcpIpcError {
    let limit = match error {
        PageAdmissionError::Owner => {
            sage_core::limits::LIMITS
                .durable
                .outstanding_page_reads_per_session
        }
        PageAdmissionError::Global => {
            sage_core::limits::LIMITS
                .durable
                .outstanding_page_reads_global
        }
    };
    AcpIpcError::Capacity {
        resource: AcpCapacityResource::PageRead,
        limit,
        message: safe_message("Too many ACP history pages are already being read."),
    }
}

/// A launch that failed before the session was live; it names the step.
fn launch_failed(step: &'static str) -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::CapabilityUnavailable,
        message: SafeText::new(
            "message",
            format!("Native launch failed safely while {step}; retry the session."),
        )
        .expect("launch step message is bounded"),
    }
}

pub(super) fn safe_message(message: &'static str) -> SafeText<8192> {
    SafeText::new("message", message).expect("static ACP message is bounded")
}

/// An archived Chat runs no agent: no prompt, session start or activation.
pub(super) fn archived_chat() -> AcpIpcError {
    AcpIpcError::InvalidRequest {
        message: safe_message("This Chat is archived. Restore it to continue."),
    }
}

pub(super) fn internal_error() -> AcpIpcError {
    AcpIpcError::Internal {
        message: safe_message("The ACP request failed safely."),
    }
}

pub(super) fn store_error(error: StoreError) -> AcpIpcError {
    match error {
        StoreError::Capacity { limit, .. } => AcpIpcError::Capacity {
            resource: AcpCapacityResource::History,
            limit,
            message: safe_message("Durable ACP history capacity has been reached."),
        },
        StoreError::Conflict { .. } => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The requested ACP projection is stale or foreign."),
        },
        StoreError::CommitUnknown => AcpIpcError::RecoveryRequired {
            message: safe_message("The durable outcome is unknown; reload authoritative history."),
        },
        StoreError::Corrupt(_) => AcpIpcError::RecoveryRequired {
            message: safe_message("Durable ACP history failed bounded validation."),
        },
        StoreError::PayloadTooLarge | StoreError::BatchShape => AcpIpcError::InvalidRequest {
            message: safe_message("The ACP request violates its bounded contract."),
        },
        StoreError::Database(_)
        | StoreError::InvalidTransition { .. }
        | StoreError::TerminalConflict { .. }
        | StoreError::CounterExhausted => internal_error(),
    }
}

pub(super) fn intent_error(error: IntentError) -> AcpIpcError {
    tracing::info!(
        intent_error = ?error,
        "ACP intent rejected by bounded guard"
    );
    let resource = match error {
        IntentError::ForeignProject | IntentError::StaleProjectGeneration => {
            AcpConflictResource::ProjectGeneration
        }
        IntentError::ForeignChat | IntentError::StaleProjectionRevision => {
            AcpConflictResource::ProjectionRevision
        }
        IntentError::ForeignSession => AcpConflictResource::Session,
        IntentError::StaleRuntimeEpoch => AcpConflictResource::RuntimeEpoch,
        IntentError::ForeignTurn | IntentError::UnexpectedTurnState => {
            AcpConflictResource::TurnState
        }
        IntentError::StaleTurnRevision => AcpConflictResource::TurnRevision,
        IntentError::InvalidProjectionRevision => AcpConflictResource::ProjectionRevision,
    };
    AcpIpcError::Conflict {
        resource,
        message: safe_message("The ACP intent is stale or belongs to another resource."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            serde_json::to_value(error).expect("serializes"),
            serde_json::json!({
                "kind": "signInRequired",
                "message": "The harness requires sign-in before it can continue."
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
            crate::acp::process::LaunchError::IdentityMismatch,
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
                crate::acp::service::CodexSessionError::LaunchSetup(
                    crate::acp::service::CodexLaunchSetupError::RunnerStartSessionResume,
                ),
                "Native launch failed safely while resuming the harness session; retry the session.",
            ),
            (
                crate::acp::service::CodexSessionError::LaunchFailure(
                    crate::acp::process::LaunchError::ContainmentFailure,
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
}
