use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::super::{CdpPort, DevtoolsPort};
use crate::{
    acp::browser::{BrowserFailure, developer::DEVELOPER_OBJECT_GROUP},
    browser::devtools::{
        snapshot::DocumentStamp,
        transport::{CdpTransport, Settled},
    },
};

struct Inspector {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Drop for Inspector {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Page(Mutex<Inspector>);

impl Page {
    fn open() -> Self {
        let mut child = Command::new("node")
            .args([
                "--input-type=module",
                "-e",
                include_str!("evaluate_inspector.mjs"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Node inspector for DevTools regression tests");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self(Mutex::new(Inspector {
            child,
            input,
            output,
        }))
    }
}

impl CdpTransport for Page {
    fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        _: Duration,
    ) -> Result<String, BrowserFailure> {
        // The real native admission runs before a request reaches V8.
        crate::browser::devtools::transport::admit_request(method, params)?;
        let mut page = self.0.lock().unwrap();
        writeln!(
            page.input,
            "{}",
            json!({"method": method, "params": serde_json::from_str::<Value>(params).unwrap()})
        )
        .unwrap();
        page.input.flush().unwrap();
        let mut line = String::new();
        page.output
            .by_ref()
            .take((max_result_bytes + 1024) as u64)
            .read_line(&mut line)
            .unwrap();
        if !line.ends_with('\n') {
            return Err(BrowserFailure::Capacity);
        }
        let mut reply: Value = serde_json::from_str(&line).expect("inspector response");
        if !reply["error"].is_null() {
            return Err(BrowserFailure::Unavailable);
        }
        let result = reply["result"].take().to_string();
        if result.len() > max_result_bytes {
            return Err(BrowserFailure::Capacity);
        }
        Ok(result)
    }

    fn observe(
        &self,
        method: &str,
        params: &str,
        max: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        self.call(method, params, max, timeout)
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        // Mutations within this fixture never navigate the document.
        Ok(DocumentStamp {
            navigation_id: 7,
            generation: 1,
        })
    }

    fn issued(&self) -> bool {
        true
    }

    fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        Ok(None)
    }
}

fn evaluate(port: &mut CdpPort<Page>, expression: &str) -> Result<Value, BrowserFailure> {
    let params = json!({
        "expression": expression,
        "objectGroup": DEVELOPER_OBJECT_GROUP,
        "returnByValue": false,
    })
    .to_string();
    let result = port.raw("Runtime.evaluate", &params);
    port.release_developer_objects();
    result.map(|result| serde_json::from_str::<Value>(&result).unwrap()["result"]["value"].take())
}

#[test]
fn a_thousand_typed_arrays_fit_native_requests() {
    let mut port = CdpPort::new(Page::open());
    let value = evaluate(
        &mut port,
        "Array.from({length:1000}, () => new Uint8Array([42]))",
    )
    .unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1000);
    assert!(
        value
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item == &json!({"0":42}))
    );
}

#[test]
fn deep_wide_collections_fit_native_requests() {
    let mut port = CdpPort::new(Page::open());
    let value = evaluate(&mut port, "(() => { let value = Array.from({length: 1000}, () => [42]); for (let i=0; i<12; i++) value = [value,42]; return value; })()").unwrap();
    let mut nested = &value;
    for _ in 0..12 {
        assert_eq!(nested[1], 42);
        nested = &nested[0];
    }
    assert_eq!(nested.as_array().unwrap().len(), 1000);
    assert!(
        nested
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item == &json!([42]))
    );
}

#[test]
fn a_stale_serializer_is_replaced_without_repeating_the_expression() {
    let mut port = CdpPort::new(Page::open());
    assert_eq!(evaluate(&mut port, "[42]").unwrap(), json!([42]));
    // Destroy just the installed function, as a navigation would. Evaluation
    // groups are independently released by the same entry point every time.
    port.transport
        .call(
            "Runtime.releaseObjectGroup",
            r#"{"objectGroup":"sage-developer-serializer"}"#,
            4096,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        evaluate(
            &mut port,
            "globalThis.evaluations = (globalThis.evaluations || 0) + 1; [42]"
        )
        .unwrap(),
        json!([42])
    );
    assert_eq!(evaluate(&mut port, "evaluations").unwrap(), json!(1));
    assert_eq!(evaluate(&mut port, "[43]").unwrap(), json!([43]));
}

#[test]
fn page_function_call_overrides_cannot_replace_the_serializer() {
    let mut port = CdpPort::new(Page::open());
    evaluate(
        &mut port,
        r#"
        globalThis.originalCall = Function.prototype.call;
        Function.prototype.call = function(...args) {
            if (this.name === 'serializeDeveloperResult') return args[0];
            return Reflect.apply(originalCall, this, args);
        };
        true
    "#,
    )
    .unwrap();
    assert_eq!(evaluate(&mut port, "[42]").unwrap(), json!([42]));
}

#[test]
fn plain_objects_fail_closed_and_selected_paths_remain_available() {
    let mut port = CdpPort::new(Page::open());
    evaluate(
        &mut port,
        "globalThis.largeRecord = {}; for (let i=0; i<5000000; i++) largeRecord[i]=i; true",
    )
    .unwrap();
    let marker = json!({
        "kind": "object", "preview": false, "reason": "not-inspectable-within-budget"
    });
    let started = Instant::now();
    assert_eq!(evaluate(&mut port, "largeRecord").unwrap(), marker);
    assert!(started.elapsed() < Duration::from_millis(750));
    assert_eq!(evaluate(&mut port, "({answer:42})").unwrap(), marker);
    assert_eq!(
        evaluate(&mut port, "[largeRecord[42],largeRecord[4999999]]").unwrap(),
        json!([42, 4999999])
    );
    assert_eq!(
        evaluate(&mut port, "[largeRecord]").unwrap(),
        json!([marker])
    );
}

#[test]
fn a_capacity_rejected_mutating_evaluate_invalidates_snapshot_handles() {
    use crate::acp::browser::{
        BrowserAction, BrowserCapability, BrowserCommand, BrowserRequest, BrowserResult,
        BrowserScope,
    };
    use crate::browser::devtools::capability::DevtoolsCapability;

    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut port = CdpPort::new(Page::open());
    evaluate(
        &mut port,
        "globalThis.button = {textContent: 'Continue'}; true",
    )
    .unwrap();
    let mut capability = DevtoolsCapability::new(scope.clone(), port);
    let request = |command| BrowserRequest {
        scope: scope.clone(),
        command,
    };
    let BrowserResult::SnapshotPage {
        nodes,
        navigation_epoch,
        ..
    } = capability
        .perform(&request(BrowserCommand::SnapshotPage {}))
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    assert_eq!(nodes[0].name, "Continue");
    assert_eq!(capability.perform(&request(BrowserCommand::Cdp {
        method: "Runtime.evaluate".into(),
        params: json!({"expression": "button.textContent = 'Delete account'; new Array(5000).fill(42)"})
            .as_object().cloned().unwrap(),
    })), Err(BrowserFailure::Capacity));
    assert_eq!(
        capability.perform(&request(BrowserCommand::Act {
            action: BrowserAction::Click {
                handle: nodes[0].handle.clone()
            },
        })),
        Err(BrowserFailure::StaleHandle)
    );
    let BrowserResult::SnapshotPage {
        nodes,
        navigation_epoch: after,
        ..
    } = capability
        .perform(&request(BrowserCommand::SnapshotPage {}))
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    assert_eq!(nodes[0].name, "Delete account");
    assert_eq!(after, navigation_epoch);
}
