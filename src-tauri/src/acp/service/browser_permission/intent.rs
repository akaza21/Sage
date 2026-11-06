//! The card's journal intent and request kind (ADR-0009, ADR-0009b).

use super::BrowserPermissionOwner;
use crate::acp::{
    browser::permission::{BrowserPermissionChoice, BrowserPermissionPrompt},
    contract::{RequiredText, SafeText, SageServerRequestId},
    intent::{ExternalId, NormalizedPayload, ToolCategory},
    runtime::permissions::PermissionIntent,
};
use sha2::{Digest, Sha256};

pub(super) fn local_intent(
    owner: &BrowserPermissionOwner,
    prompt: &BrowserPermissionPrompt,
    request_id: &SageServerRequestId,
) -> Option<PermissionIntent> {
    let elevated = prompt.operation == "cdp";
    let summary = if elevated {
        format!("Browser developer mode on {}", prompt.origin)
    } else {
        format!("Browser {} on {}", prompt.operation, prompt.origin)
    };
    let summary = SafeText::new("summary", summary).ok()?;
    let normalized_payload_json = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        category: ToolCategory::Mcp,
        summary,
        offered_option_kinds: None,
        reason: None,
    })
    .ok()?;
    let normalized_payload_digest = hex_digest(normalized_payload_json.as_bytes());
    Some(PermissionIntent {
        request_id: request_id.clone(),
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: owner.binding_digest.clone(),
        external_request_id: ExternalId::new("externalRequestId", request_id.as_str()).ok()?,
        kind: RequiredText::new(
            "kind",
            if elevated {
                DEVELOPER_REQUEST_KIND
            } else {
                "browserPermission"
            },
        )
        .ok()?,
        payload_digest: hex_digest(
            format!(
                "{}:{}:{}",
                request_id.as_str(),
                prompt.scope.tab_id,
                prompt.origin
            )
            .as_bytes(),
        ),
        normalized_payload_digest,
        normalized_payload_json,
    })
}

/// The request kind of a developer-mode card (ADR-0009b), kept apart from
/// `browserPermission` so no other decision can answer it.
const DEVELOPER_REQUEST_KIND: &str = "browserDeveloperPermission";

/// Deny answers any card; developer mode answers only an elevated one, and
/// the ordinary choices only an ordinary one.
pub(super) fn choice_fits(elevated: bool, choice: BrowserPermissionChoice) -> bool {
    match choice {
        BrowserPermissionChoice::Deny => true,
        BrowserPermissionChoice::AllowDeveloperMode => elevated,
        BrowserPermissionChoice::AllowOnce | BrowserPermissionChoice::AllowForOriginSession => {
            !elevated
        }
    }
}

pub(crate) fn is_browser_request_kind(kind: &str) -> bool {
    matches!(kind, "browserPermission" | DEVELOPER_REQUEST_KIND)
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
