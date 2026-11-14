use super::*;

#[path = "lifecycle_cases.rs"]
mod cases;
use cases::CatalogFailure;

fn catalog_page_crossing_turn(
    cold_prompt: bool,
    reply_during_turn: bool,
    failure: Option<CatalogFailure>,
    deferred: bool,
) {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_catalog_page_crossing_turn(
            cold_prompt,
            reply_during_turn,
            failure,
            deferred,
            None,
            true,
        ));
}

async fn run_catalog_page_crossing_turn(
    cold_prompt: bool,
    reply_during_turn: bool,
    failure: Option<CatalogFailure>,
    deferred: bool,
    invalid_page: Option<Value>,
    confirmed_config: bool,
) {
    let fixture = prepare_first_turn_fixture().await;
    let channel = Arc::new(crate::acp::session_config::ConfigChannel::default());
    channel.reserve(fixture.owner.clone());
    let mut adapter =
        crate::acp::bindings::runtime::NativeRuntime::new(CodexRuntime::fixture_with_identity(
            Platform::WindowsX86_64,
            [0x11; 32],
            fixture.started.session_id.clone(),
        ));
    adapter.attach_config(fixture.owner.clone(), Arc::clone(&channel));
    let adapter = ready_adapter(adapter, &fixture.external_session, confirmed_config);
    assert!(channel.live_snapshot(&fixture.owner).is_none());
    channel.publish(&fixture.owner);
    let initial_config = channel.live_snapshot(&fixture.owner);
    assert_eq!(initial_config.is_some(), confirmed_config);
    if let Some(overlay) = &initial_config {
        assert_eq!(overlay.config_revision.get(), 1);
        assert_eq!(
            serde_json::to_value(&overlay.config).unwrap(),
            json!({
                "modes":null,"configOptions":[],
                "confirmedModel":{"id":"selected"},"confirmedEffort":"high"
            })
        );
    }
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let mut completed = CompletedActivationDriverParts {
        parts: IdleActivationParts::new(
            adapter,
            fixture.activation_journal,
            Mapper,
            ActivatedSession {
                binding_digest: [0x11; 32],
                project_id: fixture.project_id.clone(),
                chat_id: fixture.chat_id.clone(),
                session_id: fixture.started.session_id.clone(),
                external_session_id: fixture.external_session.clone(),
                runtime_epoch: fixture.epoch,
                capabilities: fixture.capabilities,
                models: Vec::new(),
                next_model_cursor: None,
            },
        ),
        idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
        transport: CatalogTransport::new(&writes, Arc::clone(&mailbox), None),
        control: NoChildProcessControl {
            zero_checks: Arc::new(AtomicUsize::new(0)),
        },
        owner: fixture.owner.clone(),
        clock: Arc::new(SystemDriverClock),
    };
    completed
        .parts
        .hydrate_model_config(&mut completed.transport);
    let first_id = writes.lock().unwrap()[0]["id"].as_u64().unwrap();
    let page_id = if cold_prompt {
        first_id
    } else {
        let first_model = if failure.is_some() {
            json!({"id":"selected","displayName":"Selected","isDefault":false,
            "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                {"reasoningEffort":"high","description":"Deep"}]})
        } else {
            json!({"id":"other","displayName":"Other","isDefault":false})
        };
        let mut first_page = serde_json::to_vec(&json!({"id":first_id,"result":{
            "data":[first_model],"nextCursor":"second"
        }}))
        .unwrap();
        first_page.push(b'\n');
        completed.accept_idle_stdout_chunk(&first_page).unwrap();
        writes.lock().unwrap()[1]["id"].as_u64().unwrap()
    };
    let retained_config = channel.live_snapshot(&fixture.owner);
    assert_eq!(retained_config.is_some(), confirmed_config);
    if cold_prompt {
        assert_eq!(retained_config, initial_config);
    }
    if let Some(overlay) = &retained_config {
        let config = serde_json::to_value(&overlay.config).unwrap();
        assert_eq!(config["confirmedModel"]["id"], "selected");
        assert_eq!(config["confirmedEffort"], "high");
        // Once a first page is read the thread's model is offered, listed or
        // not; a page listing it also offers its effort.
        assert_eq!(
            config["configOptions"].as_array().unwrap().len(),
            usize::from(!cold_prompt) * (1 + usize::from(failure.is_some()))
        );
    }
    let _ = channel.take_notice();
    let error = invalid_page.map_or_else(
        || json!({"id":page_id,"error":{"code":-32601,"message":"Method not found"}}),
        |result| json!({"id":page_id,"result":result}),
    );
    if failure == Some(CatalogFailure::BeforePrompt) {
        let mut bytes = serde_json::to_vec(&error).unwrap();
        bytes.push(b'\n');
        for chunk in bytes.chunks(8_192) {
            completed.accept_idle_stdout_chunk(chunk).unwrap();
        }
        assert_eq!(
            completed.parts.session().models.len(),
            usize::from(!cold_prompt)
        );
        assert_eq!(channel.live_snapshot(&fixture.owner), retained_config);
        assert!(channel.take_notice().is_none());
    }
    let context = FirstTurnWorkerContext {
        owner: fixture.owner.clone(),
        coordinator: SendCoordinator::new(
            Arc::new(Authority(IntentSnapshot {
                project_id: fixture.project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: fixture.chat_id.clone(),
                projection_revision: fixture.ready_projection_revision,
                intent_revision: fixture.ready_projection_revision,
                session_id: Some(fixture.started.session_id.clone()),
                runtime_epoch: Some(fixture.epoch),
                turn_id: None,
                turn_state: None,
                turn_revision: None,
            })),
            fixture.journal.clone(),
            fixture.publisher.clone(),
        ),
        journal: fixture.journal.clone(),
        publisher: fixture.publisher,
        clock: Arc::new(SystemDriverClock),
        mailbox: Arc::clone(&mailbox),
        session_recovery: None,
        zero_sequence: std::sync::atomic::AtomicU64::new(0),
    };
    let mut state = FirstTurnWorkerState::Idle(Box::new(completed));
    let prepare = mailbox
        .submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
            command: PrepareTurnCommand {
                project_id: fixture.project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id: fixture.chat_id,
                session_id: fixture.started.session_id,
                runtime_epoch: fixture.epoch,
                expected_projection_revision: fixture.ready_projection_revision,
                client_intent_id: ClientIntentId::parse("intent-catalog-crossing").unwrap(),
                prompt: Prompt::new("prompt", "fast turn").unwrap(),
                now_millis: fixture.now_millis + 3,
            },
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }))
        .unwrap();
    let ControlMessage::Worker(envelope) = mailbox
        .wait_next_until(None, context.clock.as_ref())
        .unwrap()
        .unwrap()
    else {
        panic!("prepare command")
    };
    state = state.handle(&context, envelope).await;
    let WorkerReply::Prepared(turn_id) = prepare.recv().unwrap().unwrap() else {
        panic!("prepared turn")
    };
    for command in [
        WorkerCommand::DispatchPrompt {
            owner: fixture.owner.clone(),
            turn_id: turn_id.clone(),
        },
        WorkerCommand::ObserveSend {
            owner: fixture.owner.clone(),
            turn_id,
            observation: crate::acp::store::SendObservation::CompleteBytes,
        },
    ] {
        let reply = mailbox.submit_worker(command).unwrap();
        let ControlMessage::Worker(envelope) = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .unwrap()
        else {
            panic!("turn command")
        };
        state = state.handle(&context, envelope).await;
        assert!(reply.recv().unwrap().is_ok());
    }
    let FirstTurnWorkerState::Active(mut active) = state else {
        panic!("active turn")
    };
    let driver = &mut active.turn.driver;
    let written = writes.lock().unwrap().len();
    let prompt_request = writes.lock().unwrap().last().unwrap().clone();
    if confirmed_config {
        assert_eq!(prompt_request["params"]["effort"], "high");
    } else {
        assert!(prompt_request["params"].get("effort").is_none());
    }
    assert_eq!(prompt_request["method"], "turn/start");
    let prompt_id = prompt_request["id"].as_u64().unwrap();
    let external_turn_id = "fast-turn";
    let page = if failure.is_some() {
        error
    } else if deferred {
        json!({"id":page_id,"result":{
            "data":[{"id":"still-other","displayName":"Other","isDefault":false}],"nextCursor":"third"
        }})
    } else {
        json!({"id":page_id,"result":{
            "data":[{"id":"selected","displayName":"Selected","isDefault":false,
                "defaultReasoningEffort":"medium","supportedReasoningEfforts":[
                    {"reasoningEffort":"low","description":"Quick"},
                    {"reasoningEffort":"medium","description":"Balanced"},
                    {"reasoningEffort":"high","description":"Deep"}
                ]}],"nextCursor":"third"
        }})
    };
    let mut response =
        serde_json::to_vec(&json!({"id":prompt_id,"result":{"turn":{"id":external_turn_id}}}))
            .unwrap();
    response.push(b'\n');
    mailbox.try_push_chunk(response).unwrap();
    driver.run_next(mailbox.as_ref()).await.unwrap();
    if driver
        .latest_durable_turn_cursor()
        .is_none_or(|cursor| cursor.state() != TurnState::Sending)
    {
        driver.run_next(mailbox.as_ref()).await.unwrap();
    }
    assert_eq!(
        driver
            .latest_durable_turn_cursor()
            .expect("response correlation flushed")
            .state(),
        TurnState::Sending
    );
    assert_eq!(writes.lock().unwrap().len(), written);
    let mut started = serde_json::to_vec(&json!({"method":"turn/started","params":{
        "threadId":fixture.external_session.as_str(),"turn":{"id":external_turn_id,"status":"inProgress"}
    }})).unwrap();
    started.push(b'\n');
    mailbox.try_push_chunk(started).unwrap();
    driver.run_next(mailbox.as_ref()).await.unwrap();
    if driver
        .latest_durable_turn_cursor()
        .is_none_or(|cursor| cursor.state() != TurnState::Running)
    {
        driver.run_next(mailbox.as_ref()).await.unwrap();
    }
    let started_cursor = driver
        .latest_durable_turn_cursor()
        .expect("turn started flushed")
        .clone();
    assert_eq!(started_cursor.state(), TurnState::Running);
    assert_eq!(writes.lock().unwrap().len(), written);
    if reply_during_turn {
        let mut bytes = serde_json::to_vec(&page).unwrap();
        bytes.push(b'\n');
        for chunk in bytes.chunks(8_192) {
            mailbox.try_push_chunk(chunk.to_vec()).unwrap();
            driver.run_next(mailbox.as_ref()).await.unwrap();
        }
        assert_eq!(driver.latest_durable_turn_cursor(), Some(&started_cursor));
        assert_eq!(writes.lock().unwrap().len(), written);
    }
    let mut completed_turn = serde_json::to_vec(&json!({"method":"turn/completed","params":{
        "threadId":fixture.external_session.as_str(),"turn":{"id":external_turn_id,"status":"completed"}
    }})).unwrap();
    completed_turn.push(b'\n');
    mailbox.try_push_chunk(completed_turn).unwrap();
    driver.run_next(mailbox.as_ref()).await.unwrap();
    let mut reusable = active
        .try_into_completed(mailbox.as_ref())
        .unwrap_or_else(|_| panic!("reusable turn"));
    if !reply_during_turn && failure != Some(CatalogFailure::BeforePrompt) {
        let mut chunk = serde_json::to_vec(&page).unwrap();
        chunk.push(b'\n');
        for bytes in chunk.chunks(8_192) {
            reusable
                .completed
                .accept_idle_stdout_chunk(bytes, fixture.now_millis + 4, &mut reusable.transport)
                .await
                .unwrap();
        }
    }
    if deferred {
        assert_eq!(writes.lock().unwrap().len(), written);
        reusable
            .completed
            .hydrate_model_config(&mut reusable.transport);
        assert_eq!(writes.lock().unwrap().len(), written + 1);
        let request = writes.lock().unwrap().last().unwrap().clone();
        assert_eq!(request["method"], "model/list");
        assert_eq!(request["params"]["cursor"], "third");
        let overlay = channel.live_snapshot(&fixture.owner).unwrap();
        let config = serde_json::to_value(&overlay.config).unwrap();
        assert_eq!(config["confirmedModel"]["id"], "selected");
        // No page has listed the thread's model yet: it is offered by its
        // id, and no thinking level is known for it.
        assert_eq!(config["configOptions"].as_array().map(Vec::len), Some(1));
        assert_eq!(config["configOptions"][0]["category"], "model");
        assert_eq!(config["configOptions"][0]["currentValue"], "selected");
        let mut bytes = serde_json::to_vec(&json!({"id":request["id"],"result":{
            "data":[{"id":"selected","displayName":"Selected","isDefault":false,
            "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                {"reasoningEffort":"high","description":"Deep"}]}],"nextCursor":"fourth"
        }}))
        .unwrap();
        bytes.push(b'\n');
        reusable
            .completed
            .accept_idle_stdout_chunk(&bytes, fixture.now_millis + 5, &mut reusable.transport)
            .await
            .unwrap();
        assert_eq!(writes.lock().unwrap().len(), written + 1);
        reusable
            .completed
            .hydrate_model_config(&mut reusable.transport);
        let request = writes.lock().unwrap().last().unwrap().clone();
        assert_eq!(request["params"]["cursor"], "fourth");
        let mut bytes = serde_json::to_vec(&json!({"id":request["id"],"result":{
            "data":[],"nextCursor":"fifth"
        }}))
        .unwrap();
        bytes.push(b'\n');
        reusable
            .completed
            .accept_idle_stdout_chunk(&bytes, fixture.now_millis + 6, &mut reusable.transport)
            .await
            .unwrap();
        for _ in 0..2 {
            reusable
                .completed
                .hydrate_model_config(&mut reusable.transport);
        }
        assert_eq!(writes.lock().unwrap().len(), written + 2);
    } else {
        assert_eq!(writes.lock().unwrap().len(), written);
    }
    if failure.is_some() {
        reusable
            .completed
            .hydrate_model_config(&mut reusable.transport);
        assert_eq!(writes.lock().unwrap().len(), written);
        assert_eq!(channel.live_snapshot(&fixture.owner), retained_config);
        assert!(channel.take_notice().is_none());
    } else {
        let overlay = channel
            .live_snapshot(&fixture.owner)
            .expect("late catalog effort control");
        let config = serde_json::to_value(overlay.config).unwrap();
        assert_eq!(
            config["confirmedModel"],
            json!({"id":"selected","name":"Selected"})
        );
        assert_eq!(config["configOptions"][0]["currentValue"], "high");
        if !deferred {
            assert_eq!(
                config["configOptions"][0]["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|option| option["value"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["low", "medium", "high"]
            );
        }
    }
    drop(reusable);
    drop(context);
    drop(fixture.sessions);
    drop(fixture.scheduler);
    drop(fixture.journal);
    fixture.journal_task.finish();
}
