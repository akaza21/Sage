//! Reply text across assistant messages: text Claude Code writes after a
//! tool call starts a new paragraph instead of running on from the last.

use super::SdkBridge;

/// Where the turn's reply text stands between assistant messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextBreak {
    /// No reply text yet this turn.
    None,
    /// Text in the current message.
    Writing,
    /// Text in an earlier message; the next text needs a paragraph break.
    Pending,
}

impl SdkBridge {
    /// Reply text, opening a new paragraph when an earlier message of the
    /// turn already wrote some, so text around a tool call stays apart.
    pub(super) fn reply_text(&mut self, text: &str) -> String {
        let text = if self.text_break == TextBreak::Pending {
            format!("\n\n{text}")
        } else {
            text.to_owned()
        };
        self.text_break = TextBreak::Writing;
        text
    }

    pub(super) fn end_text(&mut self) {
        if self.text_break == TextBreak::Writing {
            self.text_break = TextBreak::Pending;
        }
    }
}
