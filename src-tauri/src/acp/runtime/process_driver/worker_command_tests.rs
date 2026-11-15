use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::acp::runtime::{ConfigCompletionSink, LiveRuntimePort};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigRevision},
};
use sage_core::limits::LIMITS;

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

#[test]
fn a_prompt_with_the_largest_admitted_images_queues_while_harness_output_fills_the_lane() {
    use crate::acp::{
        attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
        transcript_quote::Prompt,
    };
    use sage_acp_protocol::{
        contract::{ClientIntentId, ProjectGeneration, ProjectionRevision},
        intent::ImageMediaType,
    };
    use sage_core::limits::{IMAGE_PREVIEW_BYTES, PROMPT_IMAGE_TOTAL_BYTES};
    let mailbox = DriverMailbox::new().unwrap();
    // Harness output, in the reader's largest chunks, takes the data lane.
    let mut chunks = 0;
    while mailbox.try_push_chunk(vec![0; 8 * 1024]).is_ok() {
        chunks += 1;
    }
    assert!(chunks > 0);
    let owner = owner();
    let mut prompt = Prompt::new("prompt", "describe these").unwrap();
    let image = |bytes| PromptImage {
        media_type: ImageMediaType::Png,
        bytes: vec![0; bytes],
        preview: ImagePreview::Thumbnail(ImageMediaType::Png, vec![0; IMAGE_PREVIEW_BYTES]),
    };
    prompt.images = PromptImages(Arc::new(vec![
        image(PROMPT_IMAGE_TOTAL_BYTES / 2),
        image(PROMPT_IMAGE_TOTAL_BYTES / 2),
    ]));
    let intent = HostPromptIntent {
        command: crate::acp::store::PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("intent-images").unwrap(),
            prompt,
            now_millis: sage_core::time::now_millis(),
        },
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    };
    assert!(
        mailbox
            .submit_worker(WorkerCommand::PreparePrompt(intent))
            .is_ok()
    );
}
