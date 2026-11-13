//! Releasing a Chat's runtime while other holders come and go: a lane an
//! activation or another release holds is asked again until the deadline.

use std::time::{Duration, Instant};

use crate::acp::contract::{SageChatId, SageProjectId};

use super::{AcpService, ChatRuntimeRelease};

/// How long a release waits before asking again for a lane another holder
/// (an activation, or another release) has claimed.
const LANE_BUSY_RETRY: Duration = Duration::from_millis(25);

impl AcpService {
    /// Parks the Chat's live runtime as [`AcpService::release_chat_runtime`]
    /// does. One `deadline` bounds every attempt and the waits between
    /// them; a lane another holder has claimed is asked again until then,
    /// while a park that did not settle is answered at once. Each attempt
    /// hands its Tokio worker to the blocking pool.
    pub(crate) async fn release_chat_runtime_until(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        deadline: Instant,
        still_wanted: impl Fn() -> bool,
    ) -> Result<(), ChatRuntimeRelease> {
        loop {
            let outcome = crate::blocking::in_place(|| {
                self.release_chat_runtime(project, chat, deadline, &still_wanted)
            });
            match outcome {
                Err(ChatRuntimeRelease::LaneBusy)
                    if Instant::now() + LANE_BUSY_RETRY < deadline =>
                {
                    tokio::time::sleep(LANE_BUSY_RETRY).await;
                }
                outcome => return outcome,
            }
        }
    }
}
