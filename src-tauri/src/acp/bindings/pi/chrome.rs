//! Pi's fire-and-forget extension UI (`setStatus`, `setWidget`,
//! `set_editor_text`, `setTitle`) as bounded composer chrome. Every
//! extension renders by request kind; the runtime keeps one bounded state and
//! reports each change as a whole snapshot. What cannot be shown becomes a
//! note, once per kind, rather than vanishing.

use sage_acp_protocol::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::session_event::{
        CHROME_EDITOR_TEXT_BYTES, CHROME_KEY_BYTES, CHROME_LINE_BYTES, CHROME_STATUS_BYTES,
        CHROME_STATUSES_MAX, CHROME_WIDGET_LINES_MAX, CHROME_WIDGETS_MAX, ChromeEditorText,
        ChromePlacement, ChromeStatus, ChromeWidget, SESSION_TITLE_BYTES, SessionChrome,
    },
};
use serde_json::Value;
use std::time::{Duration, Instant};

use super::decode::redact_structured_text;
use crate::acp::redaction::text::{clip_display, neutralized};

type Key = RequiredText<CHROME_KEY_BYTES>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeUpdate {
    Status {
        key: Key,
        text: Option<RequiredText<CHROME_STATUS_BYTES>>,
    },
    Widget {
        key: Key,
        widget: Option<ChromeWidget>,
    },
    EditorText(Option<RequiredText<CHROME_EDITOR_TEXT_BYTES>>),
    Subtitle(Option<RequiredText<SESSION_TITLE_BYTES>>),
    Refused(ChromeNote),
}

/// Why a request was not shown. Each kind is noted once per runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeNote {
    StatusLimit,
    WidgetLimit,
    NotText,
    TooLong,
    BadKey,
}

impl ChromeNote {
    pub(crate) fn identity(self) -> &'static str {
        match self {
            Self::StatusLimit => "pi-extension-ui:status-limit",
            Self::WidgetLimit => "pi-extension-ui:widget-limit",
            Self::NotText => "pi-extension-ui:not-text",
            Self::TooLong => "pi-extension-ui:editor-text-limit",
            Self::BadKey => "pi-extension-ui:key",
        }
    }

    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::StatusLimit => {
                "A Pi extension set more than 8 status lines; Sage shows the first 8."
            }
            Self::WidgetLimit => "A Pi extension set more than 4 widgets; Sage shows the first 4.",
            Self::NotText => "A Pi extension sent interface Sage can't show as text.",
            Self::TooLong => {
                "A Pi extension suggested composer text over 16 KiB; Sage didn't use it."
            }
            Self::BadKey => "A Pi extension used a status or widget key Sage can't show.",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The update a fire-and-forget request makes, or `None` for other methods.
pub(crate) fn parse(method: &str, value: &Value) -> Option<ChromeUpdate> {
    use ChromeUpdate::Refused;
    Some(match method {
        "setStatus" => match (key(value.get("statusKey")), value.get("statusText")) {
            (None, _) => Refused(ChromeNote::BadKey),
            (Some(key), None | Some(Value::Null)) => ChromeUpdate::Status { key, text: None },
            (Some(key), Some(Value::String(text))) => ChromeUpdate::Status {
                key,
                text: one_line(text),
            },
            _ => Refused(ChromeNote::NotText),
        },
        "setWidget" => {
            let Some(key) = key(value.get("widgetKey")) else {
                return Some(Refused(ChromeNote::BadKey));
            };
            let placement = match value.get("widgetPlacement").and_then(Value::as_str) {
                Some("belowEditor") => ChromePlacement::Below,
                _ => ChromePlacement::Above,
            };
            match value.get("widgetLines") {
                None | Some(Value::Null) => ChromeUpdate::Widget { key, widget: None },
                Some(Value::Array(lines)) if lines.iter().all(Value::is_string) => {
                    let widget = widget(key.clone(), placement, lines);
                    ChromeUpdate::Widget { key, widget }
                }
                _ => Refused(ChromeNote::NotText),
            }
        }
        "set_editor_text" => match value.get("text") {
            None | Some(Value::Null) => ChromeUpdate::EditorText(None),
            Some(Value::String(text)) if text.len() > CHROME_EDITOR_TEXT_BYTES => {
                Refused(ChromeNote::TooLong)
            }
            Some(Value::String(text)) => {
                let text = redact_structured_text(&neutralized(text, true));
                if text.trim().is_empty() {
                    ChromeUpdate::EditorText(None)
                } else {
                    RequiredText::new("chromeEditorText", text)
                        .map_or(Refused(ChromeNote::TooLong), |text| {
                            ChromeUpdate::EditorText(Some(text))
                        })
                }
            }
            _ => Refused(ChromeNote::NotText),
        },
        "setTitle" => match value.get("title") {
            None | Some(Value::Null) => ChromeUpdate::Subtitle(None),
            Some(Value::String(title)) => ChromeUpdate::Subtitle(one_line(title)),
            _ => Refused(ChromeNote::NotText),
        },
        _ => return None,
    })
}

fn key(value: Option<&Value>) -> Option<Key> {
    let key = value?.as_str()?;
    RequiredText::new("chromeKey", key)
        .ok()
        .filter(|_| !key.chars().any(char::is_control))
}

