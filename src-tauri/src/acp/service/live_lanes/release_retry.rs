use std::time::{Duration, Instant};

use sage_acp_protocol::contract::{SageChatId, SageProjectId};

use super::{AcpService, ChatRuntimeRelease};

const LANE_BUSY_RETRY: Duration = Duration::from_millis(25);

impl AcpService {
    /// Each attempt hands its Tokio worker to the blocking pool.
    pub(crate) async fn release_chat_runtime_until(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        deadline: Instant,
        still_wanted: impl Fn() -> bool,
    ) -> Result<(), ChatRuntimeRelease> {
        loop {
            let outcome = sage_core::blocking::in_place(|| {
                self.release_chat_runtime(project, chat, deadline, &still_wanted)
            });
            match outcome {
                Err(ChatRuntimeRelease::LaneBusy | ChatRuntimeRelease::ControlRunning)
                    if Instant::now() + LANE_BUSY_RETRY < deadline =>
                {
                    tokio::time::sleep(LANE_BUSY_RETRY).await;
                }
                outcome => return outcome,
            }
        }
    }
}
