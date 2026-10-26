use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScopeWire {
    project_id: String,
    project_generation: String,
    chat_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HandleWire {
    session_id: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateRequest {
    scope: ScopeWire,
    columns: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WriteRequest {
    scope: ScopeWire,
    handle: HandleWire,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResizeRequest {
    scope: ScopeWire,
    handle: HandleWire,
    columns: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReadRequest {
    scope: ScopeWire,
    handle: HandleWire,
    cursor: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CloseRequest {
    scope: ScopeWire,
    handle: HandleWire,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleResponse {
    session_id: String,
    revision: String,
    shell_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    bytes_base64: String,
    next_cursor: String,
    truncations: String,
    truncated: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TerminalIpcError {
    code: &'static str,
}

impl TerminalIpcError {
    fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl From<TerminalError> for TerminalIpcError {
    fn from(error: TerminalError) -> Self {
        Self::new(match error {
            TerminalError::Capacity => "capacity",
            TerminalError::InvalidScope => "invalid_scope",
            TerminalError::InvalidDimensions => "invalid_dimensions",
            TerminalError::InvalidInput => "invalid_input",
            TerminalError::UnknownSession => "unknown_session",
            TerminalError::Ended => "ended",
            TerminalError::Busy => "busy",
            TerminalError::Backend => "backend",
        })
    }
}

fn available() -> Result<(), TerminalIpcError> {
    if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
        Ok(())
    } else {
        Err(TerminalIpcError::new("unavailable"))
    }
}

fn decimal(value: &str) -> Result<u64, TerminalIpcError> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(TerminalIpcError::new("invalid_request"));
    }
    value
        .parse()
        .map_err(|_| TerminalIpcError::new("invalid_request"))
}

impl ScopeWire {
    fn parse(self) -> Result<TerminalScope, TerminalIpcError> {
        Ok(TerminalScope {
            project: SageProjectId::parse(self.project_id)
                .map_err(|_| TerminalIpcError::new("invalid_request"))?,
            generation: ProjectGeneration::new(decimal(&self.project_generation)?)
                .map_err(|_| TerminalIpcError::new("invalid_request"))?,
            chat: SageChatId::parse(self.chat_id)
                .map_err(|_| TerminalIpcError::new("invalid_request"))?,
        })
    }
}

impl HandleWire {
    fn parse(self) -> Result<TerminalHandle, TerminalIpcError> {
        if self.session_id.len() != 36 {
            return Err(TerminalIpcError::new("invalid_request"));
        }
        let id = uuid::Uuid::parse_str(&self.session_id)
            .map_err(|_| TerminalIpcError::new("invalid_request"))?;
        if id.to_string() != self.session_id {
            return Err(TerminalIpcError::new("invalid_request"));
        }
        let revision = decimal(&self.revision)?;
        if revision == 0 {
            return Err(TerminalIpcError::new("invalid_request"));
        }
        Ok(TerminalHandle {
            id: TerminalSessionId(id),
            revision: TerminalRevision(revision),
        })
    }
}

#[tauri::command]
pub async fn terminal_create(
    state: State<'_, crate::state::AppState>,
    request: CreateRequest,
) -> Result<HandleResponse, TerminalIpcError> {
    available()?;
    let scope = request.scope.parse()?;
    let handle = state
        .terminals
        .create(
            &TerminalAuthority(&state),
            &scope,
            request.columns,
            request.rows,
        )
        .await?;
    let shell_path = state.terminals.shell_path(handle)?;
    Ok(HandleResponse {
        session_id: handle.id.0.to_string(),
        revision: handle.revision.0.to_string(),
        shell_path,
    })
}

#[tauri::command]
pub async fn terminal_write(
    state: State<'_, crate::state::AppState>,
    request: WriteRequest,
) -> Result<(), TerminalIpcError> {
    available()?;
    let scope = request.scope.parse()?;
    let handle = request.handle.parse()?;
    state
        .terminals
        .write_input(&TerminalAuthority(&state), &scope, handle, &request.text)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, crate::state::AppState>,
    request: ResizeRequest,
) -> Result<(), TerminalIpcError> {
    available()?;
    let scope = request.scope.parse()?;
    let handle = request.handle.parse()?;
    state
        .terminals
        .resize(
            &TerminalAuthority(&state),
            &scope,
            handle,
            request.columns,
            request.rows,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn terminal_read(
    state: State<'_, crate::state::AppState>,
    request: ReadRequest,
) -> Result<ReadResponse, TerminalIpcError> {
    available()?;
    let scope = request.scope.parse()?;
    let handle = request.handle.parse()?;
    let cursor = decimal(&request.cursor)?;
    let page = state
        .terminals
        .read_output(&TerminalAuthority(&state), &scope, handle, cursor)
        .await?;
    Ok(ReadResponse {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(page.bytes),
        next_cursor: page.next_cursor.to_string(),
        truncations: page.truncations.to_string(),
        truncated: page.truncated,
    })
}

#[tauri::command]
pub async fn terminal_close(
    state: State<'_, crate::state::AppState>,
    request: CloseRequest,
) -> Result<(), TerminalIpcError> {
    available()?;
    let scope = request.scope.parse()?;
    let handle = request.handle.parse()?;
    state
        .terminals
        .close(&TerminalAuthority(&state), &scope, handle)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unreviewed_process_control_fields() {
        let base = serde_json::json!({"scope": {"projectId": "project-1", "projectGeneration": "0", "chatId": "chat-1"}, "columns": 80, "rows": 24});
        for field in ["shell", "argv", "cwd", "environment", "pid"] {
            let mut value = base.clone();
            value[field] = serde_json::json!("injected");
            assert!(serde_json::from_value::<CreateRequest>(value).is_err());
        }
        let mut nested = base;
        nested["scope"]["shell"] = serde_json::json!("injected");
        assert!(serde_json::from_value::<CreateRequest>(nested).is_err());
    }

    #[test]
    fn rejects_noncanonical_handle_and_cursor() {
        assert!(decimal("01").is_err());
        assert!(decimal("18446744073709551616").is_err());
        assert!(
            HandleWire {
                session_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
                revision: "1".into()
            }
            .parse()
            .is_err()
        );
    }

    #[test]
    fn full_output_page_stays_within_wire_reply_limit() {
        let response = ReadResponse {
            bytes_base64: base64::engine::general_purpose::STANDARD
                .encode(vec![0xff; OUTPUT_PAGE_BYTES]),
            next_cursor: u64::MAX.to_string(),
            truncations: u64::MAX.to_string(),
            truncated: true,
        };
        assert!(serde_json::to_vec(&response).unwrap().len() <= 65_536);
    }
}
