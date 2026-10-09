use crate::{acp::bindings::characterisation::Characterisation, settings_store::SessionAccessMode};

use super::request::RequestError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnPolicySurface {
    Unsupported,
    Supported0146,
}

impl TurnPolicySurface {
    pub(super) fn from_identity(observed: Option<Characterisation>, runtime_version: &str) -> Self {
        if (observed == Some(Characterisation::Characterised) || observed.is_none())
            && runtime_version == "0.146.0"
        {
            Self::Supported0146
        } else {
            Self::Unsupported
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TurnRequestPolicy {
    pub(super) approval: &'static str,
    pub(super) sandbox: &'static str,
    pub(super) network_access: bool,
}

const READ_ONLY: TurnRequestPolicy = TurnRequestPolicy {
    approval: "never",
    sandbox: "readOnly",
    network_access: false,
};
const ASK: TurnRequestPolicy = TurnRequestPolicy {
    approval: "on-request",
    sandbox: "readOnly",
    network_access: false,
};
const WORKSPACE_WRITE: TurnRequestPolicy = TurnRequestPolicy {
    approval: "never",
    sandbox: "workspaceWrite",
    network_access: false,
};

pub(super) fn turn_policy(
    surface: TurnPolicySurface,
    mode: SessionAccessMode,
) -> Result<TurnRequestPolicy, RequestError> {
    let policy = match (surface, mode) {
        (_, SessionAccessMode::ReadOnly) => READ_ONLY,
        (TurnPolicySurface::Supported0146, SessionAccessMode::Ask) => ASK,
        (TurnPolicySurface::Supported0146, SessionAccessMode::WorkspaceWrite) => WORKSPACE_WRITE,
        _ => return Err(RequestError::UnsupportedAccessMode),
    };
    Ok(policy)
}

pub(super) fn validate_turn_policy(
    surface: TurnPolicySurface,
    mode: SessionAccessMode,
    candidate: TurnRequestPolicy,
) -> Result<(), RequestError> {
    if turn_policy(surface, mode)? == candidate {
        Ok(())
    } else {
        Err(RequestError::UnsupportedAccessMode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{contract::RequiredText, intent::Prompt};
    use serde_json::Value;

    #[test]
    fn observed_0146_ask_turn_uses_read_only_sandbox() {
        let request = super::super::request::OutgoingRequest::turn_start_with_access_mode(
            2,
            &RequiredText::new("thread", "thread-1").unwrap(),
            &Prompt::new("prompt", "inspect").unwrap(),
            &RequiredText::new("client", "client-ask").unwrap(),
            SessionAccessMode::Ask,
            TurnPolicySurface::Supported0146,
            None,
        )
        .unwrap();
        let wire: Value = serde_json::from_slice(&request.bytes).unwrap();
        assert_eq!(wire["params"]["approvalPolicy"], "on-request");
        assert_eq!(wire["params"]["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(wire["params"]["sandboxPolicy"]["networkAccess"], false);
        assert!(wire["params"].get("collaborationMode").is_none());
    }

    #[test]
    fn observed_0146_rejects_mixed_approval_and_write_sandbox() {
        assert_eq!(
            validate_turn_policy(
                TurnPolicySurface::Supported0146,
                SessionAccessMode::Ask,
                TurnRequestPolicy {
                    approval: "on-request",
                    sandbox: "workspaceWrite",
                    network_access: false,
                },
            ),
            Err(RequestError::UnsupportedAccessMode)
        );
    }
}
