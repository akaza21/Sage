use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch,
            SageAgentSessionId, SageChatId, SageProjectId,
        },
        fixture::adapter::FixtureAdapter,
        intent::{ExternalId, Prompt},
        lifecycle::{DeliveryState, TurnState},
        process::{TreeCapacityPool, TreeExit},
        runtime::{
            ActivationFlush, ActivationIntent, ActivationMode, ActivationPump,
            CoordinatorActivationHandoff, CoordinatorActivationJournal, SessionCoordinator,
            SessionTreeLease, ZeroProcessProof,
        },
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, PrepareTurnCommand, SendObservation, TurnMutationCommand,
            begin_send, load_chat_page, prepare_turn, record_send_observation,
        },
    },
    persistence::Database,
};

use super::{
    super::{
        external_fixture_adapters::{Mapper, launch_spec},
        external_fixture_support::ProcessCleanup,
        test_support::{FixtureDeadline, launch_with_deadline},
    },
    support::CompleteTransport,
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct HistoricalTurn {
    id: String,
    state: String,
    delivery_state: String,
}

pub(super) struct NewActivation<'a> {
    pub(super) expected_projection_revision: ProjectionRevision,
    pub(super) project_root: &'a Path,
    pub(super) external_session: ExternalId,
    pub(super) artifact_digest: &'a str,
    pub(super) now_millis: i64,
}

pub(super) async fn activate_new(
    sessions: &SessionCoordinator,
    owner: &RuntimeOwner,
    activation: NewActivation<'_>,
    deadline: FixtureDeadline,
) -> SessionTreeLease {
    let NewActivation {
        expected_projection_revision,
        project_root,
        external_session,
        artifact_digest,
        now_millis,
    } = activation;
    let binding_digest = [7_u8; 32];
    let capabilities = FixtureAdapter::activation_capabilities();
    let lease = tokio::time::timeout(
        deadline.operation_before_cleanup(),
        sessions.activate_session(activation_command(
            owner,
            expected_projection_revision,
            "resume-fixture-activate-1",
            artifact_digest,
            now_millis,
        )),
    )
    .await
    .expect("initial resume fixture activation timed out")
    .unwrap();
    let mut pump = ActivationPump::new(
        FixtureAdapter::new(
            binding_digest,
            owner.runtime_epoch,
            owner.session_id.clone(),
            external_session.clone(),
        )
        .unwrap(),
        CoordinatorActivationJournal::new(lease),
        Mapper,
        ActivationIntent {
            binding_digest,
            artifact_digest: artifact_digest.into(),
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            mode: ActivationMode::New {
                native_root: project_root.to_string_lossy().into_owned(),
                model: None,
            },
            curated_ceiling: capabilities.clone(),
            sage_policy: capabilities,
        },
    )
    .unwrap();
    let (mut transport, counts) = CompleteTransport::new();
    pump.begin().unwrap();
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        ActivationFlush::Written
    );
    tokio::time::timeout(
        deadline.operation_before_cleanup(),
        pump.push_stdout_chunk(
            &frame("initializeSuccess", serde_json::json!({})),
            now_millis + 1,
        ),
    )
    .await
    .expect("initial resume fixture initialize commit timed out")
    .unwrap();
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        ActivationFlush::Written
    );
    tokio::time::timeout(
        deadline.operation_before_cleanup(),
        pump.push_stdout_chunk(
            &frame(
                "newSessionSuccess",
                serde_json::json!({"externalSessionId": external_session.as_str()}),
            ),
            now_millis + 2,
        ),
    )
    .await
    .expect("initial resume fixture session commit timed out")
    .unwrap();
    assert_eq!(
        (
            counts.initialize(),
            counts.new_session(),
            counts.resume(),
            counts.prompt(),
            counts.other()
        ),
        (1, 1, 0, 0, 0)
    );
    let parts = pump.try_into_idle_parts().ok().unwrap();
    assert_eq!(
        parts.session().external_session_id,
        external_session,
        "new-session identity must come through SessionReady"
    );
    let (_, journal, _, _) = parts.into_parts();
    let (lease, handoff) = match journal.into_handoff() {
        Ok(value) => value,
        Err(_) => panic!("new-session activation did not durably reach ready"),
    };
    assert_eq!(handoff, CoordinatorActivationHandoff::Ready);
    lease
}

