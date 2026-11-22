//! A tool executed by Pi, with no filesystem or shell interpretation in Sage.
use super::{Refusal, files::Request};
use crate::acp::{bindings::command_summary::escape_hidden_format, redaction::text};
use sage_acp_protocol::contract::{RequiredText, SafeText};
use std::borrow::Cow;

pub(crate) struct Extension {
    pub tool_id: String,
    pub label: SafeText<512>,
    pub summary: SafeText<65536>,
}

pub(super) fn prepare(request: Request) -> Result<Extension, Refusal> {
    let name =
        RequiredText::<128>::new("toolName", request.tool).map_err(|_| Refusal::Unsupported)?;
    let name = display_text("toolName", name.as_str(), 512)?;
    let display_label = request
        .label
        .map(|label| {
            SafeText::<512>::new("toolLabel", display_text("toolLabel", &label, 512)?)
                .map_err(|_| Refusal::TooLarge)
        })
        .transpose()?;
    let label = display_label
        .as_ref()
        .filter(|label| !label.as_str().is_empty())
        .map(|label| format!(" ({})", label.as_str()))
        .unwrap_or_default();
    let args = serde_json::to_string(&request.args).map_err(|_| Refusal::Unsupported)?;
    // Share the credential engine used by managed shell approvals, without
    // treating structured argument strings as shell grammar or path authority.
    let args = display_text("permissionSummary", &args, 65_536)?;
    // The existing approval card has a 64 KiB text budget. Refuse rather than
    // truncate arguments: the user must see the complete operation they allow.
    let summary = SafeText::new(
        "permissionSummary",
        format!("Allow extension tool {name}{label}?\n{args}"),
    )
    .map_err(|_| Refusal::TooLarge)?;
    Ok(Extension {
        tool_id: request.tool_call_id,
        label: display_label
            .filter(|label| !label.as_str().is_empty())
            .unwrap_or_else(|| SafeText::new("toolLabel", name).expect("bounded name")),
        summary,
    })
}

fn display_text(field: &'static str, input: &str, maximum: usize) -> Result<String, Refusal> {
    let redacted =
        text::redact_complete_arguments(field, input, maximum).map_err(|_| Refusal::TooLarge)?;
    match escape_hidden_format(&redacted, maximum).map_err(|_| Refusal::TooLarge)? {
        Cow::Borrowed(_) => Ok(redacted),
        Cow::Owned(shown) => Ok(shown),
    }
}
