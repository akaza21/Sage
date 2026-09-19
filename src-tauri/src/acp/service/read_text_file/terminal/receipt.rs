//! Sanitized P-02 receipt construction and owner keys.

use super::*;

pub(crate) fn receipt(
    resource: Option<&ResourceRef>,
    range: ReadTextRange,
    source: &str,
    outcome: &str,
    bytes: usize,
) -> NormalizedPayload {
    let input = format!(
        "resource={} line={} limit={}",
        resource.map_or("unavailable".into(), |value| format!(
            "{}:{}",
            value.root_id, value.relative_path
        )),
        range
            .line
            .map_or("default".into(), |value| value.to_string()),
        range.limit.map_or("eof".into(), |value| value.to_string())
    );
    let output = format!("source={source} outcome={outcome} bytes={bytes}");
    NormalizedPayload::ToolCall {
        category: ToolCategory::File,
        label: SafeText::new("toolLabel", READ_LABEL).expect("fixed label"),
        input_summary: Some(SafeText::new("toolInput", input).expect("bounded canonical identity")),
        output_summary: Some(SafeText::new("toolOutput", output).expect("bounded outcome")),
        locations: BoundedVec::default(),
        command_activity: Default::default(),
    }
}
pub(crate) fn delivery_unknown(resource: &ResourceRef, range: ReadTextRange) -> NormalizedPayload {
    receipt(Some(resource), range, "native", "deliveryUnknown", 0)
}
pub(crate) fn settlement_handle(
    state: Arc<ReadState>,
    tokens: BTreeSet<String>,
) -> ReadTextSettlementHandle {
    ReadTextSettlementHandle { state, tokens }
}

pub(crate) fn owner_session_key(owner: &ReadTextOwner) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}",
        owner.project_id.as_str(),
        owner.project_generation.get(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
        owner.runtime_epoch.get()
    )
}
pub(crate) fn outcome(kind: ReadTextFailureKind) -> &'static str {
    match kind {
        ReadTextFailureKind::Busy => "busy",
        ReadTextFailureKind::Unavailable => "unavailable",
        ReadTextFailureKind::Timeout => "timeout",
        ReadTextFailureKind::Cancelled => "cancelled",
        ReadTextFailureKind::Forbidden => "rejected",
        ReadTextFailureKind::Corrupt => "corrupt",
    }
}
