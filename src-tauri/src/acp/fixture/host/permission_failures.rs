use serde_json::{Value, json};

use crate::{
    acp::{
        contract::{
            RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
            SageServerRequestId, SageTurnId,
        },
        fixture::generated_cases::FrozenCase,
        intent::ExternalId,
    },
    acp_protocol::permissions::{
        DeclineCommit, DeclineTransport, PermissionDisposition, PermissionError, PermissionIntent,
        PermissionJournal, decline_permission,
    },
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "permission-decline-persist-failure",
    "permission-decline-queue-failure",
    "permission-decline-encoding-failure",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Failure {
    Persist,
    Queue,
    Encode,
}

struct Journal {
    failure: Failure,
    decline_durable: bool,
    response_queued: bool,
}

impl PermissionJournal for Journal {
    fn commit_decline(
        &mut self,
        _intent: &PermissionIntent,
    ) -> Result<DeclineCommit, PermissionError> {
        if self.failure == Failure::Persist {
            return Err(PermissionError::Durable);
        }
        self.decline_durable = true;
        Ok(DeclineCommit::Applied)
    }

    fn response_queued(&mut self, _intent: &PermissionIntent) -> Result<(), PermissionError> {
        self.response_queued = true;
        Ok(())
    }

    fn resolved_declined(&mut self, _intent: &PermissionIntent) -> Result<(), PermissionError> {
        panic!("a failed decline must not become resolved")
    }
}

struct Transport {
    failure: Failure,
}

impl DeclineTransport for Transport {
    fn queue_decline(&mut self, _intent: &PermissionIntent) -> Result<(), PermissionError> {
        match self.failure {
            Failure::Queue | Failure::Encode => Err(PermissionError::ControlLane),
            Failure::Persist => panic!("a failed durable decline must not reach transport"),
        }
    }
}

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    assert_eq!(steps[0]["op"], "receivePermissionRequest");
    let failure = match case.id {
        "permission-decline-persist-failure" => {
            assert_eq!(steps[1]["op"], "injectSqliteFailure");
            assert_eq!(steps[1]["at"], "declineCommit");
            Failure::Persist
        }
        "permission-decline-queue-failure" => {
            assert_eq!(steps[1]["op"], "commitDecline");
            assert_eq!(steps[2]["op"], "injectQueueFailure");
            assert_eq!(steps[2]["at"], "declineResponse");
            Failure::Queue
        }
        "permission-decline-encoding-failure" => {
            assert_eq!(steps[1]["op"], "commitDecline");
            assert_eq!(steps[2]["op"], "injectEncodingFailure");
            assert_eq!(steps[2]["at"], "declineResponse");
            Failure::Encode
        }
        id => panic!("unexpected permission fixture {id}"),
    };
    let mut journal = Journal {
        failure,
        decline_durable: false,
        response_queued: false,
    };
    let mut transport = Transport { failure };
    let disposition = decline_permission(&mut journal, &mut transport, &intent(), 0, 1).unwrap();
    assert_eq!(disposition, PermissionDisposition::TearDown);

    match failure {
        Failure::Persist => json!({
            "responseQueued": journal.response_queued,
            "permissionGranted": false,
            "session": "failed",
        }),
        Failure::Queue => json!({
            "declineDurable": journal.decline_durable,
            "permissionGranted": false,
            "session": "failed",
            "automaticAllowFallback": false,
        }),
        Failure::Encode => json!({
            "declineDurable": journal.decline_durable,
            "responseQueued": journal.response_queued,
            "permissionGranted": false,
            "session": "failed",
            "teardownRequired": true,
        }),
    }
}

fn intent() -> PermissionIntent {
    PermissionIntent {
        request_id: SageServerRequestId::parse("fixture-request").unwrap(),
        project_id: SageProjectId::parse("fixture-project").unwrap(),
        chat_id: SageChatId::parse("fixture-chat").unwrap(),
        session_id: SageAgentSessionId::parse("fixture-session").unwrap(),
        turn_id: SageTurnId::parse("fixture-turn").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        binding_digest: "a".repeat(64),
        external_request_id: ExternalId::new("externalRequestId", "fixture-request").unwrap(),
        kind: RequiredText::new("kind", "permission").unwrap(),
        payload_digest: "b".repeat(64),
        normalized_payload_digest: "c".repeat(64),
        normalized_payload_json: "{}".into(),
    }
}
