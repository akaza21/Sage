//! Objects cross CDP only through a byte-budgeted serializer. CDP sends string
//! primitives inline even with returnByValue:false: the explicitly approved
//! agent grant is the trust boundary for that transient allocation. Reject it
//! immediately on receipt, before parsing or copying, and never log payloads.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{borrow::Cow, time::Instant};

use super::{CdpPort, CdpTransport, world::left};
use crate::acp::{browser::BrowserFailure, limits::BROWSER_LIMITS};

const SCRIPT: &str = include_str!("evaluate.js");
const ENTRY_LIMIT: usize = 4096;
const SERIALIZATION_FAILURE: &str = r#"{"result":{"type":"undefined"},"exceptionDetails":{"text":"Result could not be serialized"}}"#;

#[derive(Deserialize)]
struct Evaluation<'a> {
    #[serde(borrow)]
    result: RemoteObject<'a>,
    #[serde(rename = "exceptionDetails")]
    exception: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct RemoteObject<'a> {
    #[serde(borrow, rename = "objectId")]
    object_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    subtype: Option<Cow<'a, str>>,
}

impl<T: CdpTransport> CdpPort<T> {
    pub(super) fn bounded_evaluate(
        &self,
        params: &str,
        deadline: Instant,
    ) -> Result<String, BrowserFailure> {
        // Do not wrap the expression: CDP owns lexical/repl/await semantics.
        let response = self.transport.raw(
            "Runtime.evaluate",
            params,
            BROWSER_LIMITS.cdp_result_bytes,
            left(deadline)?,
        )?;
        // A transport must reject oversized inline results before returning;
        // check again before this borrowed parse for every transport.
        let response = admit(response)?;
        let evaluation: Evaluation<'_> =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        let Some(object_id) = evaluation
            .result
            .object_id
            .filter(|_| evaluation.exception.is_none())
        else {
            return Ok(response);
        };
        if evaluation.result.subtype.as_deref() == Some("proxy") {
            return Ok(SERIALIZATION_FAILURE.into());
        }
        let mut receiver = object_id.into_owned();
        let mut approved = 1;
        let mut resume = false;
        drop(response);
        let mut remaining = ENTRY_LIMIT;
        // Each pass adds native-classified object/prototype references. All
        // page entries visited across passes share one budget. The separate
        // pass/reference caps also bound chains with no enumerable entries.
        for _ in 0..128 {
            let params = json!({
                "objectId": receiver,
                "objectGroup": crate::acp::browser::developer::DEVELOPER_OBJECT_GROUP,
                "functionDeclaration": SCRIPT.trim_start_matches("export default ").trim(),
                "arguments": [
                    {"value": BROWSER_LIMITS.cdp_result_bytes},
                    {"value": remaining},
                    {"value": resume},
                ],
                "returnByValue": false,
                "silent": true,
                // Preserve the original side-effect restriction, and require
                // it even for effectful evaluations. Never retry unguarded.
                "throwOnSideEffect": true,
            })
            .to_string();
            let envelope_limit = 2 * BROWSER_LIMITS.cdp_result_bytes + 4096;
            let response = self.transport.raw(
                "Runtime.callFunctionOn",
                &params,
                envelope_limit,
                left(deadline)?,
            )?;
            if response.len() > envelope_limit {
                return Err(BrowserFailure::Capacity);
            }
            let metadata: Evaluation<'_> =
                serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
            let Some(pending_id) = metadata
                .result
                .object_id
                .filter(|_| metadata.exception.is_none())
            else {
                return decode(response);
            };
            // Only this fixed serializer's bounded, null-prototype scratch
            // record is inspected, never a page object's unrestricted keys.
            receiver = pending_id.into_owned();
            let params =
                json!({"objectId": receiver, "ownProperties": true, "generatePreview": false})
                    .to_string();
            drop(response);
            let pending = self.transport.raw(
                "Runtime.getProperties",
                &params,
                envelope_limit,
                left(deadline)?,
            )?;
            if !classify_pending(pending, &mut approved, &mut remaining)? {
                return Ok(SERIALIZATION_FAILURE.into());
            }
            resume = true;
        }
        Err(BrowserFailure::Capacity)
    }
}

