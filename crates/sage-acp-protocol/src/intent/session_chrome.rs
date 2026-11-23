//! Harness-owned composer chrome: status lines, text widgets, a suggested
//! composer text and a subtitle. Each harness binding keeps its own bounded
//! state and reports it here as one whole snapshot; the surface holds the
//! newest snapshot for the live runtime epoch only.

use serde::{Deserialize, Serialize};

use super::SESSION_TITLE_BYTES;
use crate::contract::{BoundedVec, RequiredText, SafeText};

pub const CHROME_STATUSES_MAX: usize = 8;
pub const CHROME_STATUS_BYTES: usize = 200;
pub const CHROME_WIDGETS_MAX: usize = 4;
pub const CHROME_WIDGET_LINES_MAX: usize = 10;
pub const CHROME_LINE_BYTES: usize = 200;
pub const CHROME_KEY_BYTES: usize = 128;
pub const CHROME_EDITOR_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChromeStatus {
    pub key: RequiredText<CHROME_KEY_BYTES>,
    pub text: RequiredText<CHROME_STATUS_BYTES>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChromePlacement {
    Above,
    Below,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChromeWidget {
    pub key: RequiredText<CHROME_KEY_BYTES>,
    pub placement: ChromePlacement,
    pub lines: BoundedVec<SafeText<CHROME_LINE_BYTES>, CHROME_WIDGET_LINES_MAX>,
    /// Lines past the cap, counted rather than kept.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hidden_lines: u32,
}

/// Text the harness offers for the composer. `revision` grows with each
/// offer in one runtime epoch, so the WebView can tell a new offer from one
/// it already handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChromeEditorText {
    pub revision: u64,
    pub text: RequiredText<CHROME_EDITOR_TEXT_BYTES>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionChrome {
    #[serde(default, skip_serializing_if = "is_empty")]
    pub statuses: BoundedVec<ChromeStatus, CHROME_STATUSES_MAX>,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub widgets: BoundedVec<ChromeWidget, CHROME_WIDGETS_MAX>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_text: Option<ChromeEditorText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<RequiredText<SESSION_TITLE_BYTES>>,
}

impl SessionChrome {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Frees the largest display state when the stored row is over its cap:
    /// the composer offer first, then the widgets.
    pub(super) fn evict_largest(&mut self) -> bool {
        if self.editor_text.take().is_some() {
            return true;
        }
        if self.widgets.as_slice().is_empty() {
            return false;
        }
        self.widgets = BoundedVec::default();
        true
    }

    pub(super) fn view(&self, epoch: u64) -> SessionChromeView {
        SessionChromeView {
            statuses: self.statuses.clone(),
            widgets: self.widgets.clone(),
            editor_text: self.editor_text.as_ref().map(|offer| ChromeEditorTextView {
                key: format!("{epoch}-{}", offer.revision),
                text: offer.text.clone(),
            }),
            subtitle: self.subtitle.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeEditorTextView {
    pub key: String,
    pub text: RequiredText<CHROME_EDITOR_TEXT_BYTES>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChromeView {
    #[serde(skip_serializing_if = "is_empty")]
    pub statuses: BoundedVec<ChromeStatus, CHROME_STATUSES_MAX>,
    #[serde(skip_serializing_if = "is_empty")]
    pub widgets: BoundedVec<ChromeWidget, CHROME_WIDGETS_MAX>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_text: Option<ChromeEditorTextView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<RequiredText<SESSION_TITLE_BYTES>>,
}

fn is_empty<T, const MAX: usize>(values: &BoundedVec<T, MAX>) -> bool {
    values.as_slice().is_empty()
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}