pub(super) fn prove_prior_zero_process(
    case_id: &'static str,
    runtime_epoch: RuntimeEpoch,
    fixture_path: PathBuf,
    artifact_digest: [u8; 32],
    fixture_byte_len: usize,
    owner: RuntimeOwner,
    deadline: FixtureDeadline,
) -> (Arc<TreeCapacityPool>, ZeroProcessProof) {
    let capacity = TreeCapacityPool::isolated_ade_0006a();
    let spec = launch_spec(
        case_id,
        runtime_epoch,
        fixture_path,
        artifact_digest,
        fixture_byte_len,
    );
    let process_lease = capacity.try_acquire(spec.limits.descendants).unwrap();
    let tree = launch_with_deadline(spec, process_lease, deadline).unwrap();
    let (stdin, stdout, control) = tree.into_parts();
    let mut cleanup = ProcessCleanup::new(control.clone(), None, deadline);
    control.request_termination(TreeExit::Cancelled).unwrap();
    let (_, zero) = control.wait_zero_within(deadline.tree()).unwrap();
    cleanup.finish_after_zero().unwrap();
    drop((stdin, stdout, control, cleanup));
    let proof = ZeroProcessProof::from_supervisor(owner, &zero).unwrap();
    (capacity, proof)
}

pub(super) fn seed_historical_turn(
    database: &Database,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    session_id: &SageAgentSessionId,
    runtime_epoch: RuntimeEpoch,
    now_millis: i64,
) -> Vec<HistoricalTurn> {
    let page = load_chat_page(
        &database.connection(),
        project_id.clone(),
        chat_id.clone(),
        None,
    )
    .unwrap();
    let mut connection = database.connection();
    let prepared = prepare_turn(
        &mut connection,
        PrepareTurnCommand {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::parse("resume-fixture-history").unwrap(),
            prompt: Prompt::new("prompt", "immutable historical prompt").unwrap(),
            now_millis,
        },
    )
    .unwrap();
    let turn_id = prepared.turn_id;
    let sending = begin_send(
        &mut connection,
        TurnMutationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
            turn_id: turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: now_millis + 1,
        },
    )
    .unwrap();
    let terminal = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
            turn_id,
            expected_revision: sending.revision,
            now_millis: now_millis + 2,
        },
        SendObservation::PossibleBytes,
    )
    .unwrap();
    assert_eq!(
        (terminal.state, terminal.delivery_state),
        (TurnState::DeliveryUnknown, DeliveryState::PossibleBytes)
    );
    drop(connection);
    let history = historical_turns(database, session_id);
    assert_eq!(history.len(), 1);
    history
}

pub(super) fn historical_turns(
    database: &Database,
    session_id: &SageAgentSessionId,
) -> Vec<HistoricalTurn> {
    let connection = database.connection();
    let mut statement = connection
        .prepare(
            "SELECT id,state,delivery_state FROM turns WHERE session_id=?1 ORDER BY ordinal,id",
        )
        .unwrap();
    statement
        .query_map([session_id.as_str()], |row| {
            Ok(HistoricalTurn {
                id: row.get(0)?,
                state: row.get(1)?,
                delivery_state: row.get(2)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

pub(super) fn persisted_capabilities(
    database: &Database,
    session_id: &SageAgentSessionId,
) -> String {
    database
        .connection()
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

pub(super) fn activation_command(
    owner: &RuntimeOwner,
    expected_projection_revision: ProjectionRevision,
    client_intent_id: &str,
    artifact_digest: &str,
    now_millis: i64,
) -> ActivateSessionCommand {
    ActivateSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision,
        client_intent_id: ClientIntentId::parse(client_intent_id).unwrap(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        artifact_digest: artifact_digest.into(),
        now_millis,
    }
}

fn frame(method: &str, params: serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": format!("fixture/{method}"),
        "params": params,
    }))
    .unwrap();
    bytes.push(b'\n');
    bytes
}
