use std::path::Path;

#[cfg(windows)]
use crate::workspace::atomic_write_windows::atomic_write;

use serde_json::{Value, json};

use crate::acp::bindings::adapter::{
    AdapterBatch, AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressToken,
};
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload};

use super::super::{PermissionReply, PermissionState, StandardAcpRuntime, outbound};
use super::created_file::target_absent;
use super::path::path_inside;

const MAX_CLIENT_WRITE_BYTES: usize = 256 * 1024;

impl StandardAcpRuntime {
    pub(super) fn ingest_client_fs_request(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let (_, _, wire_id) = super::super::permission_identity(value.get("id"))?;
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let result = if method == "fs/write_text_file" {
            self.write_client_file(&value)
        } else {
            Err(if method == "fs/read_text_file" {
                -32000
            } else {
                -32601
            })
        };
        let events = if method == "fs/write_text_file" && result.is_err() {
            let label = self.profile.agent_label;
            let reason = if result == Err(-32602) {
                format!("{label} client file write refused: malformed or oversized request.")
            } else {
                format!(
                    "{label} client file write refused: no current single-use approval for the sole pending tool and target, or filesystem safety check failed."
                )
            };
            vec![AdapterEvent::Error(NormalizedPayload::Error {
                kind: None,
                code: SafeText::new("errorCode", "clientFileWriteRefused")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                message: SafeText::new("errorMessage", &reason)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                retryable: false,
            })]
        } else {
            Vec::new()
        };
        let response = match result {
            Ok(()) => json!({"jsonrpc":"2.0","id":wire_id,"result":{}}),
            Err(code) => json!({
                "jsonrpc":"2.0","id":wire_id,
                "error":{"code":code,"message":if code == -32601 { "Method not found" } else { "Request refused" }}
            }),
        };
        AdapterBatch::new(
            events,
            vec![AdapterEgress::new(
                outbound(response)?,
                EgressToken::client_fs_response(),
            )],
        )
    }

    fn write_client_file(&mut self, value: &Value) -> Result<(), i32> {
        let params = value
            .get("params")
            .and_then(Value::as_object)
            .ok_or(-32602)?;
        if params.len() != 3
            || !params.contains_key("sessionId")
            || !params.contains_key("path")
            || !params.contains_key("content")
        {
            return Err(-32602);
        }
        let session = params["sessionId"].as_str().ok_or(-32602)?;
        let path = params["path"].as_str().ok_or(-32602)?;
        let content = params["content"].as_str().ok_or(-32602)?;
        if content.len() > MAX_CLIENT_WRITE_BYTES {
            return Err(-32602);
        }
        let active = self.active.as_ref().ok_or(-32000)?;
        let turn = active.turn.as_ref().ok_or(-32000)?;
        if active.external_id.as_str() != session
            || turn.access_mode == sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly
            || self.close_pending
            || self.safety_alerted
        {
            return Err(-32000);
        }
        let permission = self.permissions.values_mut().find(|record| {
            matches!(
                record.reply,
                PermissionReply::AllowOnce | PermissionReply::AllowAlways
            ) && record.state == PermissionState::Written
                && record.turn_request_id == turn.request_id
                && !record.client_write_used
                && record.approved_path.as_deref() == Some(Path::new(path))
        });
        let Some(permission) = permission else {
            if std::env::var_os("SAGE_OPENCODE_FS_DIAGNOSTICS").is_some() {
                tracing::warn!(
                    reason = "noMatchingWrittenApproval",
                    approvals = self
                        .permissions
                        .values()
                        .filter(|record| matches!(
                            record.reply,
                            PermissionReply::AllowOnce | PermissionReply::AllowAlways
                        ) && record.state == PermissionState::Written)
                        .count(),
                    approved_paths = self
                        .permissions
                        .values()
                        .filter(|record| record.approved_path.is_some())
                        .count(),
                    "OpenCode client file write refused"
                );
            }
            return Err(-32000);
        };
        if !active.translator.is_sole_pending_tool(&permission.tool_id) {
            if std::env::var_os("SAGE_OPENCODE_FS_DIAGNOSTICS").is_some() {
                tracing::warn!(
                    reason = "notSolePendingTool",
                    "OpenCode client file write refused"
                );
            }
            return Err(-32000);
        }
        if !path_inside(&self.workspace_root, path) {
            if std::env::var_os("SAGE_OPENCODE_FS_DIAGNOSTICS").is_some() {
                tracing::warn!(
                    reason = "pathAdmission",
                    "OpenCode client file write refused"
                );
            }
            return Err(-32000);
        }
        let created_file_written = permission.created_file_content.as_deref() == Some(content)
            && target_absent(Path::new(path));
        atomic_write(
            &self.workspace_root,
            self.workspace_identity,
            Path::new(path),
            content.as_bytes(),
        )
        .map_err(|error| {
            if std::env::var_os("SAGE_OPENCODE_FS_DIAGNOSTICS").is_some() {
                tracing::warn!(reason = "atomicWrite", error_kind = ?error.kind(), "OpenCode client file write refused");
            }
            -32000
        })?;
        permission.client_write_used = true;
        permission.created_file_written = created_file_written;
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_write(
    root: &Path,
    _identity: [u8; 24],
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::{fs::OpenOptions, io::Write};
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let temp = parent.join(format!(".sage-acp-{}.tmp", uuid::Uuid::new_v4()));
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    let result = if path_inside(root, path.to_str().ok_or(std::io::ErrorKind::InvalidInput)?) {
        std::fs::rename(&temp, path)
    } else {
        Err(std::io::ErrorKind::PermissionDenied.into())
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
