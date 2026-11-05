//! Real V8 results through the app's command layer and native request cap.
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
    time::Duration,
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
        Err(BrowserFailure::Unavailable)
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
fn deep_wide_results_fit_native_requests() {
    let mut port = CdpPort::new(Page::open());
    let value = evaluate(&mut port, "(() => { let value = Array.from({length: 1000}, () => ({answer:42})); for (let i=0; i<12; i++) value = {next:value, answer:42}; return value; })()").unwrap();
    let mut nested = &value;
    for _ in 0..12 {
        assert_eq!(nested["answer"], 42);
        nested = &nested["next"];
    }
    assert_eq!(nested.as_array().unwrap().len(), 1000);
    assert!(
        nested
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item == &json!({"answer":42}))
    );
}

#[test]
fn a_stale_serializer_is_replaced_without_repeating_the_expression() {
    let mut port = CdpPort::new(Page::open());
    assert_eq!(
        evaluate(&mut port, "({answer:42})").unwrap(),
        json!({"answer":42})
    );
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
            "globalThis.evaluations = (globalThis.evaluations || 0) + 1; ({answer:42})"
        )
        .unwrap(),
        json!({"answer":42})
    );
    assert_eq!(evaluate(&mut port, "evaluations").unwrap(), json!(1));
    // Reuse across a successful per-evaluation group release as well.
    assert_eq!(
        evaluate(&mut port, "({answer:43})").unwrap(),
        json!({"answer":43})
    );
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
    assert_eq!(
        evaluate(&mut port, "({answer:42})").unwrap(),
        json!({"answer":42})
    );
}
