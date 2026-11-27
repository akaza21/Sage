use crate::acp::{
    bindings::codex::CodexDiscoveryRunError,
    ipc::{
        AcpCapacityResource, AcpConflictResource, AcpIpcError, AcpUnavailableReason, IntentError,
    },
    store::StoreError,
};
use sage_acp_protocol::{contract::SafeText, page_admission::PageAdmissionError};
use sage_process::error::LaunchError;

pub(super) fn dispatch_error(error: crate::acp::service::SessionDispatchError) -> AcpIpcError {
    match error {
        crate::acp::service::SessionDispatchError::Binding => binding_unavailable(),
        crate::acp::service::SessionDispatchError::Operation(error) => session_error(error),
    }
}

/// Only persisted native terminal times may enter the shared sign-in cache.
pub(super) fn persisted_sign_in_error(
    state: &crate::state::AppState,
    command: crate::acp::store::commands::sign_in_evidence::RefusalCommand<'_>,
    mut failure: AcpIpcError,
) -> AcpIpcError {
    if let AcpIpcError::SignInRequired { terminal_at, .. } = &mut failure {
        let refused_at = sage_core::time::now_millis();
        let mut connection = match state.database.write_connection() {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!(?error, "sign-in refusal evidence was not admitted");
                return failure;
            }
        };
        match crate::acp::store::commands::sign_in_evidence::command_terminal_at(
            &mut connection,
            command,
            refused_at,
        ) {
            Ok(stored) => *terminal_at = Some(stored),
            Err(error) => tracing::warn!(?error, "sign-in refusal evidence was not stored"),
        }
    }
    failure
}

/// The launch step that failed, as the chat names it.
fn launch_step(error: LaunchError) -> &'static str {
    match error {
        LaunchError::ArgumentLimit | LaunchError::InvalidArgument => {
            "preparing the harness arguments"
        }
        LaunchError::CapacityUnavailable => "claiming a process slot",
        LaunchError::ContainmentFailure => "containing the harness process",
        LaunchError::Deadline => "waiting for the harness process",
        LaunchError::EnvironmentLimit | LaunchError::InvalidEnvironment => {
            "preparing the harness environment"
        }
        LaunchError::IdentityMismatch => "checking the harness identity",
        LaunchError::InvalidAbsolutePath => "resolving the harness path",
        LaunchError::Io => "setting up the harness transport",
        LaunchError::PlatformUnavailable => "using this platform's process support",
        LaunchError::StdioLimit => "opening the harness pipes",
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
        crate::acp::service::CodexSessionError::RepositoryBusy(pause) => AcpIpcError::Rejected {
            message: safe_message(pause.message()),
        },
        crate::acp::service::CodexSessionError::PromptRefused(reason) => AcpIpcError::Rejected {
            message: safe_message(reason),
        },
        crate::acp::service::CodexSessionError::CapacityAnswering => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: sage_core::limits::LIMITS.process.operational_live_trees,
            message: safe_message(
                "Every live agent is busy, and an idle one is still answering a request; try again in a moment.",
            ),
        },
        crate::acp::service::CodexSessionError::SignInRequired => AcpIpcError::SignInRequired {
            terminal_at: None,
            message: safe_message("The harness requires sign-in before it can continue."),
        },
        crate::acp::service::CodexSessionError::Recovery => AcpIpcError::RecoveryRequired {
            message: safe_message("The native runtime requires authoritative recovery."),
        },
        crate::acp::service::CodexSessionError::LaunchFailure(error) => {
            launch_failed(launch_step(error))
        }
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
        StoreError::RewindUnavailable { reason } => AcpIpcError::Rejected {
            message: safe_message(reason),
        },
        StoreError::RepositoryBusy(pause) => AcpIpcError::Rejected {
            message: safe_message(pause.message()),
        },
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
        #[cfg(test)]
        IntentError::InvalidProjectionRevision => AcpConflictResource::ProjectionRevision,
    };
    AcpIpcError::Conflict {
        resource,
        message: safe_message("The ACP intent is stale or belongs to another resource."),
    }
}

#[cfg(test)]
#[path = "errors_storage_tests.rs"]
mod storage_tests;

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
