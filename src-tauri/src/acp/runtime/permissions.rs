#[cfg(test)]
use crate::acp::limits::LIMITS;
use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageServerRequestId, SageTurnId},
    intent::ExternalId,
};

/// A user choice for one exact, durably identified permission request.
/// This remains a runtime-level value so host and adapter routing never need
/// to know which harness originated the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    Ask,
    AllowOnce,
    AllowAlways,
    Deny,
    /// Answers to a harness question request. It resolves the request the
    /// way an allow does; the answers live only until the reply is built.
    Answer(crate::acp::intent::user_input::UserInputAnswers),
}

impl PermissionDecision {
    /// The stable wire label; it never carries answer text.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AllowOnce => "allowOnce",
            Self::AllowAlways => "allowAlways",
            Self::Deny => "deny",
            Self::Answer(_) => "answer",
        }
    }

    /// True when the decision sends the harness a positive reply.
    pub(crate) fn resolves_allowed(&self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways | Self::Answer(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionIntent {
    pub request_id: SageServerRequestId,
    pub project_id: crate::acp::contract::SageProjectId,
    pub chat_id: crate::acp::contract::SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub runtime_epoch: RuntimeEpoch,
    pub binding_digest: String,
    pub external_request_id: ExternalId,
    pub kind: RequiredText<128>,
    /// Version-1 digest of the exact request method/params token identity.
    pub payload_digest: String,
    /// Integrity digest of the bounded normalized payload stored on the Item.
    pub normalized_payload_digest: String,
    pub normalized_payload_json: String,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionDisposition {
    ResponseQueued,
    ResolvedDeclined,
    Duplicate,
    TearDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclineCommit {
    Applied,
    IdenticalDuplicate,
}

#[cfg(test)]
#[derive(Debug, thiserror::Error)]
pub(crate) enum PermissionError {
    #[error("permission request identity is invalid")]
    InvalidIntent,
    #[error("permission decline exceeded its one-second deadline")]
    Deadline,
    #[error("durable decline failed")]
    Durable,
    #[error("decline response could not enter the reserved control lane")]
    ControlLane,
}

#[cfg(test)]
pub(crate) trait PermissionJournal {
    fn commit_decline(
        &mut self,
        intent: &PermissionIntent,
    ) -> Result<DeclineCommit, PermissionError>;
    fn response_queued(&mut self, intent: &PermissionIntent) -> Result<(), PermissionError>;
    fn resolved_declined(&mut self, intent: &PermissionIntent) -> Result<(), PermissionError>;
}

#[cfg(test)]
pub(crate) trait DeclineTransport {
    fn queue_decline(&mut self, intent: &PermissionIntent) -> Result<(), PermissionError>;
}

#[cfg(test)]
pub(crate) fn decline_permission<J: PermissionJournal, T: DeclineTransport>(
    journal: &mut J,
    transport: &mut T,
    intent: &PermissionIntent,
    received_millis: u64,
    now_millis: u64,
) -> Result<PermissionDisposition, PermissionError> {
    validate_intent(intent)?;
    if now_millis.saturating_sub(received_millis)
        > LIMITS.deadlines_ms.permission_decline_commit_and_queue
    {
        return Err(PermissionError::Deadline);
    }
    // Durable denial always precedes any response byte.
    match journal.commit_decline(intent) {
        Ok(DeclineCommit::Applied) => {}
        Ok(DeclineCommit::IdenticalDuplicate) => {
            // The original response may already have crossed the process
            // boundary. Never replay it from a duplicate or restart path.
            return Ok(PermissionDisposition::Duplicate);
        }
        Err(_) => return Ok(PermissionDisposition::TearDown),
    }
    if transport.queue_decline(intent).is_err() {
        return Ok(PermissionDisposition::TearDown);
    }
    if journal.response_queued(intent).is_err() {
        return Ok(PermissionDisposition::TearDown);
    }
    Ok(PermissionDisposition::ResponseQueued)
}

/// Advances a decline only after the transport confirms that the response was
/// written. Restart recovery never calls this function and never replays the
/// response.
#[cfg(test)]
pub(crate) fn decline_written<J: PermissionJournal>(
    journal: &mut J,
    intent: &PermissionIntent,
) -> Result<PermissionDisposition, PermissionError> {
    validate_intent(intent)?;
    if journal.resolved_declined(intent).is_err() {
        return Ok(PermissionDisposition::TearDown);
    }
    Ok(PermissionDisposition::ResolvedDeclined)
}

#[cfg(test)]
fn validate_intent(intent: &PermissionIntent) -> Result<(), PermissionError> {
    if intent.runtime_epoch == RuntimeEpoch::ZERO
        || !valid_digest(&intent.binding_digest)
        || !valid_digest(&intent.payload_digest)
        || !valid_digest(&intent.normalized_payload_digest)
        || intent.normalized_payload_json.len() > LIMITS.wire.permission_payload_bytes as usize
        || serde_json::from_str::<serde_json::Value>(&intent.normalized_payload_json).is_err()
    {
        return Err(PermissionError::InvalidIntent);
    }
    Ok(())
}

#[cfg(test)]
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<&'static str>>>);
    impl Recorder {
        fn events(&self) -> Vec<&'static str> {
            self.0.lock().unwrap().clone()
        }
    }
    impl PermissionJournal for Recorder {
        fn commit_decline(
            &mut self,
            _: &PermissionIntent,
        ) -> Result<DeclineCommit, PermissionError> {
            self.0.lock().unwrap().push("commit");
            Ok(DeclineCommit::Applied)
        }
        fn response_queued(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            self.0.lock().unwrap().push("queued");
            Ok(())
        }
        fn resolved_declined(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            self.0.lock().unwrap().push("resolved");
            Ok(())
        }
    }
    impl DeclineTransport for Recorder {
        fn queue_decline(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            self.0.lock().unwrap().push("write");
            Ok(())
        }
    }

    fn intent() -> PermissionIntent {
        PermissionIntent {
            request_id: SageServerRequestId::parse("request-one").unwrap(),
            project_id: crate::acp::contract::SageProjectId::parse("project-one").unwrap(),
            chat_id: crate::acp::contract::SageChatId::parse("chat-one").unwrap(),
            session_id: SageAgentSessionId::parse("session-one").unwrap(),
            turn_id: SageTurnId::parse("turn-one").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            external_request_id: ExternalId::new("externalRequestId", "external-one").unwrap(),
            kind: RequiredText::new("kind", "permission").unwrap(),
            payload_digest: "b".repeat(64),
            normalized_payload_digest: "c".repeat(64),
            normalized_payload_json: "{}".into(),
        }
    }

    #[test]
    fn denial_commits_before_the_control_response() {
        let shared = Recorder::default();
        let mut journal = shared.clone();
        let mut transport = shared.clone();
        let intent = intent();
        assert_eq!(
            decline_permission(&mut journal, &mut transport, &intent, 0, 1).unwrap(),
            PermissionDisposition::ResponseQueued
        );
        assert_eq!(shared.events(), ["commit", "write", "queued"]);
        assert_eq!(
            decline_written(&mut journal, &intent).unwrap(),
            PermissionDisposition::ResolvedDeclined
        );
        assert_eq!(shared.events(), ["commit", "write", "queued", "resolved"]);
    }

    #[test]
    fn oversized_or_late_request_fails_closed() {
        let mut oversized = intent();
        oversized.normalized_payload_json = format!("\"{}\"", "x".repeat(65_536));
        let mut journal = Recorder::default();
        let mut transport = Recorder::default();
        assert!(matches!(
            decline_permission(&mut journal, &mut transport, &oversized, 0, 0),
            Err(PermissionError::InvalidIntent)
        ));
        assert!(matches!(
            decline_permission(&mut journal, &mut transport, &intent(), 0, 1_001),
            Err(PermissionError::Deadline)
        ));
    }

    struct DuplicateJournal;
    impl PermissionJournal for DuplicateJournal {
        fn commit_decline(
            &mut self,
            _: &PermissionIntent,
        ) -> Result<DeclineCommit, PermissionError> {
            Ok(DeclineCommit::IdenticalDuplicate)
        }
        fn response_queued(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            panic!("duplicate must not advance or replay")
        }
        fn resolved_declined(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            panic!("duplicate must not advance or replay")
        }
    }

    struct PanicTransport;
    impl DeclineTransport for PanicTransport {
        fn queue_decline(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            panic!("duplicate must not replay a response")
        }
    }

    #[test]
    fn identical_duplicate_is_a_no_op_and_never_replays_transport() {
        assert_eq!(
            decline_permission(&mut DuplicateJournal, &mut PanicTransport, &intent(), 0, 1)
                .unwrap(),
            PermissionDisposition::Duplicate
        );
    }

    struct FailingJournal;
    impl PermissionJournal for FailingJournal {
        fn commit_decline(
            &mut self,
            _: &PermissionIntent,
        ) -> Result<DeclineCommit, PermissionError> {
            Err(PermissionError::Durable)
        }
        fn response_queued(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            Err(PermissionError::Durable)
        }
        fn resolved_declined(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            Err(PermissionError::Durable)
        }
    }

    struct FailingTransport;
    impl DeclineTransport for FailingTransport {
        fn queue_decline(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
            Err(PermissionError::ControlLane)
        }
    }

    #[test]
    fn conflicting_or_unwritable_declines_tear_down_fail_closed() {
        assert_eq!(
            decline_permission(&mut FailingJournal, &mut PanicTransport, &intent(), 0, 1).unwrap(),
            PermissionDisposition::TearDown
        );
        assert_eq!(
            decline_permission(
                &mut Recorder::default(),
                &mut FailingTransport,
                &intent(),
                0,
                1
            )
            .unwrap(),
            PermissionDisposition::TearDown
        );
    }
}
