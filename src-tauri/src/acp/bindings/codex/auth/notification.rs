use serde_json::{Map, Value};

use super::{AUTH_ERROR_BYTES, CodexAuthError, CodexAuthProtocol, NativeLoginId};
use crate::acp::transport::Frame;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexAuthNotification {
    /// `login_id` is absent for an API key login, which has no browser flow.
    LoginCompleted {
        login_id: Option<NativeLoginId>,
        success: bool,
    },
    /// `mode` is `None` once the account signed out.
    AccountUpdated { mode: Option<CodexAuthMode> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexAuthMode {
    Chatgpt,
    ApiKey,
}

impl CodexAuthProtocol {
    pub(crate) fn accept_notification(
        &self,
        frame: &Frame,
    ) -> Result<CodexAuthNotification, CodexAuthError> {
        let value: Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| CodexAuthError::Protocol)?;
        crate::acp::bindings::codex::incoming::validate_shape(&value)
            .map_err(|_| CodexAuthError::Protocol)?;
        let object = value.as_object().ok_or(CodexAuthError::Protocol)?;
        if object.len() != 2 || object.contains_key("id") || object.contains_key("result") {
            return Err(CodexAuthError::Protocol);
        }
        let params = object
            .get("params")
            .and_then(Value::as_object)
            .ok_or(CodexAuthError::Protocol)?;
        match object.get("method").and_then(Value::as_str) {
            Some("account/login/completed") => parse_completed(params),
            Some("account/updated") => parse_updated(params),
            _ => Err(CodexAuthError::Protocol),
        }
    }
}

fn parse_completed(params: &Map<String, Value>) -> Result<CodexAuthNotification, CodexAuthError> {
    if params.len() != 3 || !params.contains_key("error") {
        return Err(CodexAuthError::Protocol);
    }
    let login_id = match params.get("loginId") {
        Some(Value::Null) => None,
        Some(Value::String(id)) => Some(NativeLoginId::parse(id)?),
        _ => return Err(CodexAuthError::Protocol),
    };
    let success = params
        .get("success")
        .and_then(Value::as_bool)
        .ok_or(CodexAuthError::Protocol)?;
    let error_valid = if success {
        matches!(params.get("error"), Some(Value::Null))
    } else {
        params
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| error.len() <= AUTH_ERROR_BYTES && !error.contains('\0'))
    };
    if !error_valid {
        return Err(CodexAuthError::Protocol);
    }
    Ok(CodexAuthNotification::LoginCompleted { login_id, success })
}

fn parse_updated(params: &Map<String, Value>) -> Result<CodexAuthNotification, CodexAuthError> {
    if params.len() != 2 || !params.contains_key("planType") {
        return Err(CodexAuthError::Protocol);
    }
    let mode = match params.get("authMode") {
        Some(Value::String(mode)) if mode == "chatgpt" => Some(CodexAuthMode::Chatgpt),
        Some(Value::String(mode)) if mode == "apikey" => Some(CodexAuthMode::ApiKey),
        Some(Value::Null) => None,
        _ => return Err(CodexAuthError::Protocol),
    };
    let plan_valid = matches!(params.get("planType"), Some(Value::Null))
        || params
            .get("planType")
            .and_then(Value::as_str)
            .is_some_and(|plan| plan.len() <= 512 && !plan.contains('\0'));
    if !plan_valid || (mode.is_none() && !matches!(params.get("planType"), Some(Value::Null))) {
        return Err(CodexAuthError::Protocol);
    }
    Ok(CodexAuthNotification::AccountUpdated { mode })
}
