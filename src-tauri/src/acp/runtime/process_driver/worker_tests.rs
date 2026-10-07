use crate::acp::{
    contract::{
        ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageAgentSessionId,
        SageChatId, SageProjectId, SageTurnId,
    },
    intent::Prompt,
    runtime::{HostCancelIntent, HostError, HostPromptIntent, LiveRuntimePort, LiveWrite},
    scheduler::RuntimeOwner,
    store::SendObservation,
};

use super::write_fence::WriteFence;
use super::*;

struct TestClock;

impl DriverClock for TestClock {
    fn now_millis(&self) -> i64 {
        10
    }
}

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-worker").unwrap(),
        chat_id: SageChatId::parse("chat-worker").unwrap(),
        session_id: SageAgentSessionId::parse("session-worker").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
}

fn prompt(owner: &RuntimeOwner) -> HostPromptIntent {
    HostPromptIntent {
        command: crate::acp::store::PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("intent-worker").unwrap(),
            prompt: Prompt::new("prompt", "hello").unwrap(),
            now_millis: 10,
        },
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    }
}

#[test]
fn immediate_stdout_stays_queued_until_the_durable_write_observation_control_wakes() {
    let mailbox = DriverMailbox::new().unwrap();
    let active = SageTurnId::parse("turn-worker").unwrap();
    let mut fence = WriteFence::bound(active.clone());
    fence.begin_write(&active).unwrap();

    mailbox
        .try_push_chunk(br#"{"early":true}\n"#.to_vec())
        .unwrap();
    let response = mailbox
        .submit_worker(WorkerCommand::ObserveSend {
            owner: owner(),
            turn_id: active.clone(),
            observation: SendObservation::CompleteBytes,
        })
        .unwrap();

    let observed = mailbox
        .wait_control_until(None, &TestClock)
        .unwrap()
        .expect("reserved write observation");
    let ControlMessage::Worker(envelope) = observed else {
        panic!("stdout must not cross the closed fence");
    };
    let WorkerCommand::ObserveSend {
        owner: observed_owner,
        turn_id,
        observation,
    } = &envelope.command
    else {
        panic!("write observation control");
    };
    assert_eq!(observed_owner, &owner());
    fence.accept_observation(turn_id, *observation).unwrap();
    envelope.respond(Ok(WorkerReply::Unit));
    assert!(matches!(
        response.recv().unwrap().unwrap(),
        WorkerReply::Unit
    ));
    assert!(fence.stdout_allowed());

    assert!(matches!(
        mailbox
            .wait_next_until(None, &TestClock)
            .unwrap(),
        Some(ControlMessage::StdoutChunk(chunk)) if chunk == br#"{"early":true}\n"#
    ));
}

#[test]
fn process_live_port_uses_the_same_bounded_wake_source_for_prompt_write_and_observation() {
    let mailbox = std::sync::Arc::new(DriverMailbox::new().unwrap());
    let worker_mailbox = std::sync::Arc::clone(&mailbox);
    let runtime_owner = owner();
    let thread_owner = runtime_owner.clone();
    let intent = prompt(&runtime_owner);
    let caller = std::thread::spawn(move || {
        let mut port = ProcessLivePort::new(
            thread_owner.clone(),
            worker_mailbox,
            std::sync::Arc::new(|_| {}),
        );
        let turn_id = port.prepare_prompt(&intent).unwrap();
        let write = port.dispatch_prompt(&thread_owner, &turn_id).unwrap();
        port.observe_send(&thread_owner, &turn_id, SendObservation::CompleteBytes)
            .unwrap();
        let cancel = HostCancelIntent {
            owner: thread_owner.clone(),
            turn_id: turn_id.clone(),
            client_intent_id: ClientIntentId::parse("intent-cancel-worker").unwrap(),
            now_millis: 11,
        };
        port.begin_cancel(&cancel).unwrap();
        port.dispatch_cancel(&cancel).unwrap();
        (turn_id, write)
    });

    let prepare = mailbox
        .wait_next_until(None, &TestClock)
        .unwrap()
        .expect("prepare wake");
    let ControlMessage::Worker(prepare) = prepare else {
        panic!("worker prepare");
    };
    assert!(matches!(
        &prepare.command,
        WorkerCommand::PreparePrompt(intent) if intent.command.prompt.as_str() == "hello"
    ));
    let active = SageTurnId::parse("turn-worker-live").unwrap();
    prepare.respond(Ok(WorkerReply::Prepared(active.clone())));

    let dispatch = mailbox
        .wait_next_until(None, &TestClock)
        .unwrap()
        .expect("dispatch wake");
    let ControlMessage::Worker(dispatch) = dispatch else {
        panic!("worker dispatch");
    };
    assert!(matches!(
        &dispatch.command,
        WorkerCommand::DispatchPrompt { owner, turn_id }
            if owner == &runtime_owner && turn_id == &active
    ));
    mailbox.try_push_chunk(b"early\n".to_vec()).unwrap();
    dispatch.respond(Ok(WorkerReply::Write(LiveWrite::Complete)));

    let observation = mailbox
        .wait_control_until(None, &TestClock)
        .unwrap()
        .expect("observation wakes ahead of stdout");
    let ControlMessage::Worker(observation) = observation else {
        panic!("worker observation");
    };
    assert!(matches!(
        &observation.command,
        WorkerCommand::ObserveSend { owner, turn_id, observation }
            if owner == &runtime_owner
                && turn_id == &active
                && *observation == SendObservation::CompleteBytes
    ));
    observation.respond(Ok(WorkerReply::Unit));

    let begin_cancel = mailbox
        .wait_control_until(None, &TestClock)
        .unwrap()
        .expect("durable cancel wake");
    let ControlMessage::Worker(begin_cancel) = begin_cancel else {
        panic!("worker cancel begin");
    };
    assert!(matches!(
        &begin_cancel.command,
        WorkerCommand::BeginCancel(intent)
            if intent.owner == runtime_owner && intent.turn_id == active
    ));
    begin_cancel.respond(Ok(WorkerReply::Unit));

    let dispatch_cancel = mailbox
        .wait_control_until(None, &TestClock)
        .unwrap()
        .expect("cancel dispatch wake");
    let ControlMessage::Worker(dispatch_cancel) = dispatch_cancel else {
        panic!("worker cancel dispatch");
    };
    assert!(matches!(
        &dispatch_cancel.command,
        WorkerCommand::DispatchCancel(intent)
            if intent.owner == runtime_owner && intent.turn_id == active
    ));
    dispatch_cancel.respond(Ok(WorkerReply::Unit));

    assert!(matches!(
        mailbox.wait_next_until(None, &TestClock).unwrap(),
        Some(ControlMessage::StdoutChunk(chunk)) if chunk == b"early\n"
    ));
    assert_eq!(caller.join().unwrap(), (active, LiveWrite::Complete));
}

#[test]
fn closing_the_driver_mailbox_rejects_queued_callers_and_wakes_both_wait_modes() {
    let mailbox = DriverMailbox::new().unwrap();
    let prompt_response = mailbox
        .submit_prompt(
            Prompt::new("prompt", "queued").unwrap(),
            crate::acp::contract::RequiredText::new("message", "queued-message").unwrap(),
        )
        .unwrap();
    let worker_response = mailbox
        .submit_worker(WorkerCommand::ObserveSend {
            owner: owner(),
            turn_id: SageTurnId::parse("turn-worker").unwrap(),
            observation: SendObservation::CompleteBytes,
        })
        .unwrap();

    mailbox.close();

    assert!(matches!(
        prompt_response.recv().unwrap(),
        Err(DriverError::Mailbox(MailboxError::Closed))
    ));
    assert!(matches!(
        worker_response.recv().unwrap(),
        Err(HostError::Closed)
    ));

    let mailbox = std::sync::Arc::new(DriverMailbox::new().unwrap());
    let ordinary_mailbox = std::sync::Arc::clone(&mailbox);
    let ordinary = std::thread::spawn(move || ordinary_mailbox.wait_next_until(None, &TestClock));
    let control_mailbox = std::sync::Arc::clone(&mailbox);
    let control = std::thread::spawn(move || control_mailbox.wait_control_until(None, &TestClock));

    mailbox.close();

    assert!(matches!(
        ordinary.join().unwrap(),
        Err(MailboxError::Closed)
    ));
    assert!(matches!(control.join().unwrap(), Err(MailboxError::Closed)));

    let mailbox = std::sync::Arc::new(DriverMailbox::new().unwrap());
    let mut queued = 0_u64;
    loop {
        match mailbox.try_push_chunk(vec![b'x']) {
            Ok(()) => queued += 1,
            Err(MailboxError::Full) => break,
            Err(error) => panic!("unexpected mailbox fill error: {error:?}"),
        }
    }
    assert!(queued > 0);
    assert!(queued <= crate::acp::limits::LIMITS.queues.driver_data_messages);
    let producer_mailbox = std::sync::Arc::clone(&mailbox);
    let (started, waiting) = std::sync::mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        started.send(()).unwrap();
        producer_mailbox.push_chunk_blocking(vec![b'y'])
    });
    waiting.recv().unwrap();
    mailbox.close();
    assert!(matches!(
        producer.join().unwrap(),
        Err(MailboxError::Closed)
    ));
}
