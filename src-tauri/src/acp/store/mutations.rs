use crate::acp::{
    contract::ProjectionRevision,
    lifecycle::{DeliveryState, TurnState},
    recovery::RecoveryReport,
    runtime::permissions::PermissionIntent,
};

use super::{
    AbortActivationCommand, ActivateSessionCommand, ActivityIngressCommand,
    ActivityIngressMutation, AdapterBatchMutation, AdapterInboundBatchCommand, BeginCancelCommand,
    CompleteActivationCommand, EntityMutation, FailActivationCommand, FinalizeCloseCommand,
    ItemIngressCommand, ItemIngressMutation, ParkSessionCommand, PrepareTurnCommand, PreparedTurn,
    RecoverRuntimeCommand, RequestStateCommand, SendObservation, SessionMutation,
    StartSessionCommand, StoreError, TurnMutation, TurnMutationCommand,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreMutationResult {
    Lookup(Option<PreparedTurn>),
    ActiveTurnLookup(Option<TurnMutation>),
    Prepared(PreparedTurn),
    Turn(TurnMutation),
    Session(SessionMutation),
    Entity(EntityMutation),
    Item(ItemIngressMutation),
    Activity(ActivityIngressMutation),
    AdapterBatch(AdapterBatchMutation),
    Recovery(RecoveryReport),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreMutation {
    LookupTurn(PrepareTurnCommand),
    LookupActiveTurn(TurnMutationCommand),
    PrepareTurn(PrepareTurnCommand),
    BeginSend(TurnMutationCommand),
    BeginCancel(BeginCancelCommand),
    ObserveSend {
        command: TurnMutationCommand,
        observation: SendObservation,
    },
    StartSession(StartSessionCommand),
    ActivateSession(ActivateSessionCommand),
    AbortActivation(AbortActivationCommand),
    CompleteActivation(CompleteActivationCommand),
    FailActivation(FailActivationCommand),
    TurnTerminal {
        command: TurnMutationCommand,
        terminal: TurnState,
        delivery: DeliveryState,
    },
    CloseSession(super::CloseSessionCommand),
    FinalizeClose(FinalizeCloseCommand),
    ParkSession(ParkSessionCommand),
    ReceivePermission {
        intent: PermissionIntent,
        now_millis: i64,
    },
    RequestState(RequestStateCommand),
    ItemIngress(ItemIngressCommand),
    ActivityIngress(ActivityIngressCommand),
    AdapterInboundBatch(AdapterInboundBatchCommand),
    RecoverAfterRestart {
        now_millis: i64,
    },
    RecoverRuntimeAfterZero(RecoverRuntimeCommand),
}

impl StoreMutation {
    pub(super) fn is_standalone(&self) -> bool {
        !matches!(
            self,
            Self::LookupTurn(_)
                | Self::LookupActiveTurn(_)
                | Self::PrepareTurn(_)
                | Self::BeginSend(_)
                | Self::ObserveSend { .. }
        )
    }
}

pub(super) fn standalone_commit(
    connection: &mut rusqlite::Connection,
    mutation: &StoreMutation,
) -> Result<super::StoreCommit, StoreError> {
    let result = match mutation {
        StoreMutation::BeginCancel(command) => {
            StoreMutationResult::Turn(super::begin_cancel(connection, command.clone())?)
        }
        StoreMutation::StartSession(command) => {
            StoreMutationResult::Session(super::start_session(connection, command.clone())?)
        }
        StoreMutation::ActivateSession(command) => {
            StoreMutationResult::Session(super::activate_session(connection, command.clone())?)
        }
        StoreMutation::AbortActivation(command) => {
            StoreMutationResult::Session(super::abort_activation(connection, command.clone())?)
        }
        StoreMutation::CompleteActivation(command) => {
            StoreMutationResult::Session(super::complete_activation(connection, command.clone())?)
        }
        StoreMutation::FailActivation(command) => {
            StoreMutationResult::Session(super::fail_activation(connection, command.clone())?)
        }
        StoreMutation::TurnTerminal {
            command,
            terminal,
            delivery,
        } => StoreMutationResult::Entity(super::apply_turn_terminal(
            connection,
            command.clone(),
            *terminal,
            *delivery,
        )?),
        StoreMutation::CloseSession(command) => {
            StoreMutationResult::Session(super::close_session(connection, command.clone())?)
        }
        StoreMutation::FinalizeClose(command) => StoreMutationResult::Session(
            super::finalize_close_after_zero_process(connection, command.clone())?,
        ),
        StoreMutation::ParkSession(command) => StoreMutationResult::Session(
            super::park_session_after_zero_process(connection, command.clone())?,
        ),
        StoreMutation::ReceivePermission { intent, now_millis } => StoreMutationResult::Entity(
            super::receive_permission_request(connection, intent, *now_millis)?,
        ),
        StoreMutation::RequestState(command) => {
            StoreMutationResult::Entity(super::apply_request_state(connection, command.clone())?)
        }
        StoreMutation::ItemIngress(command) => {
            StoreMutationResult::Item(super::apply_item_ingress(connection, command.clone())?)
        }
        StoreMutation::ActivityIngress(command) => StoreMutationResult::Activity(
            super::apply_activity_ingress(connection, command.clone())?,
        ),
        StoreMutation::AdapterInboundBatch(command) => StoreMutationResult::AdapterBatch(
            super::adapter_batch::apply_adapter_inbound_batch_ref(connection, command)?,
        ),
        StoreMutation::RecoverAfterRestart { now_millis } => StoreMutationResult::Recovery(
            crate::acp::recovery::recover_after_restart(connection, *now_millis)?,
        ),
        StoreMutation::RecoverRuntimeAfterZero(command) => StoreMutationResult::Recovery(
            crate::acp::recovery::recover_runtime_after_zero_process(connection, command.clone())?,
        ),
        StoreMutation::LookupTurn(_)
        | StoreMutation::LookupActiveTurn(_)
        | StoreMutation::PrepareTurn(_)
        | StoreMutation::BeginSend(_)
        | StoreMutation::ObserveSend { .. } => return Err(StoreError::BatchShape),
    };
    Ok(super::StoreCommit {
        projection_revision: result_projection(&result),
        results: vec![result],
    })
}

fn result_projection(result: &StoreMutationResult) -> ProjectionRevision {
    match result {
        StoreMutationResult::Lookup(value) => value
            .as_ref()
            .map_or(ProjectionRevision::ZERO, |value| value.projection_revision),
        StoreMutationResult::ActiveTurnLookup(value) => value
            .as_ref()
            .map_or(ProjectionRevision::ZERO, |value| value.projection_revision),
        StoreMutationResult::Prepared(value) => value.projection_revision,
        StoreMutationResult::Turn(value) => value.projection_revision,
        StoreMutationResult::Session(value) => value.projection_revision,
        StoreMutationResult::Entity(value) => value.projection_revision,
        StoreMutationResult::Item(value) => value.entity.projection_revision,
        StoreMutationResult::Activity(value) => value.entity.projection_revision,
        StoreMutationResult::AdapterBatch(value) => value.projection_revision,
        StoreMutationResult::Recovery(value) => value
            .projection_revision
            .unwrap_or(ProjectionRevision::ZERO),
    }
}
