use super::*;
use crate::acp::contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId};

fn service() -> AcpService {
    let database =
        crate::persistence::Database::open(std::path::Path::new(":memory:")).expect("database");
    AcpService::new(database).expect("service")
}

fn owner(session: &str, epoch: u64) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-lanes").unwrap(),
        chat_id: SageChatId::parse(format!("chat-{session}")).unwrap(),
        session_id: SageAgentSessionId::parse(format!("session-{session}")).unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}

#[test]
fn the_lane_count_is_the_frozen_live_tree_limit_and_above_one() {
    let service = service();
    assert_eq!(service.lanes.len(), LIVE_RUNTIME_LANES);
    assert_eq!(
        LIVE_RUNTIME_LANES as u64,
        crate::acp::limits::LIMITS.process.operational_live_trees
    );
    const { assert!(LIVE_RUNTIME_LANES > 1, "B2 requires concurrent live chats") };
}

#[test]
fn concurrent_activations_claim_distinct_lanes_until_capacity_is_reached() {
    let service = service();
    let mut claims = Vec::new();
    for index in 0..LIVE_RUNTIME_LANES {
        let candidate = owner(&format!("a{index}"), 1);
        let (lane, claim) = service
            .claim_activation_lane(&candidate)
            .expect("a free lane");
        lane.runtime.reserve_start(candidate.clone()).unwrap();
        assert_eq!(
            service.lane_for(&candidate).unwrap().runtime.owner(),
            Some(candidate)
        );
        claims.push(claim);
    }
    assert!(matches!(
        service.claim_activation_lane(&owner("overflow", 1)),
        Err(CodexSessionError::Capacity)
    ));
    drop(claims);
    // Starting lanes are neither free, recoverable, nor idle: still bounded.
    assert!(matches!(
        service.claim_activation_lane(&owner("overflow", 1)),
        Err(CodexSessionError::Capacity)
    ));
}

#[test]
fn a_reactivated_session_returns_to_the_lane_that_holds_its_older_epoch() {
    let service = service();
    let first = owner("same", 1);
    let (lane, claim) = service.claim_activation_lane(&owner("other", 1)).unwrap();
    drop(claim);
    drop(lane);
    let (held, claim) = service.claim_activation_lane(&first).unwrap();
    held.runtime.reserve_start(first.clone()).unwrap();
    drop(claim);
    let (again, _claim) = service.claim_activation_lane(&owner("same", 2)).unwrap();
    assert!(Arc::ptr_eq(&again.runtime, &held.runtime));
}

#[test]
fn an_unknown_owner_routes_to_no_lane() {
    let service = service();
    let held = owner("held", 1);
    service
        .lane_at(1)
        .runtime
        .reserve_start(held.clone())
        .unwrap();
    assert!(service.lane_for(&owner("unknown", 1)).is_none());
    assert!(matches!(
        service.host_for(&owner("unknown", 1)),
        Err(RuntimeSlotError::Foreign)
    ));
    assert!(Arc::ptr_eq(
        &service.lane_for(&held).unwrap().runtime,
        &service.lane_at(1).runtime
    ));
}

#[test]
fn durable_activation_admits_at_most_the_live_tree_limit() {
    use crate::acp::{
        contract::ClientIntentId,
        store::{ActivateSessionCommand, StartSessionCommand, StoreError},
    };
    tauri::async_runtime::block_on(async {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id.clone()).unwrap();
        let service = AcpService::new_with_registry_publisher_for_test(
            database.clone(),
            Arc::new(crate::acp::bindings::registry::BindingRegistry::default()),
            Arc::new(AlwaysPublished),
        )
        .unwrap();
        // Chats are created before any session binds a test-only harness.
        let chats: Vec<SageChatId> = (0..=LIVE_RUNTIME_LANES)
            .map(|_| {
                let snapshot = database.bootstrap_snapshot().unwrap();
                SageChatId::parse(
                    database
                        .create_chat(&crate::domain::ProjectLayoutRequest {
                            project_id: project.id.clone(),
                            tabs: snapshot.tabs,
                            active_tab_id: snapshot.active_tab_id,
                            selected_root_id: snapshot.selected_root_id,
                            selected_resource: snapshot.selected_resource,
                        })
                        .unwrap(),
                )
                .unwrap()
            })
            .collect();
        let mut leases = Vec::new();
        for (index, chat_id) in chats.into_iter().enumerate() {
            let revision = service
                .load_chat_page(project_id.clone(), chat_id.clone(), None)
                .unwrap()
                .projection_revision;
            let started = service
                .session_starts
                .start_session(StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "test-lanes")
                        .unwrap(),
                    binding_digest: "11".repeat(32),
                    harness_version: None,
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                })
                .await
                .unwrap();
            let command = ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id,
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id,
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                artifact_digest: "22".repeat(32),
                now_millis: crate::persistence::now_millis(),
            };
            if index < LIVE_RUNTIME_LANES {
                leases.push(
                    service
                        .lane_at(index)
                        .sessions
                        .activate_session(command)
                        .await
                        .unwrap(),
                );
            } else {
                // A coordinator with a free scheduler still meets the durable
                // bound: the store refuses a tree beyond the limit.
                let failure = service
                    .session_starts
                    .activate_session(command)
                    .await
                    .expect_err("the store refuses a live tree beyond the limit");
                assert!(matches!(
                    failure.error,
                    crate::acp::runtime::SessionCoordinatorError::Journal(
                        crate::acp::runtime::JournalError::Store(StoreError::Conflict {
                            resource: "runtimeEpoch"
                        })
                    )
                ));
            }
        }
        assert_eq!(leases.len(), LIVE_RUNTIME_LANES);
    });
}
struct AlwaysPublished;

impl crate::acp::runtime::ProjectionPublisher for AlwaysPublished {
    fn publish_committed(
        &self,
        _: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<
        crate::acp::runtime::ProjectionPublishStatus,
        crate::acp::runtime::ProjectionPublishError,
    > {
        Ok(crate::acp::runtime::ProjectionPublishStatus::Published)
    }
}
