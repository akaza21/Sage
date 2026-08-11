use std::{
    fs,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RequiredText, RuntimeEpoch,
            SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
        },
        intent::{CapabilitySnapshotV1, ExternalId, IntentSnapshot, ProjectionChangedV1, Prompt},
        lifecycle::TurnState,
        runtime::{
            ActiveTurnIntent, AdapterBatchJournal, DurableAdapterEvent, HostCancelIntent,
            HostCloseIntent, HostError, HostPromptIntent, HostState, NotificationError,
            NotificationSink, NotificationSinkFuture, PrepareDispatch, ProjectIntentAuthority,
            PromptWrite, PublishOutcome, RuntimeEngine, RuntimeHost, SendCoordinator,
            SessionPumpIntent, SqliteAdapterBatchJournal, TurnScope,
        },
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, AdapterTurnCursor, CompleteActivationCommand,
            PrepareTurnCommand, StartSessionCommand,
        },
    },
    domain::{ProjectLayoutRequest, ProjectSwitchRequest},
    persistence::Database,
};

use super::AcpService;

#[cfg(windows)]
#[test]
fn service_and_catalog_share_the_exact_admitted_codex_snapshot() {
    let snapshot = crate::acp::bindings::codex::CodexBindingSnapshot::admitted_for_test();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let service = AcpService::new_with_codex_for_test(database, snapshot.clone()).unwrap();
    let catalog = crate::harnesses::HarnessCatalog::with_codex_snapshot(snapshot);

    assert!(Arc::ptr_eq(
        service.codex_binding().unwrap(),
        catalog.codex_binding().unwrap(),
    ));
}

#[test]
fn renderer_recreation_snapshot_covers_the_exact_native_invariance_boundary() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .connection()
        .execute_batch(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES('project-reload','Reload',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES('chat-reload','project-reload',0,'Reload',7,1,1);",
        )
        .unwrap();
    let service = AcpService::new(database.clone()).unwrap();
    let project_id = SageProjectId::parse("project-reload").unwrap();
    let chat_id = SageChatId::parse("chat-reload").unwrap();

    let before = service
        .renderer_recreation_native_snapshot(&project_id, &chat_id)
        .unwrap();
    let evidence_policy: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/evidence-policy-v1.json"
    ))
    .unwrap();
    let observation = &evidence_policy["rendererRecreationObservation"];
    let contracted_tables = observation["sqliteTables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        before.table_names().as_slice(),
        contracted_tables.as_slice()
    );
    assert_eq!(
        before.maximum_rows_per_table(),
        observation["maximumRowsPerTable"].as_u64().unwrap()
    );
    assert_eq!(before.table_rows("chats"), Some(1));
    assert_eq!(before.chat_projection_revision(), 7);
    assert_eq!(before.runtime_slot_active(), 0);
    assert_eq!(before.live_process_tree_reservations(), 0);
    assert_eq!(
        before,
        service
            .renderer_recreation_native_snapshot(&project_id, &chat_id)
            .unwrap()
    );

    database
        .connection()
        .execute(
            "UPDATE chats SET title='Changed' WHERE id='chat-reload'",
            [],
        )
        .unwrap();
    let changed = service
        .renderer_recreation_native_snapshot(&project_id, &chat_id)
        .unwrap();
    assert_ne!(before, changed);
    assert_eq!(changed.chat_projection_revision(), 7);
}

struct StaticAuthority(IntentSnapshot);

impl ProjectIntentAuthority for StaticAuthority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

struct GatedSink {
    events: Mutex<Vec<ProjectionChangedV1>>,
    entered: tokio::sync::Notify,
    completed: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

impl Default for GatedSink {
    fn default() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            entered: tokio::sync::Notify::new(),
            completed: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

impl NotificationSink for GatedSink {
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            self.entered.notify_one();
            self.release
                .acquire()
                .await
                .map_err(|_| NotificationError::SinkUnavailable)?
                .forget();
            self.completed.notify_one();
            Ok(())
        })
    }
}

struct Engine(Arc<Mutex<Vec<&'static str>>>);

impl RuntimeEngine for Engine {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        self.0.lock().unwrap().push("activate");
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.0.lock().unwrap().push("prepare");
        Ok(SageTurnId::parse("turn-service").unwrap())
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.0.lock().unwrap().push("dispatch");
        Ok(PromptWrite::Complete(turn_id.clone()))
    }
    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Ok(PromptWrite::Complete(turn_id.clone()))
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
        self.0.lock().unwrap().push("shutdown");
        Ok(())
    }
}

