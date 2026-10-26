//! The element target of the `browserElement` context item
//! (`docs/work-items/BROWSER-FIRST-CLASS/contracts/browser-element-context-v1.md`):
//! what the isolated-world read of a clicked element gives, how each field is
//! bounded and redacted, and the checks the item passes as it leaves the
//! browser layer.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::acp::{
    browser::{redact_console, redact_url, safe_page_text},
    limits::{ATTACHED_CONTEXT_LIMIT, ATTACHED_CONTEXT_NOTE_BYTES, BROWSER_LIMITS},
};

use super::super::page_context::valid_page_context_tab_id;

const URL_BYTES: usize = 2048;
const TITLE_BYTES: usize = 256;
const SELECTOR_BYTES: usize = 512;
const ROLE_BYTES: usize = BROWSER_LIMITS.node_role_bytes;
const NAME_BYTES: usize = 256;
const TEXT_BYTES: usize = 1024;
const ENTRY_BYTES: usize = 8 * 1024;
const COORDINATE_MAX: i64 = 10_000_000;
const EXTENT_MAX: i64 = 100_000;
const SELECTOR_STEP: &str = " > ";

static ROLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9-]*$").expect("valid role regex"));

/// What the isolated-world script reports about the clicked element. The
/// selector is anchored at the nearest unique id; `positional` uses tag
/// positions only, for when a page-chosen id would not pass redaction.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PageElement {
    pub selector: String,
    pub positional: String,
    pub text: String,
    pub sensitive: bool,
    pub title: String,
    pub rect: Rect,
    pub viewport: Viewport,
}

/// A box in page CSS px.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The layout viewport in page CSS px: its scroll offset, its size with and
/// without scrollbars, and device pixels per CSS px.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub scale: f64,
}

