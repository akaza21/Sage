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
        crate::acp::service::CodexSessionError::Intent => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The native session identity is stale or foreign."),
        },
        crate::acp::service::CodexSessionError::EarlierCodexVersion => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message(
                "This native thread started on an earlier Codex version. Its Chat history is still available here; start a new Codex session to continue.",
            ),
        },
        crate::acp::service::CodexSessionError::BindingIncompatible(refusal) => {
            session_incompatible(&refusal)
        }
        crate::acp::service::CodexSessionError::SteerOutcomeUnknown => AcpIpcError::Conflict {
            resource: AcpConflictResource::SteerOutcomeUnknown,
            message: safe_message(
                "Steer acknowledgement timed out. The message may have been delivered; the session is still live. Retry the queued message explicitly.",
            ),
        },
        crate::acp::service::CodexSessionError::Capacity => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: crate::acp::limits::LIMITS.process.operational_live_trees,
            message: safe_message("The native live runtime capacity is occupied."),
        },
        crate::acp::service::CodexSessionError::SignInRequired => AcpIpcError::SignInRequired {
            message: safe_message("The harness requires sign-in before it can continue."),
        },
        crate::acp::service::CodexSessionError::Recovery => AcpIpcError::RecoveryRequired {
            message: safe_message("The native runtime requires authoritative recovery."),
        },
        crate::acp::service::CodexSessionError::LaunchFailure(
            crate::acp::process::LaunchError::IdentityMismatch,
        ) => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Native launch identity check failed safely; retry the session."),
        },
        crate::acp::service::CodexSessionError::LaunchFailure(
            crate::acp::process::LaunchError::Io,
        ) => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message(
                "Native launch transport setup failed safely; retry the session.",
            ),
        },
        crate::acp::service::CodexSessionError::LaunchFailure(_) => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Native launch setup failed safely; retry the session."),
        },
        crate::acp::service::CodexSessionError::LaunchSetup(
            crate::acp::service::CodexLaunchSetupError::ActiveOperation,
        ) => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message(
                "Native launch operation setup failed safely; retry the session.",
            ),
        },
        crate::acp::service::CodexSessionError::LaunchSetup(_) => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Native launch setup failed safely; retry the session."),
        },
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

fn session_incompatible(refusal: &crate::acp::store::RefuseSessionBindingCommand) -> AcpIpcError {
    use crate::acp::{intent::AcpSessionIncompatibleReason as Reason, store::BindingRefusalReason};
    let installed = refusal.to_version.as_str();
    let started = refusal
        .from_version
        .as_ref()
        .map_or("an earlier build", |version| version.as_str());
    let (reason, cause) = match refusal.reason {
        BindingRefusalReason::UnknownBuild => (
            Reason::UnknownBuild,
            format!(
                "Sage hasn't verified the installed build ({installed}) for sessions started on {started}."
            ),
        ),
        BindingRefusalReason::OtherFamily => (
            Reason::OtherFamily,
            format!(
                "The installed build ({installed}) uses a different session protocol than {started}."
            ),
        ),
    };
    AcpIpcError::SessionIncompatible {
        reason,
        // Versions are bounded to 128 bytes each, so the message always fits.
        message: SafeText::new(
            "message",
            format!(
                "This session can't be resumed. {cause} Your chat history is kept; continue in a new session."
            ),
        )
        .unwrap_or_else(|_| {
            safe_message(
                "This session can't be resumed. Your chat history is kept; continue in a new session.",
            )
        }),
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
            crate::acp::limits::LIMITS
                .durable
                .outstanding_page_reads_per_session
        }
        PageAdmissionError::Global => {
            crate::acp::limits::LIMITS
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

pub(super) fn safe_message(message: &'static str) -> SafeText<8192> {
    SafeText::new("message", message).expect("static ACP message is bounded")
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
    fn earlier_codex_thread_has_a_readable_session_conflict() {
        let error = session_error(crate::acp::service::CodexSessionError::EarlierCodexVersion);
        assert!(matches!(
            error,
            AcpIpcError::Conflict { resource: AcpConflictResource::Session, message }
                if message.as_str().contains("started on an earlier Codex version")
                    && message.as_str().contains("Chat history is still available")
        ));
    }

    #[test]
    fn an_incompatible_build_is_a_typed_refusal_naming_both_builds() {
        let refusal = crate::acp::store::RefuseSessionBindingCommand {
            project_id: crate::acp::contract::SageProjectId::parse("project-a").unwrap(),
            chat_id: crate::acp::contract::SageChatId::parse("chat-a").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
            expected_projection_revision: crate::acp::contract::ProjectionRevision::ZERO,
            client_intent_id: crate::acp::contract::ClientIntentId::parse("intent-a").unwrap(),
            reason: crate::acp::store::BindingRefusalReason::UnknownBuild,
            from_version: Some(SafeText::new("v", "0.101.0").unwrap()),
            to_version: SafeText::new("v", "0.102.0").unwrap(),
            now_millis: 1,
        };
        let error = session_error(crate::acp::service::CodexSessionError::BindingIncompatible(
            Box::new(refusal),
        ));
        assert_eq!(
            serde_json::to_value(error).expect("serializes"),
            serde_json::json!({
                "kind": "sessionIncompatible",
                "reason": "unknownBuild",
                "message": "This session can't be resumed. Sage hasn't verified the installed build (0.102.0) for sessions started on 0.101.0. Your chat history is kept; continue in a new session."
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
                    == "Native launch identity check failed safely; retry the session."
        ));
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
                    == "Native launch operation setup failed safely; retry the session."
        ));
    }
}
