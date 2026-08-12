use std::mem::size_of;

use crate::acp::{limits::LIMITS, store::StoreMutation};

use super::journal::JournalLane;

pub(super) fn mutation_retained_bytes(mutation: &StoreMutation) -> usize {
    let heap_utf8 = match mutation {
        StoreMutation::LookupTurn(command) | StoreMutation::PrepareTurn(command) => {
            command.project_id.as_str().len()
                + command.chat_id.as_str().len()
                + command.session_id.as_str().len()
                + command.client_intent_id.as_str().len()
                + command.prompt.as_str().len()
        }
        StoreMutation::LookupActiveTurn(command) => {
            command.project_id.as_str().len()
                + command.chat_id.as_str().len()
                + command.session_id.as_str().len()
                + command.turn_id.as_str().len()
        }
        StoreMutation::BeginSend(command) | StoreMutation::ObserveSend { command, .. } => {
            command.project_id.as_str().len()
                + command.chat_id.as_str().len()
                + command.session_id.as_str().len()
                + command.turn_id.as_str().len()
        }
        StoreMutation::BeginCancel(command) => {
            let turn = &command.turn;
            turn.project_id.as_str().len()
                + turn.chat_id.as_str().len()
                + turn.session_id.as_str().len()
                + turn.turn_id.as_str().len()
                + command.client_intent_id.as_str().len()
        }
        StoreMutation::StartSession(command) => {
            command.project_id.as_str().len()
                + command.chat_id.as_str().len()
                + command.client_intent_id.as_str().len()
                + command.binding_id.as_str().len()
                + command.binding_digest.len()
                + command
                    .harness_version
                    .as_ref()
                    .map_or(0, |value| value.as_str().len())
                + command
                    .model_id
                    .as_ref()
                    .map_or(0, |value| value.as_str().len())
        }
        StoreMutation::ActivateSession(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.client_intent_id.as_str().len()
                + command.artifact_digest.len()
        }
        StoreMutation::CompleteActivation(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.binding_digest.len()
                + command.artifact_digest.len()
                + command.external_session_id.as_str().len()
                + command.capabilities_json.len()
        }
        StoreMutation::FailActivation(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.binding_digest.len()
                + command.artifact_digest.len()
                + command.exit_kind.as_str().len()
        }
        StoreMutation::AbortActivation(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.binding_digest.len()
                + command.artifact_digest.len()
                + command.category.as_str().len()
        }
        StoreMutation::TurnTerminal { command, .. } => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.turn_id.as_str().len()
        }
        StoreMutation::CloseSession(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.client_intent_id.as_str().len()
        }
        StoreMutation::FinalizeClose(command) => {
            let command = &command.request;
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.client_intent_id.as_str().len()
        }
        StoreMutation::ParkSession(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.exit_kind.as_str().len()
        }
        StoreMutation::ReceivePermission { intent, .. } => {
            session_command_bytes(
                intent.project_id.as_str(),
                intent.chat_id.as_str(),
                intent.session_id.as_str(),
            ) + intent.turn_id.as_str().len()
                + intent.request_id.as_str().len()
                + intent.binding_digest.len()
                + intent.external_request_id.as_str().len()
                + intent.kind.as_str().len()
                + intent.payload_digest.len()
                + intent.normalized_payload_json.len()
        }
        StoreMutation::RequestState(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.turn_id.as_str().len()
                + command.request_id.as_str().len()
        }
        StoreMutation::ItemIngress(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.turn_id.as_str().len()
                + command.external_item_id.as_str().len()
                + command
                    .activity_id
                    .as_ref()
                    .map_or(0, |value| value.as_str().len())
                + command.payload_digest.len()
                + LIMITS.wire.normalized_item_bytes as usize
        }
        StoreMutation::ActivityIngress(command) => {
            session_command_bytes(
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            ) + command.turn_id.as_str().len()
                + command.external_activity_id.as_str().len()
                + command.activity_kind.as_str().len()
                + command.parent_item_id.as_str().len()
                + command
                    .parent_activity_id
                    .as_ref()
                    .map_or(0, |value| value.as_str().len())
                + command.provenance_digest.len()
                + LIMITS.wire.metadata_bytes as usize
        }
        StoreMutation::AdapterInboundBatch(command) => command.retained_bytes(),
        StoreMutation::RecoverAfterRestart { .. } => 0,
        StoreMutation::RecoverRuntimeAfterZero(command) => session_command_bytes(
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
        ),
    };
    size_of::<StoreMutation>() + heap_utf8
}

fn session_command_bytes(project: &str, chat: &str, session: &str) -> usize {
    project.len() + chat.len() + session.len()
}

pub(super) fn lane_name(lane: JournalLane) -> &'static str {
    match lane {
        JournalLane::Data => "data",
        JournalLane::Control => "control",
    }
}

pub(super) fn data_count_limit() -> usize {
    (LIMITS.queues.journal_batches - LIMITS.queues.journal_control_batches_inside_total) as usize
}

pub(super) fn data_byte_limit() -> usize {
    (LIMITS.queues.journal_bytes - LIMITS.queues.journal_control_bytes_inside_total) as usize
}

pub(super) fn control_count_limit() -> usize {
    LIMITS.queues.journal_control_batches_inside_total as usize
}

pub(super) fn control_byte_limit() -> usize {
    LIMITS.queues.journal_control_bytes_inside_total as usize
}
