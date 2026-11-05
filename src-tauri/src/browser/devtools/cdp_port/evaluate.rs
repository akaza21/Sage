//! Objects cross CDP only through a byte-budgeted serializer. CDP sends string
//! primitives inline even with returnByValue:false: the explicitly approved
//! agent grant is the trust boundary for that transient allocation. Reject it
//! immediately on receipt, before parsing or copying, and never log payloads.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
    time::Instant,
};

use super::{CdpPort, CdpTransport, world::left};
use crate::acp::{browser::BrowserFailure, limits::BROWSER_LIMITS};

const SCRIPT: &str = include_str!("evaluate.js");
const ENTRY_LIMIT: usize = 4096;
const SERIALIZATION_FAILURE: &str = r#"{"result":{"type":"undefined"},"exceptionDetails":{"text":"Result could not be serialized"}}"#;

/// One function handle per developer session. It closes over no page values;
/// evaluation scratch records remain in the per-call, promptly released group.
#[derive(Clone, Default)]
pub(in crate::browser::devtools) struct Serializer(Arc<Mutex<Option<String>>>);

impl Serializer {
    pub(in crate::browser::devtools) fn clear(&self) {
        if let Ok(mut cached) = self.0.lock() {
            cached.take();
        }
    }
}

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
        let mut collection_kinds = collection_mask(evaluation.result.subtype.as_deref());
        let serializer = self.serializer_function(deadline)?;
        let mut resume = false;
        drop(response);
        let mut remaining = ENTRY_LIMIT;
        let mut references = 1;
        // Each pass adds native-classified object/prototype references. All
        // page entries visited across passes share one budget. The separate
        // pass/reference caps also bound chains with no enumerable entries.
        for _ in 0..128 {
            let params = json!({
                "objectId": serializer,
                "objectGroup": crate::acp::browser::developer::DEVELOPER_OBJECT_GROUP,
                "functionDeclaration": "function(root,limit,entries,resume,kinds){return this(root,limit,entries,resume,kinds);}",
                "arguments": [
                    {"objectId": receiver},
                    {"value": BROWSER_LIMITS.cdp_result_bytes},
                    {"value": remaining},
                    {"value": resume},
                    {"value": collection_kinds},
                ],
                "returnByValue": false,
                "silent": true,
                // Preserve the original side-effect restriction, and require
                // it even for effectful evaluations. Never retry unguarded.
                "throwOnSideEffect": true,
            })
            .to_string();
            let envelope_limit = 2 * BROWSER_LIMITS.cdp_result_bytes + 4096;
            let response = self.serializer_request(
                "Runtime.callFunctionOn",
                &params,
                envelope_limit,
                deadline,
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
            let pending = self.serializer_request(
                "Runtime.getProperties",
                &params,
                envelope_limit,
                deadline,
            )?;
            if !classify_pending(
                pending,
                &mut references,
                &mut remaining,
                &mut collection_kinds,
            )? {
                return Ok(SERIALIZATION_FAILURE.into());
            }
            resume = true;
        }
        Err(BrowserFailure::Capacity)
    }

    fn serializer_request(
        &self,
        method: &str,
        params: &str,
        max: usize,
        deadline: Instant,
    ) -> Result<String, BrowserFailure> {
        super::super::transport::admit_request(method, params)?;
        let response = self.transport.raw(method, params, max, left(deadline)?)?;
        if response.len() > max {
            return Err(BrowserFailure::Capacity);
        }
        Ok(response)
    }

    fn serializer_function(&self, deadline: Instant) -> Result<String, BrowserFailure> {
        let cached = self
            .serializer
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .clone();
        if let Some(id) = cached {
            // Navigation destroys the context and its handle. Validate only
            // our function, never retry the agent's expression or traversal.
            let params = json!({
                "objectId": id,
                "functionDeclaration": "function(){return true;}",
                "returnByValue": true,
                "throwOnSideEffect": true,
            })
            .to_string();
            match self.serializer_request("Runtime.callFunctionOn", &params, 4096, deadline) {
                Ok(response)
                    if serde_json::from_str::<Value>(&response).is_ok_and(|v| {
                        v["result"]["value"] == true && v["exceptionDetails"].is_null()
                    }) =>
                {
                    return Ok(id);
                }
                Ok(_) | Err(BrowserFailure::Unavailable) => self.serializer.clear(),
                Err(error) => return Err(error),
            }
        }
        // A function expression captures no receiver and publishes no globals.
        // Its separate group lives only as long as the native session.
        let params = json!({
            "objectGroup": "sage-developer-serializer",
            "expression": format!("({})", SCRIPT.trim_start_matches("export default ").trim()),
            "returnByValue": false,
            "silent": true,
            "throwOnSideEffect": true,
        })
        .to_string();
        // CDP's function description includes its source. Bound that envelope
        // separately; retain only the small handle, never the description.
        let response_limit = 2 * super::super::transport::INTERNAL_REQUEST_BYTES + 4096;
        let response =
            self.serializer_request("Runtime.evaluate", &params, response_limit, deadline)?;
        let evaluation: Evaluation<'_> =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        let id = evaluation
            .result
            .object_id
            .filter(|_| evaluation.exception.is_none())
            .ok_or(BrowserFailure::Unavailable)?
            .into_owned();
        if id.len() > 512 {
            return Err(BrowserFailure::Capacity);
        }
        *self
            .serializer
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)? = Some(id.clone());
        Ok(id)
    }
}

