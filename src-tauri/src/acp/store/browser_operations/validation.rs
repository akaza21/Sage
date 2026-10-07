use crate::acp::projection::BrowserOperationProjection;

use super::StoreError;

pub(super) fn validate(record: &BrowserOperationProjection) -> Result<(), StoreError> {
    let valid_kind = matches!(
        record.operation.as_str(),
        "openTab"
            | "navigate"
            | "snapshotPage"
            | "act"
            | "waitFor"
            | "screenshot"
            | "readConsole"
            | "readNetwork"
    );
    let valid_phase = matches!(
        record.phase.as_str(),
        "started" | "completed" | "denied" | "failed" | "outcomeUnknown"
    );
    let valid_action_summary = if record.operation == "act" {
        matches!(
            record.summary.as_str(),
            "Click element" | "Fill field" | "Press key" | "Scroll page"
        ) || record
            .summary
            .strip_prefix("Click \"")
            .and_then(|label| label.strip_suffix('"'))
            .is_some_and(|label| {
                !label.is_empty()
                    && label.len() <= 80
                    && !label.chars().any(|ch| {
                        ch.is_control()
                            || ch == '"'
                            || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                    })
            })
    } else {
        true
    };
    if !valid_kind
        || !valid_phase
        || !valid_action_summary
        || (record.operation == "navigate" && record.summary != "Navigate to origin")
        || uuid::Uuid::parse_str(&record.operation_id).is_err()
        || !record.tab_id.starts_with("browser-")
        || record.tab_id.len() > 128
        || record.origin.len() > 512
        || record.summary.is_empty()
        || record.summary.len() > 512
        || record
            .result_summary
            .as_ref()
            .is_some_and(|value| value.len() > 512)
        || record.updated_at < record.created_at
    {
        return Err(StoreError::Corrupt("browser operation shape"));
    }
    Ok(())
}
