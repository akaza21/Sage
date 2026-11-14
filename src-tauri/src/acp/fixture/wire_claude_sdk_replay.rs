use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{Read, Write},
    path::Path,
    rc::Rc,
};

use serde_json::Value;

use super::WireState;

/// Replays `case_id`, launched with Claude Code's session flags
/// `harness_args` in profile `config` and working in `cwd`, against
/// `client`. The client is called with each awaited step's kind and the
/// frames the child wrote since its last call, and returns the frames it
/// sends. When a step reads past what the client sent, the client is called
/// again with no kind and the newer frames, so it can answer them, as a live
/// client answers a control request mid-step. The final call (no kind)
/// receives the remaining output and must send nothing more.
pub(crate) fn claude_sdk_wire_for_test(
    case_id: &str,
    harness_args: &[std::ffi::OsString],
    config: &Path,
    cwd: &Path,
    client: impl FnMut(Option<&str>, Vec<Value>) -> Vec<Value>,
) {
    let args: Vec<String> = harness_args
        .iter()
        .map(|arg| arg.to_str().expect("UTF-8 argument").to_owned())
        .collect();
    let mut state = WireState::new();
    state.claude_sdk = super::claude_sdk::SdkWireState::opened(&args, config, cwd)
        .unwrap_or_else(|error| panic!("{case_id}: {args:?} opens a session: {error:?}"));
    replay(case_id, state, client);
}

fn replay(
    case_id: &str,
    mut state: WireState,
    client: impl FnMut(Option<&str>, Vec<Value>) -> Vec<Value>,
) {
    let case = crate::acp_protocol::native_cases::JOURNEY_CASES
        .iter()
        .find(|case| case.id == case_id)
        .expect("SDK journey exists");
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).expect("journey steps");
    let output = Output::default();
    let mut pipe = Pipe {
        client,
        kind: None,
        input: VecDeque::new(),
        output: output.clone(),
    };
    let mut writer = output.clone();
    for step in &steps {
        pipe.kind = (step["op"] == "awaitClient")
            .then(|| step["kind"].as_str().expect("awaited kind").to_owned());
        super::execute_with_io(step, &mut state, &mut pipe, &mut writer, &mut Vec::new())
            .unwrap_or_else(|error| panic!("{case_id}: {step} replays: {error:?}"));
        assert!(
            pipe.input.is_empty(),
            "{case_id}: {step} read all it was sent"
        );
    }
    let rest = output.drain();
    let unserved = (pipe.client)(None, rest);
    assert!(unserved.is_empty(), "{case_id}: no unserved requests");
}

#[derive(Clone, Default)]
struct Output(Rc<RefCell<Vec<u8>>>);

impl Output {
    fn drain(&self) -> Vec<Value> {
        String::from_utf8(std::mem::take(&mut *self.0.borrow_mut()))
            .expect("utf-8 output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON frame"))
            .collect()
    }
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Pipe<F> {
    client: F,
    kind: Option<String>,
    input: VecDeque<u8>,
    output: Output,
}

impl<F: FnMut(Option<&str>, Vec<Value>) -> Vec<Value>> Read for Pipe<F> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.input.is_empty() {
            let kind = self.kind.take();
            for frame in (self.client)(kind.as_deref(), self.output.drain()) {
                self.input.extend(format!("{frame}\n").into_bytes());
            }
        }
        self.input.read(buffer)
    }
}
