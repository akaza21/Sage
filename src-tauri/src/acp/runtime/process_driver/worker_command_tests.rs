use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    limits::LIMITS,
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
fn worker_commands_use_the_frozen_operation_deadlines() {
    assert_eq!(
        WorkerCommand::WaitZero { owner: owner() }.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
    assert_eq!(
        WorkerCommand::Shutdown.deadline_millis(),
        LIMITS.deadlines_ms.explicit_close
    );
}