fn classify_pending(
    response: String,
    approved: &mut usize,
    remaining: &mut usize,
) -> Result<bool, BrowserFailure> {
    if response.len() > 2 * BROWSER_LIMITS.cdp_result_bytes + 4096 {
        return Err(BrowserFailure::Capacity);
    }
    let parsed: Value = serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
    let properties = parsed["result"]
        .as_array()
        .ok_or(BrowserFailure::Unavailable)?;
    let value = |name: &str| {
        properties
            .iter()
            .find(|p| p["name"] == name)
            .map(|p| &p["value"]["value"])
    };
    let visited = value("0")
        .and_then(Value::as_u64)
        .ok_or(BrowserFailure::Unavailable)?;
    let length = value("length")
        .and_then(Value::as_u64)
        .ok_or(BrowserFailure::Unavailable)?;
    if !(2..=ENTRY_LIMIT as u64 + 1).contains(&length) || properties.len() as u64 != length + 3 {
        return Err(BrowserFailure::Unavailable);
    }
    *remaining = remaining
        .checked_sub(usize::try_from(visited).map_err(|_| BrowserFailure::Capacity)?)
        .ok_or(BrowserFailure::Capacity)?;
    for property in properties {
        let name = property["name"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        if matches!(name, "0" | "length" | "root" | "previous") {
            continue;
        }
        let remote = &property["value"];
        // CDP inspects the internal Proxy brand without running traps. V8's
        // side-effect checker alone can still execute pure Proxy handlers.
        if remote["subtype"] == "proxy" {
            return Ok(false);
        }
        if *approved >= ENTRY_LIMIT {
            return Err(BrowserFailure::Capacity);
        }
        remote["objectId"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        *approved += 1;
    }
    Ok(true)
}

fn admit(response: String) -> Result<String, BrowserFailure> {
    if response.len() > BROWSER_LIMITS.cdp_result_bytes {
        // Ownership ends here; no clone, parse, log, or retained buffer.
        return Err(BrowserFailure::Capacity);
    }
    Ok(response)
}

fn decode(response: String) -> Result<String, BrowserFailure> {
    if response.len() > 2 * BROWSER_LIMITS.cdp_result_bytes + 4096 {
        return Err(BrowserFailure::Capacity);
    }
    let mut parsed: Value =
        serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
    if parsed
        .get("exceptionDetails")
        .is_some_and(|details| !details.is_null())
    {
        return admit(response);
    }
    let Value::String(encoded) = parsed["result"]["value"].take() else {
        return Err(BrowserFailure::Unavailable);
    };
    if encoded == "capacity" {
        return Err(BrowserFailure::Capacity);
    }
    admit(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_capacity_and_values_become_browser_results() {
        assert_eq!(
            decode(r#"{"result":{"type":"string","value":"capacity"}}"#.into()),
            Err(BrowserFailure::Capacity)
        );
        let value = r#"{"result":{"type":"number","value":42}}"#;
        assert_eq!(
            decode(json!({"result": {"type": "string", "value": value}}).to_string()),
            Ok(value.into())
        );
    }

    #[test]
    fn oversized_inline_result_is_consumed_without_parsing_or_panicking() {
        // Invalid JSON proves the size check happens before the parser. The
        // only owner is moved in; Capacity contains no buffer to retain.
        let response = "x".repeat(BROWSER_LIMITS.cdp_result_bytes + 1);
        assert_eq!(admit(response), Err(BrowserFailure::Capacity));
        let response = r#"{"result":{"type":"string","value":"small"}}"#.to_owned();
        let original_buffer = response.as_ptr();
        let admitted = admit(response).unwrap();
        assert_eq!(admitted.as_ptr(), original_buffer);
        let metadata: Evaluation<'_> = serde_json::from_str(&admitted).unwrap();
        assert!(metadata.result.object_id.is_none());
        assert_eq!(admitted, r#"{"result":{"type":"string","value":"small"}}"#);
    }
}