struct BlockingShutdownEngine(Arc<(Mutex<bool>, Condvar)>);

impl RuntimeEngine for BlockingShutdownEngine {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
        let (released, ready) = &*self.0;
        let mut released = released.lock().unwrap();
        while !*released {
            released = ready.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn service_keeps_background_turn_live_and_stops_runtime_before_journal_drop() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let runtime = Arc::new(RuntimeHost::start(Engine(Arc::clone(&events))).unwrap());
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-service").unwrap(),
        chat_id: SageChatId::parse("chat-service").unwrap(),
        session_id: SageAgentSessionId::parse("session-service").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    runtime.activate(owner.clone()).unwrap();
    runtime
        .send(HostPromptIntent {
            command: crate::acp::store::PrepareTurnCommand {
                project_id: owner.project_id.clone(),
                project_generation: ProjectGeneration::new(1).unwrap(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::parse("intent-service").unwrap(),
                prompt: Prompt::new("prompt", "hello").unwrap(),
                now_millis: crate::persistence::now_millis(),
            },
        })
        .unwrap();
    service.install_runtime_for_test(owner, Arc::clone(&runtime));

    service.project_switched(&SageProjectId::parse("project-other").unwrap());
    assert_eq!(runtime.state().unwrap(), HostState::Running);
    service.shutdown_blocking().unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        ["activate", "prepare", "dispatch", "shutdown"]
    );
}

#[test]
fn final_window_shutdown_returns_at_deadline_and_retries_the_same_actor() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = Arc::new(
        RuntimeHost::start_with_timeout(
            BlockingShutdownEngine(Arc::clone(&gate)),
            Duration::from_millis(20),
        )
        .unwrap(),
    );
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-timeout").unwrap(),
        chat_id: SageChatId::parse("chat-timeout").unwrap(),
        session_id: SageAgentSessionId::parse("session-timeout").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    runtime.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner, Arc::clone(&runtime));

    let started = Instant::now();
    assert_eq!(service.shutdown_blocking(), Err(HostError::Deadline));
    assert!(started.elapsed() < Duration::from_secs(1));

    let (released, ready) = &*gate;
    *released.lock().unwrap() = true;
    ready.notify_all();
    service.shutdown_blocking().unwrap();
}

#[test]
fn desktop_notifier_starts_without_an_entered_tokio_runtime() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let service =
        AcpService::new_with_notification_sink(database, Arc::new(GatedSink::default())).unwrap();
    service.shutdown_blocking().unwrap();
}

