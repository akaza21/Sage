use crate::acp::limits::BROWSER_LIMITS;
use serde_json::{Value, json};

const TOOLS: [&str; 8] = [
    "openTab",
    "navigate",
    "snapshotPage",
    "act",
    "waitFor",
    "screenshot",
    "readConsole",
    "readNetwork",
];

/// The browser tools, with the developer-mode `cdp` tool when `developer`.
pub(super) fn tools(developer: bool) -> Vec<Value> {
    TOOLS.into_iter().chain(developer.then_some("cdp"))
        .map(|name| {
            let scope = json!({"type":"object","description":"Optional. Omit scope or tabId to use this session's attached browser tab. A tabId N denotes canonical tab browser-N.","properties":{
                "tabId":{"type":"integer","minimum":1,"description":"Numeric N from canonical browser-N, such as 2 for browser-2. Omit to use the attached tab."}},
                "additionalProperties":false});
            let (fields, required) = match name {
                "openTab" | "navigate" => (json!({"url":{"type":"string","maxLength":BROWSER_LIMITS.url_bytes}}), vec!["url"]),
                "act" => (json!({"action":action_schema()}), vec!["action"]),
                "waitFor" => (json!({"condition":condition_schema(),"timeoutMs":{"type":"integer","minimum":1,"maximum":BROWSER_LIMITS.wait_timeout_ms}}), vec!["condition","timeoutMs"]),
                "screenshot" => (json!({"region":region_schema()}), vec![]),
                "readConsole" | "readNetwork" => (json!({"cursor":{"type":["integer","null"],"minimum":0},"limit":{"type":"integer","minimum":1,"maximum":BROWSER_LIMITS.diagnostic_entries}}), vec!["limit"]),
                "cdp" => (json!({"method":{"type":"string","enum":super::super::developer::allowed_methods()},"params":{"type":"object"}}), vec!["method"]),
                _ => (json!({}), vec![]),
            };
            let mut properties = fields.as_object().cloned().expect("schema object");
            if name != "openTab" {
                properties.insert("scope".into(), scope);
            }
            let description = if name == "act" {
                "Act on a handle from snapshotPage. Put kind and its fields directly in action; for filling, use action.kind=fill, action.handle, and action.value (not a nested action.fill object)."
            } else if name == "cdp" {
                "Developer mode: send one allowlisted Chrome DevTools Protocol command from the method enum to this session's browser tab. Only listed top-level params are admitted. Scripts run as the signed-in user and can access data available to the page. Approval is required for this tab and site; Stop, Take over or leaving the site ends it. Params are at most 12 KiB of JSON; results over 256 KiB fail."
            } else if name == "openTab" {
                "Open a visible Sage browser tab for this session. Call openTab when no tab is attached, then use the returned tabId for snapshotPage and other browser tools. HTTP(S) URL only; Sage asks permission before opening."
            } else {
                ""
            };
            json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})
        }).collect()
}

fn action_schema() -> Value {
    let handle = json!({"type":"string","maxLength":BROWSER_LIMITS.handle_bytes});
    json!({"description":"Replace the example handle with a handle returned by snapshotPage. Each action is a flat object selected by kind.","examples":[
        {"kind":"click","handle":"HANDLE_FROM_SNAPSHOT"},
        {"kind":"fill","handle":"HANDLE_FROM_SNAPSHOT","value":"Ada"},
        {"kind":"key","handle":"HANDLE_FROM_SNAPSHOT","key":"A"},
        {"kind":"scroll","handle":"HANDLE_FROM_SNAPSHOT","deltaX":0,"deltaY":1}
    ],"oneOf":[
        {"type":"object","properties":{"kind":{"const":"click"},"handle":handle},"required":["kind","handle"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"fill"},"handle":handle,"value":{"type":"string","maxLength":BROWSER_LIMITS.fill_bytes}},"required":["kind","handle","value"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"key"},"handle":handle,"key":{"type":"string","maxLength":BROWSER_LIMITS.key_bytes}},"required":["kind","handle","key"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"scroll"},"handle":handle,"deltaX":{"type":"integer","minimum":-BROWSER_LIMITS.scroll_delta,"maximum":BROWSER_LIMITS.scroll_delta},"deltaY":{"type":"integer","minimum":-BROWSER_LIMITS.scroll_delta,"maximum":BROWSER_LIMITS.scroll_delta}},"required":["kind","handle","deltaX","deltaY"],"additionalProperties":false}
    ]})
}

fn condition_schema() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"kind":{"const":"handle"},"handle":{"type":"string","maxLength":BROWSER_LIMITS.handle_bytes}},"required":["kind","handle"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"text"},"text":{"type":"string","maxLength":BROWSER_LIMITS.wait_text_bytes}},"required":["kind","text"],"additionalProperties":false}
    ]})
}

fn region_schema() -> Value {
    json!({"type":["object","null"],"properties":{
        "x":{"type":"integer","minimum":0},"y":{"type":"integer","minimum":0},
        "width":{"type":"integer","minimum":1,"maximum":BROWSER_LIMITS.screenshot_width},
        "height":{"type":"integer","minimum":1,"maximum":BROWSER_LIMITS.screenshot_height}
    },"required":["x","y","width","height"],"additionalProperties":false})
}
