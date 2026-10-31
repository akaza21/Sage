use serde_json::{Map, Value, json};

use super::*;

const ORIGIN: &str = "https://app.example.com";

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn admit(method: &str, value: Value) -> Result<(), BrowserFailure> {
    admit_command(method, &params(value), ORIGIN)
}

#[test]
fn method_shape_is_domain_dot_command() {
    for method in [
        "Runtime.evaluate",
        "DOM.getDocument",
        "CSS.getComputedStyleForNode",
    ] {
        assert_eq!(admit_method_shape(method), Ok(()), "{method}");
    }
    for method in [
        "",
        "Runtime",
        ".evaluate",
        "Runtime.",
        "Runtime.evaluate.x",
        "Run time.evaluate",
        "Runtime.évaluer",
        &format!("A.{}", "b".repeat(127)),
    ] {
        assert_eq!(
            admit_method_shape(method),
            Err(BrowserFailure::InvalidRequest),
            "{method}"
        );
    }
}

#[test]
fn cookie_reads_must_name_only_the_granted_origin() {
    assert_eq!(
        admit(
            "Network.getCookies",
            json!({"urls": ["https://app.example.com/a"]})
        ),
        Ok(())
    );
    for value in [
        json!({}),
        json!({"urls": []}),
        json!({"urls": ["https://bank.example"]}),
        json!({"urls": ["https://app.example.com/", "https://evil.example"]}),
        json!({"urls": ["http://app.example.com/"]}),
        json!({"urls": [7]}),
    ] {
        assert_eq!(
            admit("Network.getCookies", value.clone()),
            Err(BrowserFailure::Denied),
            "{value}"
        );
    }
}

#[test]
fn cookie_writes_stay_on_the_granted_host() {
    for method in ["Network.setCookie", "Network.deleteCookies"] {
        assert_eq!(
            admit(
                method,
                json!({"name": "a", "url": "https://app.example.com/"})
            ),
            Ok(())
        );
        assert_eq!(
            admit(
                method,
                json!({"name": "a", "url": "https://app.example.com/", "domain": "app.example.com"})
            ),
            Ok(())
        );
        for value in [
            json!({"name": "a"}),
            json!({"name": "a", "url": "https://other.example/"}),
            json!({"name": "a", "url": "https://app.example.com/", "domain": ".example.com"}),
            json!({"name": "a", "url": "https://app.example.com/", "domain": "example.com"}),
        ] {
            assert_eq!(
                admit(method, value.clone()),
                Err(BrowserFailure::Denied),
                "{value}"
            );
        }
    }
    assert_eq!(
        admit(
            "Network.setCookies",
            json!({"cookies": [{"name": "a", "value": "1", "url": "https://app.example.com/"}]})
        ),
        Ok(())
    );
    assert_eq!(
        admit(
            "Network.setCookies",
            json!({"cookies": [
                {"name": "a", "value": "1", "url": "https://app.example.com/"},
                {"name": "b", "value": "1", "domain": "bank.example"}
            ]})
        ),
        Err(BrowserFailure::Denied)
    );
}

#[test]
fn audit_outcomes_have_stable_names() {
    assert_eq!(CdpAuditOutcome::Ok.as_str(), "ok");
    assert_eq!(CdpAuditOutcome::Blocked.as_str(), "blocked");
    assert_eq!(CdpAuditOutcome::Denied.as_str(), "denied");
    assert_eq!(
        CdpAuditOutcome::Failed(BrowserFailure::Capacity).as_str(),
        "capacity"
    );
    assert_eq!(
        CdpAuditOutcome::Failed(BrowserFailure::OutcomeUnknown).as_str(),
        "outcomeUnknown"
    );
    assert_eq!(
        CdpAuditOutcome::Failed(BrowserFailure::StaleHandle).as_str(),
        "unavailable"
    );
}