impl Viewport {
    /// Whether the page reported a usable viewport.
    pub(crate) fn admitted(&self) -> bool {
        let size = |value: f64| value.is_finite() && (1.0..=EXTENT_MAX as f64).contains(&value);
        [self.x, self.y]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
            && [
                self.width,
                self.height,
                self.client_width,
                self.client_height,
            ]
            .into_iter()
            .all(size)
            && self.scale.is_finite()
            && self.scale > 0.0
            && self.scale <= 8.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct ElementBox {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// A picked element before the user numbers it and comments on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picked {
    pub tab_id: String,
    pub url: String,
    pub title: String,
    pub selector: String,
    pub role: String,
    pub name: String,
    pub text: String,
    pub bounds: ElementBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Kind {
    #[serde(rename = "browserElement")]
    BrowserElement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Target {
    #[serde(rename = "element")]
    Element,
}

/// The wire item, exactly as the contract names its fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserElement {
    kind: Kind,
    target: Target,
    pub ordinal: u8,
    pub tab_id: String,
    pub url: String,
    pub title: String,
    pub selector: String,
    pub role: String,
    pub name: String,
    pub text: String,
    #[serde(rename = "box")]
    pub bounds: ElementBox,
    pub comment: String,
}

/// Bounds and redacts what the page reported. `role` and `name` come from
/// the accessibility tree; a sensitive field (password, hidden, card or
/// one-time-code) keeps its role only.
pub(crate) fn pick(
    page: &PageElement,
    role: &str,
    name: &str,
    tab_id: &str,
    url: &str,
) -> Result<Picked, String> {
    let picked = Picked {
        tab_id: tab_id.to_owned(),
        url: redact_url(url),
        title: settled(&page.title, TITLE_BYTES, false),
        selector: selector(&page.selector)
            .or_else(|| selector(&page.positional))
            .ok_or("browser element selector unavailable")?,
        role: if valid_role(role) { role } else { "generic" }.to_owned(),
        name: if page.sensitive {
            String::new()
        } else {
            settled(name, NAME_BYTES, false)
        },
        text: settled(&page.text, TEXT_BYTES, true),
        bounds: element_box(page.rect)?,
    };
    confirm(&picked, 1, "")?;
    Ok(picked)
}

/// Numbers and comments a picked element, then checks the whole item as the
/// Chat admission will: limits, redaction fixpoints and the entry size.
pub(crate) fn confirm(
    picked: &Picked,
    ordinal: u8,
    comment: &str,
) -> Result<BrowserElement, String> {
    let element = BrowserElement {
        kind: Kind::BrowserElement,
        target: Target::Element,
        ordinal,
        tab_id: picked.tab_id.clone(),
        url: picked.url.clone(),
        title: picked.title.clone(),
        selector: picked.selector.clone(),
        role: picked.role.clone(),
        name: picked.name.clone(),
        text: picked.text.clone(),
        bounds: picked.bounds,
        comment: comment.to_owned(),
    };
    validate(&element)?;
    Ok(element)
}

/// The contract's hard limits and send-time checks, run on the item that
/// leaves the browser layer.
pub(crate) fn validate(element: &BrowserElement) -> Result<(), String> {
    let reject = |field: &str| Err(format!("browser element {field} rejected"));
    if !(1..=ATTACHED_CONTEXT_LIMIT).contains(&usize::from(element.ordinal)) {
        return reject("ordinal");
    }
    if !valid_page_context_tab_id(&element.tab_id) {
        return reject("tab");
    }
    if !valid_url(&element.url) {
        return reject("url");
    }
    if !clean(&element.title, TITLE_BYTES, false) {
        return reject("title");
    }
    if element.selector.is_empty() || !clean(&element.selector, SELECTOR_BYTES, false) {
        return reject("selector");
    }
    if !valid_role(&element.role) {
        return reject("role");
    }
    if !clean(&element.name, NAME_BYTES, false) {
        return reject("name");
    }
    if !clean(&element.text, TEXT_BYTES, true) {
        return reject("text");
    }
    let ElementBox {
        x,
        y,
        width,
        height,
    } = element.bounds;
    if ![x, y]
        .iter()
        .all(|value| (0..=COORDINATE_MAX).contains(value))
        || ![width, height]
            .iter()
            .all(|value| (1..=EXTENT_MAX).contains(value))
    {
        return reject("box");
    }
    if element.comment.len() > ATTACHED_CONTEXT_NOTE_BYTES {
        return reject("comment");
    }
    match serde_json::to_vec(element) {
        Ok(bytes) if bytes.len() <= ENTRY_BYTES => Ok(()),
        _ => reject("entry"),
    }
}

fn valid_url(value: &str) -> bool {
    value.len() <= URL_BYTES
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
        })
        && redact_url(value) == value
}

fn valid_role(role: &str) -> bool {
    role.len() <= ROLE_BYTES && ROLE.is_match(role)
}

/// Within `limit`, free of controls and bidi characters, and unchanged by
/// redaction.
fn clean(value: &str, limit: usize, multiline: bool) -> bool {
    value.len() <= limit
        && safe_page_text(value, limit, multiline) == value
        && redact_console(value) == value
}

/// Redacts, filters and cuts `value` until another pass changes nothing, so
/// the send-time checks accept it. A cut falls between words where it can,
/// so it never leaves half a secret for the next pass to grow back.
fn settled(value: &str, limit: usize, multiline: bool) -> String {
    let mut current = value.to_owned();
    for _ in 0..4 {
        let next = clip_words(
            &safe_page_text(&redact_console(&current), usize::MAX, multiline),
            limit,
        );
        if next == current {
            return current;
        }
        current = next;
    }
    String::new()
}

fn clip_words(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let cut = &value[..end];
    let cut = match cut.rfind(char::is_whitespace) {
        Some(space) if !value[end..].starts_with(char::is_whitespace) => &cut[..space],
        _ => cut,
    };
    cut.trim_end().to_owned()
}

/// The selector, dropping its outermost steps until it fits; `None` when it
/// would not pass the send-time checks.
fn selector(candidate: &str) -> Option<String> {
    let mut steps: Vec<&str> = candidate.split(SELECTOR_STEP).collect();
    while steps.len() > 1 && steps.join(SELECTOR_STEP).len() > SELECTOR_BYTES {
        steps.remove(0);
    }
    let selector = steps.join(SELECTOR_STEP);
    (!selector.is_empty() && clean(&selector, SELECTOR_BYTES, false)).then_some(selector)
}

fn element_box(rect: Rect) -> Result<ElementBox, String> {
    let Rect {
        x,
        y,
        width,
        height,
    } = rect;
    if ![x, y, width, height].iter().all(|value| value.is_finite()) {
        return Err("browser element box unavailable".into());
    }
    let round = |value: f64, low: i64, high: i64| (value.round() as i64).clamp(low, high);
    Ok(ElementBox {
        x: round(x, 0, COORDINATE_MAX),
        y: round(y, 0, COORDINATE_MAX),
        width: round(width, 1, EXTENT_MAX),
        height: round(height, 1, EXTENT_MAX),
    })
}

#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;
