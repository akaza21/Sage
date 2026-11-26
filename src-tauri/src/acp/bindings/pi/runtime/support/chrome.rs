use crate::acp::bindings::adapter::{AdapterBatch, AdapterEvent, AdapterRuntimeError};
use sage_acp_protocol::{
    contract::RequiredText,
    intent::session_event::{SessionNotice, SessionNoticeLevel, SessionScopedEvent},
};

use super::super::PiRuntime;
use crate::acp::bindings::pi::chrome::ChromeUpdate;

impl PiRuntime {
    /// Folds one request into the chrome; a refusal becomes a session notice
    /// the first time its kind occurs.
    pub(super) fn chrome_update(
        &mut self,
        update: ChromeUpdate,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        match self.chrome.apply(update) {
            Some(note) => self.notice(
                SessionNoticeLevel::Info,
                "Pi extension",
                note.text(),
                Some(note.identity()),
            ),
            None => Ok(AdapterEvent::Absorbed),
        }
    }

    /// The whole chrome once it changed. It waits for Pi's session, so what
    /// extensions set while Pi starts up still shows once the chat opens.
    pub(super) fn chrome_snapshot(&mut self) -> Option<AdapterEvent> {
        self.external_session.as_ref()?;
        let chrome = self.chrome.take_snapshot(self.translator.question_now())?;
        Some(self.session_event(SessionScopedEvent::Chrome(chrome)))
    }

    /// The next question expiry or coalesced chrome change.
    pub(in crate::acp::bindings::pi::runtime) fn next_deadline(
        &self,
    ) -> Option<std::time::Instant> {
        let chrome = self
            .external_session
            .as_ref()
            .and_then(|_| self.chrome.deadline());
        self.next_question_deadline()
            .into_iter()
            .chain(chrome)
            .min()
    }

    /// Expired questions and a chrome change that waited out its gap.
    pub(in crate::acp::bindings::pi::runtime) fn poll_deadlines(
        &mut self,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        let expired = self.expire_queued_questions()?;
        let Some(chrome) = self.chrome_snapshot() else {
            return Ok(expired);
        };
        let (mut events, egress) = expired.map(AdapterBatch::into_parts).unwrap_or_default();
        events.push(chrome);
        AdapterBatch::new(events, egress).map(Some)
    }

    /// A session notice; notices of one identity replace each other.
    pub(super) fn notice(
        &self,
        level: SessionNoticeLevel,
        source: &str,
        message: &str,
        identity: Option<&str>,
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        Ok(
            self.session_event(SessionScopedEvent::Notice(SessionNotice {
                usage_limit: None,
                identity: identity
                    .map(|identity| text("noticeIdentity", identity))
                    .transpose()?,
                level,
                diagnostic: false,
                text: text("noticeText", message)?,
                source: Some(text("noticeSource", source)?),
            })),
        )
    }
}

fn text<const MAX: usize>(
    field: &'static str,
    value: &str,
) -> Result<RequiredText<MAX>, AdapterRuntimeError> {
    RequiredText::new(field, value).map_err(|_| AdapterRuntimeError::Frame)
}