#[test]
fn listed_main_frame_commands_pass_with_their_own_parameters() {
    for (method, value) in [
        (
            "Runtime.evaluate",
            json!({"expression": "1 + 1", "returnByValue": true}),
        ),
        (
            "Runtime.callFunctionOn",
            json!({"functionDeclaration": "function () {}", "objectId": "1"}),
        ),
        ("DOM.getDocument", json!({"depth": -1})),
        ("DOM.querySelector", json!({"nodeId": 1, "selector": "h1"})),
        ("DOM.getOuterHTML", json!({"nodeId": 4})),
        (
            "DOM.setAttributeValue",
            json!({"nodeId": 4, "name": "class", "value": "x"}),
        ),
        (
            "Page.navigate",
            json!({"url": "https://app.example.com/next"}),
        ),
        ("Page.reload", json!({"ignoreCache": true})),
        ("Page.captureScreenshot", json!({"format": "png"})),
        ("Page.getFrameTree", json!({})),
    ] {
        assert_eq!(admit(method, value.clone()), Ok(()), "{method} {value}");
    }
}

#[test]
fn any_method_not_listed_is_refused() {
    for method in [
        // Leaving the tab or the profile.
        "Target.createTarget",
        "Browser.grantPermissions",
        "Storage.getCookies",
        "Network.getAllCookies",
        "Page.close",
        // Local files and credentialed fetches.
        "DOM.setFileInputFiles",
        "Input.dispatchDragEvent",
        "Page.handleFileChooser",
        "IO.read",
        "Network.loadNetworkResource",
        // State that outlives the grant on the shared DevTools session.
        "Page.addScriptToEvaluateOnNewDocument",
        "Page.setBypassCSP",
        "Page.setGeolocationOverride",
        "Page.setDeviceMetricsOverride",
        "Page.setTouchEmulationEnabled",
        "Runtime.addBinding",
        "Fetch.enable",
        "Debugger.enable",
        "Emulation.setUserAgentOverride",
        "Network.setExtraHTTPHeaders",
        "Network.enable",
        "Runtime.enable",
        "WebAuthn.addVirtualAuthenticator",
        // Input can land in any frame, cross-origin iframes included.
        "Input.dispatchMouseEvent",
        "Input.dispatchKeyEvent",
        "Input.insertText",
        // Other frames.
        "Page.createIsolatedWorld",
        "Page.getResourceContent",
        "DOMSnapshot.captureSnapshot",
        "CacheStorage.requestEntries",
        "IndexedDB.requestData",
    ] {
        assert_eq!(
            admit(method, json!({})),
            Err(BrowserFailure::Denied),
            "{method}"
        );
    }
}

#[test]
fn a_listed_method_refuses_any_parameter_it_does_not_list() {
    for (method, value) in [
        // Injected into every frame after the reload, cross-origin ones too.
        ("Page.reload", json!({"scriptToEvaluateOnLoad": "steal()"})),
        (
            "Page.navigate",
            json!({"url": "file:///etc/passwd", "frameId": "F2"}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "1", "contextId": 3}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "1", "uniqueContextId": "x"}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "debug(f)", "includeCommandLineAPI": true}),
        ),
        (
            "Runtime.callFunctionOn",
            json!({"functionDeclaration": "f", "executionContextId": 3}),
        ),
        ("DOM.getDocument", json!({"pierce": true})),
        ("DOM.describeNode", json!({"nodeId": 1, "pierce": true})),
        ("DOM.getOuterHTML", json!({"backendNodeId": 99})),
        (
            "Network.setCookie",
            json!({"name": "a", "url": "https://app.example.com/", "partitionKey": {"topLevelSite": "https://bank.example"}}),
        ),
    ] {
        assert_eq!(
            admit(method, value.clone()),
            Err(BrowserFailure::Denied),
            "{method} {value}"
        );
    }
    assert_eq!(
        admit(
            "Network.setCookies",
            json!({"cookies": [{"name": "a", "url": "https://app.example.com/", "partitionKey": "x"}]})
        ),
        Err(BrowserFailure::Denied)
    );
}
