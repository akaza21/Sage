use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use serde::Serialize;
use serde_json::Value;

use crate::acp::browser::{redact_console, redact_url};

use super::super::snapshot::project;
use super::{BrowserState, DevtoolsPort, NativePort};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapturedPageContext {
    kind: &'static str,
    tab_id: String,
    url: String,
    title: String,
    text: String,
}

fn safe_page_text(value: &str, limit: usize, multiline: bool) -> String {
    let filtered: String = value
        .chars()
        .filter(|ch| {
            (!ch.is_control() || (multiline && *ch == '\n'))
                && !matches!(*ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let mut end = filtered.len().min(limit);
    while !filtered.is_char_boundary(end) {
        end -= 1;
    }
    filtered[..end].to_owned()
}

pub(crate) fn capture_page_context(
    state: &BrowserState,
    tab_id: &str,
    expected_revision: &str,
) -> Result<CapturedPageContext, String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable")?;
    let mut port = NativePort {
        webview,
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    };
    let before = port.stamp().map_err(|_| "browser page unavailable")?;
    if before.generation.to_string() != expected_revision {
        return Err("browser page changed; retry".into());
    }
    let url = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .openable_url(tab_id)
        .map_err(|_| "browser page unavailable")?;
    let (snapshot_stamp, nodes) = port
        .snapshot()
        .map_err(|_| "browser page snapshot unavailable")?;
    if snapshot_stamp != before {
        return Err("browser page changed; retry".into());
    }
    let (ax_text, _) = project(nodes).map_err(|_| "browser page snapshot unavailable")?;
    let payload = port.call("Runtime.evaluate", r#"{"expression":"({title:document.title,selection:window.getSelection()?.toString().slice(0,2048) ?? ''})","returnByValue":true}"#, 16_384, Duration::from_secs(5))
        .map_err(|_| "browser page title unavailable")?;
    let value: Value =
        serde_json::from_str(&payload).map_err(|_| "browser page title unavailable")?;
    let title = value["result"]["value"]["title"]
        .as_str()
        .ok_or("browser page title unavailable")?;
    let selection = value["result"]["value"]["selection"].as_str().unwrap_or("");
    if port.stamp().map_err(|_| "browser page changed; retry")? != before {
        return Err("browser page changed; retry".into());
    }
    let mut text = String::new();
    if !selection.is_empty() {
        text.push_str("Selected text:\n");
        text.push_str(&safe_page_text(&redact_console(selection), 1024, true));
        text.push_str("\n\n");
    }
    text.push_str("Page accessibility text:\n");
    text.push_str(&safe_page_text(
        &redact_console(&ax_text),
        4096 - text.len(),
        true,
    ));
    let url = redact_url(&url);
    if url == "[redacted]" || url.len() > 2048 {
        return Err("browser page URL unavailable".into());
    }
    Ok(CapturedPageContext {
        kind: "browserPage",
        tab_id: tab_id.to_owned(),
        url,
        title: safe_page_text(&redact_console(title), 256, false),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::safe_page_text;

    #[test]
    fn page_text_filter_removes_controls_and_bounds_multibyte_text() {
        assert_eq!(safe_page_text("A\u{202e}B\nC", 16, true), "AB\nC");
        assert_eq!(safe_page_text("ééé", 5, true), "éé");
    }
}
