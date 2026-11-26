//! A session start that ended needing recovery (its harness was too slow to
//! start, and the process tree could not be proven gone in time) is driven to
//! completion here instead of waiting for the chat to be archived or the app
//! to close. Each attempt waits on the tree's own exit and its job emptying,
//! the real signals; once they come, the start is aborted durably, the
//! session is back to created and cold, and the projection that publishes
//! re-enables the composer.

use sage_acp_protocol::contract::{SageChatId, SageProjectId};

use crate::acp::service::{ChatRuntimeRelease, explicit_close_deadline};
use crate::state::AppState;

/// Attempts, each bounded by the explicit-close deadline: a tree still being
/// scanned or torn down usually settles within one or two.
const ATTEMPTS: usize = 6;

/// Drives the chat's retained start recovery; `Err` keeps the last reason
/// it did not settle.
pub(crate) async fn drive(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<(), ChatRuntimeRelease> {
    let mut outcome = Ok(());
    for _ in 0..ATTEMPTS {
        outcome = state
            .acp
            .release_chat_runtime_until(project, chat, explicit_close_deadline(), || true)
            .await;
        match outcome {
            Err(ChatRuntimeRelease::Unsettled | ChatRuntimeRelease::LaneBusy) => {}
            settled => return settled,
        }
    }
    outcome
}

/// Starts [`drive`] once, off the request that reported the recovery.
pub(crate) fn drive_in_background(app: tauri::AppHandle, project: SageProjectId, chat: SageChatId) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let state = app.state::<AppState>();
        let _ = drive(&state, &project, &chat).await;
    });
}
