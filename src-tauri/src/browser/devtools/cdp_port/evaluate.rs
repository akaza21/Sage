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
        let params = json!({
            "objectId": object_id,
            "objectGroup": crate::acp::browser::developer::DEVELOPER_OBJECT_GROUP,
            "functionDeclaration": SCRIPT.trim_start_matches("export default ").trim(),
            "arguments": [{"value": BROWSER_LIMITS.cdp_result_bytes}],
            "returnByValue": true,
            "silent": true,
        })
        .to_string();
        drop(response);
        let response = self.transport.raw(
            "Runtime.callFunctionOn",
            &params,
            // The bounded JSON string has one additional escape layer.
            2 * BROWSER_LIMITS.cdp_result_bytes + 4096,
            left(deadline)?,
        )?;
        decode(response)
    }
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