#[test]
fn running_turn_terminal_after_project_switch_is_durable_but_cannot_touch_active_projection() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base = std::env::temp_dir().join(format!("sage-acp-switch-{}", uuid::Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project_a = database.create_project_from_root(&root_a).unwrap();
        let project_b = database.create_project_from_root(&root_b).unwrap();
        let snapshot_b = database.bootstrap_snapshot().unwrap();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_a.id.clone(),
                current_layout: layout(&snapshot_b),
            })
            .unwrap();
        let project_a_id = SageProjectId::parse(project_a.id).unwrap();
        let project_b_id = SageProjectId::parse(project_b.id).unwrap();
        let chat_a = SageChatId::parse("chat-switch-a").unwrap();
        let chat_b = SageChatId::parse("chat-switch-b").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute_batch(&format!(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES ('{}','{}',1,'A',0,{now},{now});
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES ('{}','{}',1,'B',0,{now},{now});",
                chat_a.as_str(), project_a_id.as_str(), chat_b.as_str(), project_b_id.as_str()
            ))
            .unwrap();

        let sink = Arc::new(GatedSink::default());
        let entered = sink.entered.notified();
        let service = AcpService::new_with_notification_sink(database.clone(), sink.clone()).unwrap();
        let initial_a = service
            .load_chat_page(project_a_id.clone(), chat_a.clone(), None)
            .unwrap();
        let binding = "11".repeat(32);
        let artifact = "22".repeat(32);
        let started = service
            .sessions
            .start_session(StartSessionCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                expected_projection_revision: initial_a.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-switch-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-switch").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now + 1,
            })
            .await
            .unwrap();
        entered.await;
        let epoch = RuntimeEpoch::new(1).unwrap();
        let mut tree = service
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-switch-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-switch").unwrap();
        let ready = tree
            .complete_activation(CompleteActivationCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding.clone(),
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 3,
            })
            .await
            .unwrap();
        let snapshot = service
            .intent_snapshot(project_a_id.clone(), ProjectGeneration::ZERO, chat_a.clone())
            .unwrap();
        let sends = SendCoordinator::new(
            Arc::new(StaticAuthority(snapshot)),
            service.journal.clone(),
            service.notifier.as_ref().unwrap().clone(),
        );
        let prepared = tree
            .prepare_turn(
                &sends,
                PrepareTurnCommand {
                    project_id: project_a_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_a.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready.projection_revision,
                    client_intent_id: ClientIntentId::parse("intent-switch-turn").unwrap(),
                    prompt: Prompt::new("prompt", "continue in background").unwrap(),
                    now_millis: now + 4,
                },
            )
            .await
            .unwrap();
        let PrepareDispatch::Applied(mut dispatch) = prepared else {
            panic!("new Turn")
        };
        let sending = dispatch.begin_send(now + 5).await.unwrap();
        dispatch
            .observe(crate::acp::store::SendObservation::CompleteBytes, now + 6)
            .await
            .unwrap();

        let snapshot_a = database.bootstrap_snapshot().unwrap();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_b_id.as_str().to_owned(),
                current_layout: layout(&snapshot_a),
            })
            .unwrap();
        service.project_switched(&project_b_id);
        let before_b = service
            .load_chat_page(project_b_id.clone(), chat_b.clone(), None)
            .unwrap();
        sink.events.lock().unwrap().clear();
        let notifier = service.notifier.as_ref().unwrap();
        let wakes = notifier.scheduled_wakes();

        let active_turn = ActiveTurnIntent::store_minted(dispatch.turn_id().clone()).unwrap();
        let scope = TurnScope {
            project_id: project_a_id.clone(),
            chat_id: chat_a.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            turn_id: dispatch.turn_id().clone(),
            binding_digest: binding,
        };
        let mut adapter_journal = SqliteAdapterBatchJournal::new(
            service.journal.clone(),
            SessionPumpIntent {
                binding_digest: [0x11; 32],
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                session_id: started.session_id.clone(),
                external_session_id: external_session,
                runtime_epoch: epoch,
            }
            .bind_turn(active_turn),
        );
        let receipt = adapter_journal
            .commit(
                vec![
                    DurableAdapterEvent::Turn {
                        scope: scope.clone(),
                        state: TurnState::Running,
                        now_millis: now + 7,
                    },
                    DurableAdapterEvent::Turn {
                        scope,
                        state: TurnState::Completed,
                        now_millis: now + 8,
                    },
                ],
                2,
            )
            .await
            .unwrap();
        assert_eq!(
            notifier
                .publish_committed(ProjectionChangedV1 {
                    project_id: project_a_id.clone(),
                    chat_id: chat_a.clone(),
                    session_id: started.session_id.clone(),
                    projection_revision: receipt.projection_revision,
                })
                .unwrap(),
            PublishOutcome::SuppressedInactive
        );
        assert_eq!(notifier.scheduled_wakes(), wakes);
        dispatch.accept_adapter_commit(receipt.turn_cursor()).unwrap();

        let terminal: (String, Option<i64>) = database
            .connection()
            .query_row(
                "SELECT state,terminal_at FROM turns WHERE id=?1",
                [dispatch.turn_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(terminal, ("completed".into(), Some(now + 8)));
        let active = service.scheduler.active().unwrap();
        assert_eq!(active.tree.project_id, project_a_id);
        assert!(!active.turn_reserved);
        assert_eq!(active.turn_id, None);
        let after_b = service
            .load_chat_page(project_b_id, chat_b, None)
            .unwrap();
        assert_eq!(after_b, before_b);
        assert!(sink.events.lock().unwrap().is_empty());
        assert_eq!(notifier.recovery(), None);

        let completed = sink.completed.notified();
        sink.release.add_permits(1);
        completed.await;
        service.shutdown_blocking().unwrap();
        drop(service);
        drop(database);
        fs::remove_dir_all(base).unwrap();
        assert_eq!(sending.state, TurnState::Sending);
    });
}

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    }
}
