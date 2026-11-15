//! Codex reports a compaction as a `contextCompaction` item and, in older
//! releases or alongside it, as the legacy `thread/compacted`. The nth report
//! from one source pairs with the nth from the other, so each compaction shows
//! once whichever arrives first.

use super::CodexAdapter;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Compactions {
    turn: String,
    items: u16,
    legacy: u16,
}

impl CodexAdapter {
    /// Counts one compaction report in `turn` and returns its ordinal among
    /// its source's reports, or `None` when the other source already showed
    /// that compaction.
    pub(in crate::acp::bindings::codex) fn record_compaction(
        &mut self,
        turn: &str,
        legacy: bool,
    ) -> Option<u16> {
        let seen = &mut self.compactions;
        if seen.turn != turn {
            *seen = Compactions {
                turn: turn.to_owned(),
                ..Compactions::default()
            };
        }
        let (own, other) = if legacy {
            (&mut seen.legacy, seen.items)
        } else {
            (&mut seen.items, seen.legacy)
        };
        *own = own.saturating_add(1);
        (*own > other).then_some(*own)
    }
}