/// Display text on one line: controls neutralized, secrets redacted,
/// whitespace collapsed and clipped to `MAX`; blank text is `None`.
fn one_line<const MAX: usize>(text: &str) -> Option<RequiredText<MAX>> {
    let text = redact_structured_text(&neutralized(text, true));
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    RequiredText::new("chromeText", clip_display(&text, MAX)).ok()
}

/// Each string is one or more lines; past the line cap they are only counted.
fn widget(key: Key, placement: ChromePlacement, values: &[Value]) -> Option<ChromeWidget> {
    let mut lines = Vec::new();
    let mut hidden = 0_u32;
    for line in values.iter().filter_map(Value::as_str).flat_map(str::lines) {
        if lines.len() == CHROME_WIDGET_LINES_MAX {
            hidden = hidden.saturating_add(1);
            continue;
        }
        let line = redact_structured_text(&neutralized(line, true));
        lines.push(
            SafeText::new(
                "chromeLine",
                clip_display(line.trim_end(), CHROME_LINE_BYTES),
            )
            .ok()?,
        );
    }
    if lines.iter().all(|line| line.as_str().trim().is_empty()) {
        return None;
    }
    Some(ChromeWidget {
        key,
        placement,
        lines: BoundedVec::new("chromeLines", lines).ok()?,
        hidden_lines: hidden,
    })
}

/// One runtime's chrome. Bounded by the protocol caps; nothing grows with
/// the number of requests.
#[derive(Debug, Default)]
pub(crate) struct PiChrome {
    statuses: Vec<ChromeStatus>,
    widgets: Vec<ChromeWidget>,
    editor_text: Option<ChromeEditorText>,
    subtitle: Option<RequiredText<SESSION_TITLE_BYTES>>,
    revision: u64,
    noted: u8,
    dirty: bool,
    /// When the last snapshot left, so a ticking status coalesces.
    sent_at: Option<Instant>,
}

/// The shortest gap between two snapshots; changes in between are merged.
pub(crate) const CHROME_SNAPSHOT_GAP: Duration = Duration::from_millis(250);

impl PiChrome {
    /// Applies one update. Returns a note the first time this runtime
    /// refuses a request of its kind.
    pub(crate) fn apply(&mut self, update: ChromeUpdate) -> Option<ChromeNote> {
        let note = match update {
            ChromeUpdate::Status { key, text } => {
                let index = self.statuses.iter().position(|row| row.key == key);
                match (index, text) {
                    (Some(index), None) => {
                        self.statuses.remove(index);
                        self.dirty = true;
                        None
                    }
                    (Some(index), Some(text)) => {
                        self.dirty |= self.statuses[index].text != text;
                        self.statuses[index].text = text;
                        None
                    }
                    (None, None) => None,
                    (None, Some(_)) if self.statuses.len() == CHROME_STATUSES_MAX => {
                        Some(ChromeNote::StatusLimit)
                    }
                    (None, Some(text)) => {
                        self.statuses.push(ChromeStatus { key, text });
                        self.dirty = true;
                        None
                    }
                }
            }
            ChromeUpdate::Widget { key, widget } => {
                let index = self.widgets.iter().position(|row| row.key == key);
                match (index, widget) {
                    (Some(index), None) => {
                        self.widgets.remove(index);
                        self.dirty = true;
                        None
                    }
                    (Some(index), Some(widget)) => {
                        self.dirty |= self.widgets[index] != widget;
                        self.widgets[index] = widget;
                        None
                    }
                    (None, None) => None,
                    (None, Some(_)) if self.widgets.len() == CHROME_WIDGETS_MAX => {
                        Some(ChromeNote::WidgetLimit)
                    }
                    (None, Some(widget)) => {
                        self.widgets.push(widget);
                        self.dirty = true;
                        None
                    }
                }
            }
            ChromeUpdate::EditorText(None) => {
                self.clear_offer();
                None
            }
            ChromeUpdate::EditorText(Some(text)) => {
                self.revision = self.revision.saturating_add(1);
                self.editor_text = Some(ChromeEditorText {
                    revision: self.revision,
                    text,
                });
                self.dirty = true;
                None
            }
            ChromeUpdate::Subtitle(subtitle) => {
                self.dirty |= self.subtitle != subtitle;
                self.subtitle = subtitle;
                None
            }
            ChromeUpdate::Refused(note) => Some(note),
        };
        note.filter(|note| self.noted & note.bit() == 0)
            .inspect(|note| self.noted |= note.bit())
    }

    /// A sent prompt answers any open composer offer.
    pub(crate) fn clear_offer(&mut self) {
        self.dirty |= self.editor_text.take().is_some();
    }

    /// When a pending change may next be sent, if one is waiting.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.dirty
            .then(|| self.sent_at.map(|sent| sent + CHROME_SNAPSHOT_GAP))
            .flatten()
    }

    /// The whole chrome after a change, at most once per
    /// [`CHROME_SNAPSHOT_GAP`]; a later change inside the gap waits for
    /// [`Self::deadline`] and is sent merged with any others.
    pub(crate) fn take_snapshot(&mut self, now: Instant) -> Option<SessionChrome> {
        if !self.dirty || self.deadline().is_some_and(|due| now < due) {
            return None;
        }
        self.dirty = false;
        self.sent_at = Some(now);
        Some(SessionChrome {
            statuses: BoundedVec::new("chromeStatuses", self.statuses.clone()).ok()?,
            widgets: BoundedVec::new("chromeWidgets", self.widgets.clone()).ok()?,
            editor_text: self.editor_text.clone(),
            subtitle: self.subtitle.clone(),
        })
    }
}
