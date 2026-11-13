use super::{DurableAdapterEvent, NormalizedFact, NormalizedPayload};

pub(in crate::acp::runtime::adapter_pump) fn control_critical(fact: &NormalizedFact) -> bool {
    match &fact.event {
        DurableAdapterEvent::PermissionRequest { .. } => true,
        DurableAdapterEvent::PermissionDeny { .. } => true,
        DurableAdapterEvent::PermissionResolved { .. } => true,
        DurableAdapterEvent::Turn { state, .. } => state.is_terminal(),
        DurableAdapterEvent::RuntimePayload { payload, .. } => matches!(
            payload,
            NormalizedPayload::Error { .. } | NormalizedPayload::Unsupported { .. }
        ),
        DurableAdapterEvent::UsageSnapshot { .. } => false,
        DurableAdapterEvent::TerminalReason { .. } => true,
        // A steered message is durable before its accept completes.
        DurableAdapterEvent::Item { payload, .. } => {
            matches!(payload, NormalizedPayload::UserPrompt { .. })
        }
        DurableAdapterEvent::ChildThreadItem { .. } => false,
        DurableAdapterEvent::ChildLiveItem { .. } => false,
        DurableAdapterEvent::ChildTurnStatus { .. } => false,
        DurableAdapterEvent::FilePair { .. } => false,
        DurableAdapterEvent::ActivityItem(_) => false,
        DurableAdapterEvent::Session { .. } => false,
    }
}
