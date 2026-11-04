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
            json!({"expression": "document.title", "awaitPromise": false}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "fetch('/a')", "awaitPromise": true}),
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
        // Node ids reach same-process iframe documents, and DOM enables
        // itself, leaving tracking on after the grant.
        "DOM.getDocument",
        "DOM.querySelector",
        "DOM.getOuterHTML",
        "DOM.setOuterHTML",
        "DOM.resolveNode",
        // HttpOnly session tokens page script cannot read.
        "Network.getCookies",
        "Network.setCookie",
        "Network.setCookies",
        "Network.deleteCookies",
        // A remote objectId names its execution context, so a hand-built
        // one could reach an iframe's.
        "Runtime.callFunctionOn",
        "Runtime.getProperties",
        "Runtime.awaitPromise",
        "Runtime.releaseObject",
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
            "Page.captureScreenshot",
            json!({"format": "png", "frameId": "F2"}),
        ),
        // Results stay by value in Sage's own group, released every call.
        (
            "Runtime.evaluate",
            json!({"expression": "new ArrayBuffer(1e8)", "returnByValue": false}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "x", "serializationOptions": {"serialization": "idOnly"}}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "x", "generatePreview": true}),
        ),
        (
            "Runtime.evaluate",
            json!({"expression": "x", "objectGroup": "mine"}),
        ),
        (
            "Runtime.releaseObjectGroup",
            json!({"objectGroup": "sage-developer-mode"}),
        ),
    ] {
        assert_eq!(
            admit(method, value.clone()),
            Err(BrowserFailure::Denied),
            "{method} {value}"
        );
    }
}

#[test]
fn only_an_evaluation_in_sages_group_releases_when_it_completes() {
    let ours = json!({"expression": "1", "objectGroup": DEVELOPER_OBJECT_GROUP}).to_string();
    let other = json!({"expression": "1", "objectGroup": "page"}).to_string();
    assert!(releases_on_completion("Runtime.evaluate", &ours));
    assert!(!releases_on_completion("Runtime.evaluate", &other));
    assert!(!releases_on_completion("Runtime.evaluate", "{}"));
    assert!(!releases_on_completion("Page.reload", &ours));
    let release: Value = serde_json::from_str(&release_params()).unwrap();
    assert_eq!(release, json!({"objectGroup": DEVELOPER_OBJECT_GROUP}));
}

#[test]
fn screenshot_rejects_bitmap_allocations_above_the_pixel_budget() {
    for (width, height, scale) in [
        (16000.0, 16000.0, 1.0),
        (1920.0, 1081.0, 1.0),
        (3840.0, 2162.0, 0.5),
        (1920.1, 1080.0, 1.0),
    ] {
        assert_eq!(
            admit(
                "Page.captureScreenshot",
                json!({
                    "clip": {"x": 0, "y": 0, "width": width, "height": height, "scale": scale},
                    "captureBeyondViewport": true
                })
            ),
            Err(BrowserFailure::Capacity),
            "{width} x {height} at {scale}"
        );
    }
}

#[test]
fn screenshot_accepts_bounded_clips_including_downscaling_and_portrait() {
    for (width, height, scale) in [
        (1920.0, 1080.0, 1.0),
        (1080.0, 1920.0, 1.0),
        (1440.0, 1440.0, 1.0),
        (3840.0, 2160.0, 0.5),
        (16000.0, 16000.0, 0.01),
    ] {
        for beyond in [false, true] {
            assert_eq!(
                admit(
                    "Page.captureScreenshot",
                    json!({
                        "clip": {"x": 0, "y": 0, "width": width, "height": height, "scale": scale},
                        "captureBeyondViewport": beyond, "fromSurface": true
                    })
                ),
                Ok(()),
                "{width} x {height} at {scale}, beyond={beyond}"
            );
        }
    }
}

#[test]
fn screenshot_rejects_invalid_geometry_scales_and_capture_flags() {
    let valid_clip = json!({"x": 0, "y": 0, "width": 640, "height": 480, "scale": 1});
    for (field, value, expected) in [
        ("width", json!(0), BrowserFailure::InvalidRequest),
        ("height", json!(-1), BrowserFailure::InvalidRequest),
        ("width", json!("640"), BrowserFailure::InvalidRequest),
        ("x", Value::Null, BrowserFailure::InvalidRequest),
        ("scale", json!(0), BrowserFailure::InvalidRequest),
        ("scale", json!(-1), BrowserFailure::InvalidRequest),
        ("scale", json!(1e-300), BrowserFailure::Capacity),
        ("scale", json!(0.00999), BrowserFailure::Capacity),
        ("scale", json!(1.001), BrowserFailure::Capacity),
        ("scale", json!(1e300), BrowserFailure::Capacity),
        ("width", json!(1e300), BrowserFailure::Capacity),
        ("extra", json!(1), BrowserFailure::InvalidRequest),
    ] {
        let mut clip = valid_clip.clone();
        clip[field] = value;
        assert_eq!(
            admit("Page.captureScreenshot", json!({"clip": clip})),
            Err(expected),
            "{clip}"
        );
    }
    for clip in [
        Value::Null,
        json!([]),
        json!({}),
        json!({"width": 640, "height": 480}),
    ] {
        assert_eq!(
            admit("Page.captureScreenshot", json!({"clip": clip})),
            Err(BrowserFailure::InvalidRequest)
        );
    }
    for key in ["fromSurface", "captureBeyondViewport"] {
        for value in [Value::Null, json!(0), json!("true")] {
            assert_eq!(
                admit("Page.captureScreenshot", json!({key: value})),
                Err(BrowserFailure::InvalidRequest)
            );
        }
    }
    assert_eq!(
        admit(
            "Page.captureScreenshot",
            json!({"fromSurface": false, "clip": valid_clip})
        ),
        Err(BrowserFailure::Denied)
    );
}

#[test]
fn unbounded_native_document_serializers_remain_denied() {
    for (method, value) in [
        (
            "Page.printToPDF",
            json!({"paperWidth": 16000, "paperHeight": 16000, "scale": 2}),
        ),
        ("Page.captureSnapshot", json!({"format": "mhtml"})),
        (
            "DOMSnapshot.captureSnapshot",
            json!({"computedStyles": ["display"], "includePaintOrder": true}),
        ),
        (
            "DOMSnapshot.getSnapshot",
            json!({"computedStyleWhitelist": ["display"]}),
        ),
        ("DOM.getDocument", json!({"depth": -1, "pierce": true})),
    ] {
        assert_eq!(
            admit(method, value),
            Err(BrowserFailure::Denied),
            "{method}"
        );
    }
}
