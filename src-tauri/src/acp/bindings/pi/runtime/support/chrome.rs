use crate::acp::bindings::adapter::{AdapterEvent, AdapterRuntimeError};
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
        let chrome = self.chrome.take_snapshot()?;
        Some(self.session_event(SessionScopedEvent::Chrome(chrome)))
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
