use super::{DurableAdapterEvent, NormalizedFact, TurnScope, TurnTerminalReason};
use crate::acp::contract::SafeText;

pub(super) fn normalize(
    scope: TurnScope,
    reason: TurnTerminalReason,
    message: Option<SafeText<8192>>,
    now_millis: i64,
) -> NormalizedFact {
    let bytes = message.as_ref().map_or(0, |message| message.as_str().len());
    NormalizedFact::new(
        DurableAdapterEvent::TerminalReason {
            scope,
            reason,
            message: message.map(|message| message.as_str().to_owned()),
            remediation: None,
            now_millis,
        },
        bytes,
    )
}
