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
fn tab_scoped_commands_pass_including_script() {
    for method in [
        "Runtime.evaluate",
        "Page.navigate",
        "DOM.querySelector",
        "Network.enable",
        "Profiler.start",
        "Security.enable",
    ] {
        assert_eq!(
            admit(method, json!({"expression": "1"})),
            Ok(()),
            "{method}"
        );
    }
}

#[test]
fn domains_and_commands_that_leave_the_tab_are_blocked() {
    for method in [
        "Target.createTarget",
        "Target.attachToTarget",
        "Browser.setDownloadBehavior",
        "Browser.grantPermissions",
        "Storage.getCookies",
        "Storage.clearDataForOrigin",
        "SystemInfo.getProcessInfo",
        "Tethering.bind",
        "Autofill.trigger",
        "Extensions.loadUnpacked",
        "FedCm.selectAccount",
        "PWA.install",
        "ServiceWorker.stopAllWorkers",
        "BackgroundService.startObserving",
        "Tracing.start",
        "Network.getAllCookies",
        "Network.clearBrowserCookies",
        "Network.clearBrowserCache",
        "Page.setDownloadBehavior",
        "Security.setIgnoreCertificateErrors",
        "Security.handleCertificateError",
        "Security.setOverrideCertificateErrors",
        // Local files.
        "DOM.setFileInputFiles",
        "Input.dispatchDragEvent",
        "Page.setInterceptFileChooserDialog",
        "Page.handleFileChooser",
        "IO.read",
        // Fetching with the user's credentials outside the page.
        "Network.loadNetworkResource",
        // State that would outlive the grant on the shared DevTools session.
        "Page.addScriptToEvaluateOnNewDocument",
        "Page.addScriptToEvaluateOnLoad",
        "Page.setBypassCSP",
        "Runtime.addBinding",
        "Fetch.enable",
        "Debugger.enable",
        "Debugger.setBreakpointByUrl",
        "DOMDebugger.setEventListenerBreakpoint",
        "Emulation.setUserAgentOverride",
        "Network.setExtraHTTPHeaders",
        "Network.setBlockedURLs",
        "Network.setUserAgentOverride",
        "Input.setIgnoreInputEvents",
        "WebAuthn.addVirtualAuthenticator",
        // Other frames, the tab itself, the whole browser.
        "DOMSnapshot.captureSnapshot",
        "Page.captureSnapshot",
        "Page.close",
        "Page.crash",
        "Network.setCookieControls",
        "Memory.simulatePressureNotification",
        "Cast.enable",
    ] {
        assert_eq!(
            admit(method, json!({})),
            Err(BrowserFailure::Denied),
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
fn storage_partitions_must_be_the_granted_origin() {
    assert_eq!(
        admit(
            "IndexedDB.requestDatabaseNames",
            json!({"securityOrigin": "https://app.example.com"})
        ),
        Ok(())
    );
    assert_eq!(
        admit(
            "DOMStorage.getDOMStorageItems",
            json!({"storageId": {"storageKey": "https://app.example.com/^0https://x.example", "isLocalStorage": true}})
        ),
        Ok(())
    );
    for (method, value) in [
        (
            "IndexedDB.requestDatabaseNames",
            json!({"securityOrigin": "https://bank.example"}),
        ),
        (
            "DOMStorage.getDOMStorageItems",
            json!({"storageId": {"securityOrigin": "https://bank.example", "isLocalStorage": true}}),
        ),
        (
            "CacheStorage.requestCacheNames",
            json!({"storageBucket": {"storageKey": "https://bank.example/"}}),
        ),
        ("IndexedDB.requestData", json!({"storageKey": 3})),
    ] {
        assert_eq!(
            admit(method, value.clone()),
            Err(BrowserFailure::Denied),
            "{value}"
        );
    }
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
fn commands_stay_in_the_main_frame_and_its_default_context() {
    assert_eq!(admit("DOM.getDocument", json!({"depth": 2})), Ok(()));
    assert_eq!(admit("DOM.getDocument", json!({"pierce": false})), Ok(()));
    for (method, value) in [
        ("DOM.getDocument", json!({"pierce": true})),
        ("DOM.describeNode", json!({"nodeId": 1, "pierce": true})),
        (
            "Runtime.evaluate",
            json!({"expression": "1", "contextId": 3}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "1", "uniqueContextId": "x"}),
        ),
        (
            "Runtime.callFunctionOn",
            json!({"functionDeclaration": "f", "executionContextId": 3}),
        ),
        (
            "Page.navigate",
            json!({"url": "file:///etc/passwd", "frameId": "F2"}),
        ),
        (
            "Page.getResourceContent",
            json!({"frameId": "F2", "url": "https://x"}),
        ),
        ("Page.createIsolatedWorld", json!({"frameId": "F2"})),
        (
            "CacheStorage.requestEntries",
            json!({"cacheId": "https://bank.example/|c"}),
        ),
        (
            "Network.continueInterceptedRequest",
            json!({"nested": {"files": ["/x"]}}),
        ),
    ] {
        assert_eq!(
            admit(method, value.clone()),
            Err(BrowserFailure::Denied),
            "{method} {value}"
        );
    }
}
