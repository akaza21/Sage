use crate::acp::{
    bindings::codex::CodexDiscoveryRunError,
    contract::SafeText,
    intent::{
        AcpCapacityResource, AcpConflictResource, AcpIpcError, AcpUnavailableReason, IntentError,
    },
    page_admission::PageAdmissionError,
    store::StoreError,
};

pub(super) fn codex_session_error(error: crate::acp::service::CodexSessionError) -> AcpIpcError {
    match error {
        crate::acp::service::CodexSessionError::Binding(_) => binding_unavailable(),
        crate::acp::service::CodexSessionError::Intent => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The Codex session identity is stale or foreign."),
        },
        crate::acp::service::CodexSessionError::Capacity => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: crate::acp::limits::LIMITS.process.operational_live_trees,
            message: safe_message("The Codex live runtime capacity is occupied."),
        },
        crate::acp::service::CodexSessionError::Recovery => AcpIpcError::RecoveryRequired {
            message: safe_message("The Codex runtime requires authoritative recovery."),
        },
        crate::acp::service::CodexSessionError::Launch => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex launch failed safely; retry the session."),
        },
        crate::acp::service::CodexSessionError::Session(_) => internal_error(),
    }
}

pub(super) fn codex_discovery_error(error: CodexDiscoveryRunError) -> AcpIpcError {
    match error {
        CodexDiscoveryRunError::Unavailable => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::PlatformUnsupported,
            message: safe_message("Codex discovery is unsupported on this platform."),
        },
        CodexDiscoveryRunError::BindingUnavailable => binding_unavailable(),
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
        CodexDiscoveryRunError::Protocol
        | CodexDiscoveryRunError::Timeout
        | CodexDiscoveryRunError::Eof
        | CodexDiscoveryRunError::Transport => AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex discovery failed safely; retry it."),
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
