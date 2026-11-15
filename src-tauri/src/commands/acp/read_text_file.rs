//! Typed P-02 desktop acknowledgement. This is not filesystem IPC: it accepts
//! no content or native path and only settles a token emitted by an admitted
//! ACP adapter read service.

use tauri::State;

use crate::{
    acp::{
        ipc::{AcpIpcError, AcpUnavailableReason},
        read_text_file::{ReadTextFlushAck, ReadTextNextAction},
    },
    state::AppState,
};

use super::errors::safe_message;

#[tauri::command]
pub async fn acknowledge_acp_read_text_file(
    state: State<'_, AppState>,
    ack: ReadTextFlushAck,
) -> Result<ReadTextNextAction, AcpIpcError> {
    let context = state
        .lock_active_project(&ack.owner.project_id, ack.owner.project_generation)
        .await
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::SessionUnavailable,
            message: safe_message("The read request is no longer owned by this project."),
        })?;
    if context.project_id() != &ack.owner.project_id {
        return Err(AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::SessionUnavailable,
            message: safe_message("The read request is no longer owned by this project."),
        });
    }
    // This signals the adapter-owned native oneshot. It returns no source or
    // completion lease to Tauri, so this command cannot consume or drop egress.
    state
        .acp
        .read_text_file_service()
        .acknowledge(ack)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::SessionUnavailable,
            message: safe_message("The ACP read acknowledgement was unavailable."),
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ReadTextFlushAck;

    #[test]
    fn acknowledgement_rejects_content_and_requires_owner_token_resource() {
        let exact = json!({
            "owner": {"projectId":"project","projectGeneration":"1","chatId":"chat","sessionId":"session","runtimeEpoch":"2"},
            "resource": {"rootId":"root","relativePath":"file.txt"},
            "expectedEditorRevision":null,
            "token":"p02-1",
            "state": {"kind":"diskAllowed","state":"notOpen","editorRevision":null}
        });
        assert!(serde_json::from_value::<ReadTextFlushAck>(exact.clone()).is_ok());
        let mut content = exact.clone();
        content["content"] = json!("secret");
        assert!(serde_json::from_value::<ReadTextFlushAck>(content).is_err());
        let mut nested = exact.clone();
        nested["resource"]["content"] = json!("secret");
        assert!(serde_json::from_value::<ReadTextFlushAck>(nested).is_err());
        for (field, limit) in [("token", 512), ("rootId", 512), ("relativePath", 16 * 1024)] {
            for size in [limit, limit + 1] {
                let mut candidate = exact.clone();
                if field == "token" {
                    candidate[field] = json!("x".repeat(size));
                } else {
                    candidate["resource"][field] = json!("x".repeat(size));
                }
                assert_eq!(
                    serde_json::from_value::<ReadTextFlushAck>(candidate).is_ok(),
                    size == limit,
                    "{field} {size}"
                );
            }
        }
        let mut foreign = exact;
        foreign["owner"]["runtimeEpoch"] = json!(0);
        assert!(serde_json::from_value::<ReadTextFlushAck>(foreign).is_err());
    }
}