fn classify_pending(
    response: String,
    references: &mut usize,
    remaining: &mut usize,
    collection_kinds: &mut u8,
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
    if !(2..=ENTRY_LIMIT as u64 + 1).contains(&length) || properties.len() as u64 != length + 4 {
        return Err(BrowserFailure::Unavailable);
    }
    *remaining = remaining
        .checked_sub(usize::try_from(visited).map_err(|_| BrowserFailure::Capacity)?)
        .ok_or(BrowserFailure::Capacity)?;
    let count = usize::try_from(length - 1).map_err(|_| BrowserFailure::Capacity)?;
    *references += count;
    if *references > ENTRY_LIMIT {
        return Err(BrowserFailure::Capacity);
    }
    // Native proxy admission precedes all page-side brand checks. No
    // per-value labels or page object IDs are echoed back in parameters.
    for property in properties {
        let name = property["name"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        if matches!(name, "0" | "length" | "root" | "previous" | "previousKinds") {
            continue;
        }
        let remote = &property["value"];
        // CDP inspects the internal Proxy brand without running traps. V8's
        // side-effect checker alone can still execute pure Proxy handlers.
        if remote["subtype"] == "proxy" {
            return Ok(false);
        }
        *collection_kinds |= collection_mask(remote["subtype"].as_str());
        remote["objectId"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        name.parse::<usize>()
            .ok()
            .filter(|index| (1..=count).contains(index))
            .ok_or(BrowserFailure::Unavailable)?;
    }
    Ok(true)
}

fn collection_mask(subtype: Option<&str>) -> u8 {
    match subtype {
        Some("map") => 1,
        Some("set") => 2,
        Some("typedarray") => 4,
        _ => 0,
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
    fn pending_references_are_admitted_independently_of_descriptor_order() {
        let response = json!({"result": [
            {"name": "3", "value": {"objectId": "map", "subtype": "map"}},
            {"name": "1", "value": {"objectId": "view", "subtype": "typedarray"}},
            {"name": "2", "value": {"objectId": "set", "subtype": "set"}},
            {"name": "0", "value": {"value": 3}},
            {"name": "length", "value": {"value": 4}},
            {"name": "root", "value": {"objectId": "root"}},
            {"name": "previous", "value": {"type": "object", "subtype": "null"}},
            {"name": "previousKinds", "value": {"value": ""}}
        ]})
        .to_string();
        let mut references = 1;
        let mut collection_kinds = 0;
        let mut remaining = ENTRY_LIMIT;
        assert_eq!(
            classify_pending(
                response,
                &mut references,
                &mut remaining,
                &mut collection_kinds
            ),
            Ok(true)
        );
        assert_eq!(references, 4);
        assert_eq!(collection_kinds, 7);
        assert_eq!(remaining, ENTRY_LIMIT - 3);
    }

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

#[cfg(test)]
#[path = "evaluate_tests.rs"]
mod integration_tests;
