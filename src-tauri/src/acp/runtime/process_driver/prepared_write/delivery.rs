use crate::acp::{contract::SageTurnId, intent::Prompt, store::SendObservation};

use super::super::{WriteFence, WriteFenceError};

/// One Turn's write fence and the prompt it may still write. The fence admits
/// a write before the first attempt and again after zero-byte evidence; once an
/// observation settles delivery (complete, correlated, possible bytes or a lost
/// process) no write can follow, so the prompt text and its attachment snapshot
/// are released instead of living as long as the active Turn.
pub(super) struct PromptDelivery {
    fence: WriteFence,
    prompt: Option<Prompt>,
}

impl PromptDelivery {
    pub(super) fn new(turn_id: SageTurnId, prompt: Prompt) -> Self {
        Self {
            fence: WriteFence::bound(turn_id),
            prompt: Some(prompt),
        }
    }

    /// Opens the fence for one write and returns the prompt to write.
    pub(super) fn begin_write(&mut self, turn_id: &SageTurnId) -> Result<Prompt, WriteFenceError> {
        self.fence.begin_write(turn_id)?;
        self.prompt.clone().ok_or(WriteFenceError::State)
    }

    pub(super) fn accept_observation(
        &mut self,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), WriteFenceError> {
        self.fence.accept_observation(turn_id, observation)?;
        if self.fence.delivery_settled() {
            self.prompt = None;
        }
        Ok(())
    }

    pub(super) fn fence(&self) -> &WriteFence {
        &self.fence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::attached_context::files::{Resource, Resources};
    use std::sync::{Arc, Weak};

    fn prompt_with_attachment() -> (Prompt, Weak<Vec<Resource>>) {
        let resources = Resources(Arc::new(vec![Resource {
            id: "file-1".into(),
            name: "notes.txt".into(),
            bytes: vec![7; 64 * 1024],
        }]));
        let weak = Arc::downgrade(&resources.0);
        let mut prompt = Prompt::new("prompt", "Explain").unwrap();
        prompt.resources = resources;
        (prompt, weak)
    }

    #[test]
    fn the_prompt_is_kept_while_a_retry_is_possible_and_released_once_delivery_settles() {
        let turn = SageTurnId::parse("turn-active").unwrap();
        for settled in [
            SendObservation::CompleteBytes,
            SendObservation::Correlated,
            SendObservation::PossibleBytes,
            SendObservation::ProcessLost,
        ] {
            let (prompt, attachment) = prompt_with_attachment();
            let mut delivery = PromptDelivery::new(turn.clone(), prompt);
            drop(delivery.begin_write(&turn).unwrap());
            delivery
                .accept_observation(&turn, SendObservation::ZeroBytes)
                .unwrap();
            // Zero bytes admit one more write of the same prompt.
            assert!(attachment.upgrade().is_some());
            let retry = delivery.begin_write(&turn).unwrap();
            assert_eq!(retry.as_str(), "Explain");
            drop(retry);
            delivery.accept_observation(&turn, settled).unwrap();
            assert!(
                attachment.upgrade().is_none(),
                "{settled:?} kept the prompt"
            );
            assert_eq!(
                delivery.begin_write(&turn).err(),
                Some(WriteFenceError::State)
            );
        }
    }
}
