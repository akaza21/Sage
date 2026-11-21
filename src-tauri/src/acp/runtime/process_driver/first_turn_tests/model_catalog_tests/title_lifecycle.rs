use super::*;
use crate::acp::transport::IncrementalFrameDecoder;
use crate::acp::{
    bindings::{registry::TitleCapability, runtime::NativeRuntime},
    service::chat_title::TitleScheduler,
};

#[test]
fn title_generation_survives_post_completion_usage_reads_without_changing_the_main_transcript() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture = prepare_first_turn_fixture().await;
        fixture.database.connection().execute_batch(&format!(
            "UPDATE chats SET title_source='default' WHERE id='chat-first-turn';
             INSERT INTO workbench_tabs(id,project_id,kind,title,position,created_at,updated_at)
             VALUES('title-lifecycle-tab','{}','chat','Chat: Chat',1,1,1);
             INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
             VALUES('{}','chat-first-turn','title-lifecycle-tab',1);",fixture.project_id.as_str(),fixture.project_id.as_str())).unwrap();
        let mut adapter = NativeRuntime::new(CodexRuntime::fixture_with_identity(
            Platform::WindowsX86_64,[0x11;32],fixture.started.session_id.clone()));
        adapter.attach_titles(TitleScheduler::new(fixture.database.clone(),fixture.publisher.clone(),TitleCapability::GenerateIsolated, Arc::default()));
        let adapter = ready_adapter(adapter,&fixture.external_session,true);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(adapter,fixture.activation_journal,Mapper,ActivatedSession {
                binding_digest:[0x11;32],project_id:fixture.project_id.clone(),chat_id:fixture.chat_id.clone(),
                session_id:fixture.started.session_id.clone(),external_session_id:fixture.external_session.clone(),
                runtime_epoch:fixture.epoch,capabilities:fixture.capabilities,models:Vec::new(),next_model_cursor:None }),
            idle_decoder:IncrementalFrameDecoder::new(),transport:CatalogTransport::new(&writes,Arc::clone(&mailbox),None),
            control:NoChildProcessControl {zero_checks:Arc::new(AtomicUsize::new(0)),harness_error:None},
            owner:fixture.owner.clone(),clock:Arc::new(SystemDriverClock),
        };
        let context = FirstTurnWorkerContext {
            owner:fixture.owner.clone(), coordinator:SendCoordinator::new(Arc::new(Authority(IntentSnapshot {
                project_id:fixture.project_id.clone(),project_generation:ProjectGeneration::ZERO,chat_id:fixture.chat_id.clone(),
                projection_revision:fixture.ready_projection_revision,intent_revision:fixture.ready_projection_revision,
                session_id:Some(fixture.started.session_id.clone()),runtime_epoch:Some(fixture.epoch),
                turn_id:None,turn_state:None,turn_revision:None })),fixture.journal.clone(),fixture.publisher.clone()),
            journal:fixture.journal.clone(),publisher:fixture.publisher.clone(),clock:Arc::new(SystemDriverClock),
            mailbox:Arc::clone(&mailbox),session_recovery:None,zero_sequence:std::sync::atomic::AtomicU64::new(0),
        };
        let prepare = mailbox.submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
            command:PrepareTurnCommand {project_id:fixture.project_id.clone(),project_generation:ProjectGeneration::ZERO,
                chat_id:fixture.chat_id.clone(),session_id:fixture.started.session_id.clone(),runtime_epoch:fixture.epoch,
                expected_projection_revision:fixture.ready_projection_revision,client_intent_id:ClientIntentId::parse("title-first-prompt").unwrap(),
                prompt:Prompt::new("prompt","Can you please look into why the sidebar shows empty chats").unwrap(),now_millis:fixture.now_millis+3},
            access_mode:sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly })).unwrap();
        let ControlMessage::Worker(envelope) = mailbox.wait_next_until(None,context.clock.as_ref()).unwrap().unwrap() else { panic!("prepare") };
        let mut state = FirstTurnWorkerState::Idle(Box::new(completed)).handle(&context,envelope).await;
        let WorkerReply::Prepared(turn_id) = prepare.recv().unwrap().unwrap() else { panic!("prepared") };
        for command in [WorkerCommand::DispatchPrompt {owner:fixture.owner.clone(),turn_id:turn_id.clone()},
            WorkerCommand::ObserveSend {owner:fixture.owner.clone(),turn_id,observation:crate::acp::store::SendObservation::CompleteBytes}] {
            let reply = mailbox.submit_worker(command).unwrap();
            let ControlMessage::Worker(envelope) = mailbox.wait_next_until(None,context.clock.as_ref()).unwrap().unwrap() else { panic!("turn") };
            state = state.handle(&context,envelope).await;
            assert!(reply.recv().unwrap().is_ok());
        }
        let FirstTurnWorkerState::Active(mut active) = state else { panic!("active") };
        let prompt = writes.lock().unwrap().last().unwrap().clone();
        for event in [
            json!({"id":prompt["id"],"result":{"turn":{"id":"main-turn"}}}),
            json!({"method":"turn/started","params":{"threadId":fixture.external_session.as_str(),"turn":{"id":"main-turn","status":"inProgress"}}}),
            json!({"method":"item/completed","params":{"threadId":fixture.external_session.as_str(),"turnId":"main-turn","item":{"id":"main-answer","type":"agentMessage","text":"Check the sidebar filters for empty chat placeholders."}}}),
            json!({"method":"turn/completed","params":{"threadId":fixture.external_session.as_str(),"turn":{"id":"main-turn","status":"completed"}}}),
        ] {
            let mut bytes=serde_json::to_vec(&event).unwrap();bytes.push(b'\n');mailbox.try_push_chunk(bytes).unwrap();
            active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
        }
        let cursor=active.turn.driver.latest_durable_turn_cursor().unwrap().clone();
        assert_eq!(cursor.state(),TurnState::Completed);
        // This is the runner iteration between durable completion and its
        // accepted handoff. It must not cancel a newly scheduled operation.
        active.turn.driver.poll_title();
        let attempted:i64=fixture.database.connection().query_row("SELECT count(*) FROM session_facts WHERE kind='chatTitleAttempt'",[],|r|r.get(0)).unwrap();
        assert_eq!(attempted,0);
        let active=active.accept_terminal(&cursor).unwrap_or_else(|_|panic!("accept terminal"));
        let reusable=active.try_into_completed(mailbox.as_ref()).unwrap_or_else(|_|panic!("reusable"));
        let before:(i64,i64)=fixture.database.connection().query_row("SELECT (SELECT count(*) FROM chats),(SELECT count(*) FROM items)",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        let mut answered=writes.lock().unwrap().len();
        // The UI automatically refreshes usage immediately after completion.
        // This read must coexist with the title operation, not cancel it.
        let reply=mailbox.submit_worker(WorkerCommand::NativeRpc {
            intent:crate::acp::goal::NativeRpcIntent {owner:fixture.owner.clone(),request:crate::acp::goal::NativeRpcRequest::UsageRead},
            completion:crate::acp::runtime::NativeRpcCompletionSink::new(|_|{}),
        }).unwrap();
        let ControlMessage::Worker(envelope)=mailbox.wait_next_until(None,context.clock.as_ref()).unwrap().unwrap() else {panic!("usage read")};
        let state=super::super::super::live_runner::idle_command_for_test(FirstTurnWorkerState::Reusable(Box::new(reusable)),&context,envelope).await;
        assert!(reply.recv().unwrap().is_ok());
        let FirstTurnWorkerState::Reusable(mut reusable)=state else {panic!("still reusable")};
        for _ in 0..8 {
            let pending=reusable.completed.poll_title(false,&mut reusable.transport);
            let requests=writes.lock().unwrap()[answered..].to_vec();answered+=requests.len();
            for request in requests {
                let events=if request["method"]=="account/rateLimits/read" {
                    vec![json!({"id":request["id"],"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":64}},"ordinaryUsageAllowed":true}})]
                }else{crate::acp::fixture::title_wire(&request)};
                for event in events {
                    let mut bytes=serde_json::to_vec(&event).unwrap();bytes.push(b'\n');
                    reusable.completed.accept_idle_stdout_chunk(&bytes,fixture.now_millis+10,&mut reusable.transport).await.unwrap();
                }
            }
            if !pending { break }
        }
        let title:String=fixture.database.connection().query_row("SELECT title FROM chats WHERE id='chat-first-turn'",[],|r|r.get(0)).unwrap();
        assert_eq!(title,"Chat: Investigate empty sidebar chats");
        let after:(i64,i64)=fixture.database.connection().query_row("SELECT (SELECT count(*) FROM chats),(SELECT count(*) FROM items)",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(before,after);
        assert_eq!(before.1,2,"only the original prompt and final answer are durable");
    });
}
