//! A tool executed by Pi, with no filesystem or shell interpretation in Sage.
use super::{Refusal, files::Request};
use sage_acp_protocol::contract::{RequiredText, SafeText};

pub(crate) struct Extension {
    pub tool_id: String,
    pub label: SafeText<512>,
    pub summary: SafeText<65536>,
}

pub(super) fn prepare(request: Request) -> Result<Extension, Refusal> {
    let name =
        RequiredText::<128>::new("toolName", request.tool).map_err(|_| Refusal::Unsupported)?;
    let display_label = request
        .label
        .map(|label| SafeText::<512>::new("toolLabel", label))
        .transpose()
        .map_err(|_| Refusal::TooLarge)?;
    let label = display_label
        .as_ref()
        .filter(|label| !label.as_str().is_empty())
        .map(|label| format!(" ({})", label.as_str()))
        .unwrap_or_default();
    let args = serde_json::to_string(&request.args).map_err(|_| Refusal::Unsupported)?;
    // Share the credential engine used by managed shell approvals, without
    // treating structured argument strings as shell grammar or path authority.
    let args =
        crate::acp::redaction::text::redact_complete_arguments("permissionSummary", &args, 65_536)
            .map_err(|_| Refusal::TooLarge)?;
    // The existing approval card has a 64 KiB text budget. Refuse rather than
    // truncate arguments: the user must see the complete operation they allow.
    let summary = SafeText::new(
        "permissionSummary",
        format!("Allow extension tool {}{label}?\n{}", name.as_str(), args),
    )
    .map_err(|_| Refusal::TooLarge)?;
    Ok(Extension {
        tool_id: request.tool_call_id,
        label: display_label
            .filter(|label| !label.as_str().is_empty())
            .unwrap_or_else(|| SafeText::new("toolLabel", name.as_str()).expect("bounded name")),
        summary,
    })
}
