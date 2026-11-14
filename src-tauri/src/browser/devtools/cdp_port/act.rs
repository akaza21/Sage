use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::acp::browser::{BrowserAction, BrowserFailure};
use crate::browser::devtools::snapshot::DocumentStamp;

use super::{CdpPort, CdpTransport, highlight};

pub(super) fn perform<T: CdpTransport>(
    port: &mut CdpPort<T>,
    stamp: DocumentStamp,
    backend_id: u64,
    action: &BrowserAction,
) -> Result<bool, BrowserFailure> {
    if port.stamp()? != stamp {
        return Err(BrowserFailure::StaleHandle);
    }
    match action {
        BrowserAction::Key { key, .. } => {
            let text = if key.chars().count() == 1 {
                key.as_str()
            } else {
                ""
            };
            // Focus after the marker's lead, right before the keys, so the
            // page has no extra time to move focus elsewhere.
            marked(port, stamp, backend_id, |port| {
                port.call(
                    "DOM.focus",
                    &json!({"backendNodeId": backend_id}).to_string(),
                    4096,
                    Duration::from_secs(5),
                )?;
                if port.stamp()? != stamp {
                    return Err(BrowserFailure::StaleHandle);
                }
                for kind in ["keyDown", "keyUp"] {
                    port.call("Input.dispatchKeyEvent", &json!({"type":kind,"key":key,"text": if kind == "keyDown" { text } else { "" }}).to_string(), 4096, Duration::from_secs(5))?;
                }
                Ok(true)
            })
        }
        _ => {
            let deadline = Instant::now() + Duration::from_secs(5);
            let resolved = port.in_agent_world(
                Some(stamp.navigation_id),
                false,
                deadline,
                |context, left| {
                    port.call(
                        "DOM.resolveNode",
                        &json!({"backendNodeId":backend_id,"executionContextId":context})
                            .to_string(),
                        4096,
                        left,
                    )
                },
            )?;
            let resolved: Value =
                serde_json::from_str(&resolved).map_err(|_| BrowserFailure::Unavailable)?;
            let object_id = resolved["object"]["objectId"]
                .as_str()
                .ok_or(BrowserFailure::StaleHandle)?;
            if port.stamp()? != stamp {
                let _ = port.call(
                    "Runtime.releaseObject",
                    &json!({"objectId":object_id}).to_string(),
                    4096,
                    Duration::from_secs(5),
                );
                return Err(BrowserFailure::StaleHandle);
            }
            let (function, arguments) = match action {
                BrowserAction::Click { .. } => {
                    ("function() { this.click(); return true; }", vec![])
                }
                BrowserAction::Fill { value, .. } => (
                    "function(value) { if (!(this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement || this.isContentEditable)) return false; this.focus(); if (this.isContentEditable) { this.textContent = value; } else { const proto = this instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, 'value').set.call(this, value); } this.dispatchEvent(new InputEvent('input', {bubbles:true})); return true; }",
                    vec![json!({"value":value.expose()})],
                ),
                BrowserAction::Scroll {
                    delta_x, delta_y, ..
                } => (
                    "function(x, y) { this.scrollBy(x, y); return true; }",
                    vec![json!({"value":delta_x}), json!({"value":delta_y})],
                ),
                BrowserAction::Key { .. } => unreachable!(),
            };
            let params = json!({"objectId":object_id,"functionDeclaration":function,"arguments":arguments,"returnByValue":true});
            let response = marked(port, stamp, backend_id, |port| {
                port.call(
                    "Runtime.callFunctionOn",
                    &params.to_string(),
                    4096,
                    Duration::from_secs(5),
                )
            });
            let _ = port.call(
                "Runtime.releaseObject",
                &json!({"objectId":object_id}).to_string(),
                4096,
                Duration::from_secs(5),
            );
            let response: Value =
                serde_json::from_str(&response?).map_err(|_| BrowserFailure::Unavailable)?;
            if response.get("exceptionDetails").is_some() {
                return Err(BrowserFailure::Unavailable);
            }
            Ok(response["result"]["value"].as_bool().unwrap_or(false))
        }
    }
}

/// Marking is best effort and never fails or delays the action past
/// `highlight::LEAD`.
fn marked<T: CdpTransport, R>(
    port: &mut CdpPort<T>,
    stamp: DocumentStamp,
    backend_id: u64,
    action: impl FnOnce(&mut CdpPort<T>) -> Result<R, BrowserFailure>,
) -> Result<R, BrowserFailure> {
    if !highlight::mark(port, stamp, backend_id) {
        return action(port);
    }
    std::thread::sleep(highlight::LEAD);
    let result = action(port);
    highlight::clear(&port.transport);
    result
}

#[cfg(test)]
#[path = "act_tests.rs"]
mod tests;
