use super::*;
use crate::acp::{
    runtime::read_text_file::encode_read_text_file_response,
    transport::{P02Egress, P02EgressOutcome, write_p02_response_until},
};

#[allow(dead_code, reason = "P-02 sealed adapter delivery entrypoint")]
impl ReadTextCompletionLease {
    pub(super) fn new(
        state: Arc<ReadState>,
        token: String,
        deadline: Instant,
        completion: ReadTextCompletion,
    ) -> Self {
        Self {
            state,
            token,
            deadline,
            completion: Some(completion),
        }
    }

    /// Encodes while this lease still owns the reservation, releases all P-02
    /// state before I/O, then atomically settles exactly one receipt.
    pub(crate) async fn deliver_response(
        mut self,
        request_id: &ReadTextRequestId,
        egress: &mut impl P02Egress,
    ) -> ReadTextResult<ReadTextDelivery> {
        let cancellation = begin_delivery(&self.state, &self.token)?;
        let frame = match encode_read_text_file_response(
            request_id,
            &self.completion.as_ref().expect("leased completion").content,
        ) {
            Ok(frame) => frame,
            Err(_) => {
                let completion = self.completion.take().expect("leased completion");
                return state_machine::settle_lease(
                    &self.state,
                    &self.token,
                    P02EgressOutcome::ZeroByteFailure,
                    completion,
                );
            }
        };
        let outcome = write_p02_response_until(egress, &frame, self.deadline, &cancellation).await;
        let completion = self.completion.take().expect("leased completion");
        state_machine::settle_lease(&self.state, &self.token, outcome, completion)
    }
}

impl Drop for ReadTextCompletionLease {
    fn drop(&mut self) {
        if self.completion.take().is_some() {
            lease_dropped(&self.state, &self.token);
        }
    }
}
