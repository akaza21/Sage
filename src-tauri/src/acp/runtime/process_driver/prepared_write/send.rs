use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{AdapterBatchJournal, LiveWrite, NonfatalRefusal},
};
use sage_acp_protocol::contract::RequiredText;

use super::{
    DriverError, PreparedProcessWrite, PreparedWriteError, PreparedWriteFailure,
    ProcessTreeControl, StdinTransport,
};

impl<A, J, R, T, C> PreparedProcessWrite<A, J, R, T, C>
where
    A: HarnessAdapter,
    J: AdapterBatchJournal,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
{
    pub(crate) async fn dispatch_once(
        mut self,
        now_millis: i64,
    ) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        if let Err(error) = self.turn.dispatch.begin_send(now_millis).await {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        self.write_prompt(now_millis).await
    }

    pub(crate) async fn retry_send_projection(
        mut self,
        now_millis: i64,
    ) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        if let Err(error) = self.turn.dispatch.retry_projection() {
            return Err(Box::new(PreparedWriteFailure {
                state: self,
                error: error.into(),
            }));
        }
        self.write_prompt(now_millis).await
    }

    async fn write_prompt(
        mut self,
        now_millis: i64,
    ) -> Result<(Self, LiveWrite), Box<PreparedWriteFailure<Self>>> {
        let turn_id = self.turn.dispatch.turn_id().clone();
        let (prompt, access_mode) = match self.delivery.begin_write(&turn_id) {
            Ok(write) => write,
            Err(error) => {
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: error.into(),
                }));
            }
        };
        let client_message_id =
            match RequiredText::new("clientMessageId", turn_id.as_str().to_owned()) {
                Ok(value) => value,
                Err(_) => {
                    return Err(Box::new(PreparedWriteFailure {
                        state: self,
                        error: PreparedWriteError::Intent,
                    }));
                }
            };
        let result = self.turn.driver.dispatch_prompt_with_access_mode(
            prompt,
            client_message_id,
            access_mode,
        );
        let write = match result {
            Ok(progress) if progress.written_frames == 1 && progress.zero_byte_failures == 0 => {
                LiveWrite::Complete
            }
            Ok(progress) if progress.written_frames == 0 && progress.zero_byte_failures == 1 => {
                LiveWrite::ZeroBytes
            }
            Err(DriverError::TeardownRequired(_))
                if self.turn.driver.first_delivery_unknown().is_some() =>
            {
                LiveWrite::PossibleBytes
            }
            Ok(_) => {
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: PreparedWriteError::Intent,
                }));
            }
            Err(refused @ DriverError::Refused(NonfatalRefusal::SignIn)) => {
                let error = self
                    .turn
                    .driver
                    .fail_unwritten_prompt_for_sign_in(now_millis)
                    .await
                    .err()
                    .unwrap_or(refused);
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: PreparedWriteError::Driver(error),
                }));
            }
            Err(error) => {
                return Err(Box::new(PreparedWriteFailure {
                    state: self,
                    error: PreparedWriteError::Driver(error),
                }));
            }
        };
        Ok((self, write))
    }
}
