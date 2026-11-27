use std::time::{Duration, Instant};

use sage_acp_protocol::contract::{SageChatId, SageProjectId};

use super::{AcpService, ChatRuntimeRelease};

const LANE_BUSY_RETRY: Duration = Duration::from_millis(25);

impl AcpService {
    /// Whether a lane still retains recovery for the Chat, such as a start
    /// whose tree was not yet proven gone.
    pub(crate) fn chat_holds_recovery(&self, project: &SageProjectId, chat: &SageChatId) -> bool {
        self.lanes.iter().any(|lane| {
            lane.recovery_owner()
                .is_some_and(|owner| &owner.project_id == project && &owner.chat_id == chat)
        })
    }

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
