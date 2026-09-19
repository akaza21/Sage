use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    limits::LIMITS,
    runtime::{ConfigCompletionSink, LiveRuntimePort},
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigRevision},
};

use super::*;

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-deadline").unwrap(),
        chat_id: SageChatId::parse("chat-deadline").unwrap(),
        session_id: SageAgentSessionId::parse("session-deadline").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
}

#[test]
fn nonresponding_worker_returns_at_deadline_and_closes_its_mailbox() {
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let exits = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&exits);
    let abort: Arc<dyn WorkerDeadlineAbort> = Arc::new(move |reason| {
        recorded.lock().unwrap().push(reason);
    });
    let port = ProcessLivePort::new_with_timeout(owner(), Arc::clone(&mailbox), abort, 20);
    let started = Instant::now();
    assert!(matches!(
        port.call(WorkerCommand::Shutdown),
        Err(HostError::Deadline)
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(matches!(
        mailbox.submit_worker(WorkerCommand::Shutdown),
        Err(MailboxError::Closed)
    ));
    assert_eq!(*exits.lock().unwrap(), [TreeExit::Deadline]);
}

#[test]
fn configuration_begin_timeout_does_not_close_mailbox_or_terminate_tree() {
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let exits = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&exits);
    let abort: Arc<dyn WorkerDeadlineAbort> = Arc::new(move |reason| {
        recorded.lock().unwrap().push(reason);
    });
    let mut port = ProcessLivePort::new_with_timeout(owner(), Arc::clone(&mailbox), abort, 20);
    let completion = Arc::new(Mutex::new(None));
    let observed = Arc::clone(&completion);
    let result = port.begin_configure_session(
        &ConfigIntent {
            owner: owner(),
            expected_config_revision: ConfigRevision::first(),
            mutation: ConfigMutation::Mode(ConfigId::new("mode", "code").unwrap()),
        },
        Instant::now() + std::time::Duration::from_millis(20),
        ConfigCompletionSink::new(move |result| {
            *observed.lock().unwrap() = Some(result);
        }),
    );

    assert_eq!(result, Err(HostError::Deadline));
    assert!(mailbox.submit_worker(WorkerCommand::Shutdown).is_ok());
    assert!(exits.lock().unwrap().is_empty());
    mailbox.close();
    assert_eq!(
        completion.lock().unwrap().take(),
        Some(Err(HostError::Closed))
    );
}

#[test]
fn worker_commands_use_the_frozen_operation_deadlines() {
    assert_eq!(
        WorkerCommand::WaitZero { owner: owner() }.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
    assert_eq!(
        WorkerCommand::AcceptLatestCompleted.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
    assert_eq!(
        WorkerCommand::BeginPark.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
    assert_eq!(
        WorkerCommand::ParkAfterZero { owner: owner() }.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
    assert_eq!(
        WorkerCommand::Shutdown.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
}
