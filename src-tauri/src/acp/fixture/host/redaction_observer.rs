use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tracing_subscriber::{Layer, fmt::MakeWriter, layer::SubscriberExt};
use uuid::Uuid;

use crate::acp::{
    contract::{RuntimeEpoch, SafeText, SageChatId, SageProjectId},
    fixture::generated_cases::FrozenCase,
    intent::{ExternalId, NormalizedPayload},
    lifecycle::ItemLifecycle,
    limits::LIMITS,
    redaction::{DiagnosticCode, SafeDiagnostic, SensitiveBytes, StderrRing},
    store::{ItemIngressCommand, apply_item_ingress, load_chat_page},
};

use super::store_bounds::{digest, item_fixture};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &["redaction-canaries-all-sinks"];

const CANARY_COUNT: usize = 25;
const CANARIES_PER_SINK: usize = 5;
const NODE_OUTPUT_BYTES: u64 = 64 * 1024;
const NODE_TIMEOUT: Duration = Duration::from_secs(10);
const STRUCTURED_LOG_BYTES: usize = 64 * 1024;

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    assert_eq!(case.id, CASE_IDS[0]);
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [step] = steps.as_slice() else {
        panic!("expected one redaction exercise");
    };
    assert_eq!(step["op"], "exerciseRedactionSinks");
    let canaries = step["canaries"].as_array().unwrap();
    assert_eq!(canaries.len(), CANARY_COUNT);

    let fixture = item_fixture();
    let mut connection = fixture.database.connection();
    let mut stderr = StderrRing::new(
        LIMITS.process.stderr_ring_bytes as usize,
        LIMITS.process.stderr_line_bytes as usize,
    );
    let mut typed_redactions = Vec::with_capacity(canaries.len());
    let mut structured = Vec::new();

    for (index, value) in canaries.iter().enumerate() {
        let canary = value.as_str().unwrap();
        let (class, sink) = parse_canary(canary);
        let sensitive = SensitiveBytes::new(canary.as_bytes().to_vec());
        if sink == "stderrRing" {
            stderr.ingest(sensitive.expose_to_normalizer());
        }
        let diagnostic = sensitive.redact(diagnostic_code(class));
        match sink {
            "sqlite" | "projection" => persist_diagnostic(&mut connection, index, sink, diagnostic),
            "structuredLog" => structured.push(diagnostic),
            "stderrRing" | "curatedEvidence" => {}
            _ => panic!("unknown redaction sink"),
        }
        typed_redactions.push(json!({
            "diagnostic": diagnostic,
            "source": {"caseIndex": index, "sink": sink},
        }));
    }

    let sqlite = read_sqlite(&connection);
    assert_eq!(
        sqlite["items"].as_array().unwrap().len(),
        CANARIES_PER_SINK * 2
    );
    let projection = load_chat_page(
        &connection,
        SageProjectId::parse("project-a").unwrap(),
        SageChatId::parse("chat-a").unwrap(),
        None,
    )
    .unwrap();
    let projected_items = projection
        .turns
        .as_slice()
        .iter()
        .map(|turn| turn.items.as_slice().len())
        .sum::<usize>();
    assert_eq!(projected_items, CANARIES_PER_SINK * 2);
    let stderr = stderr.safe_summary();
    assert!(stderr.observed_bytes > 0);
    let observation = json!({
        "sqlite": sqlite,
        "projection": projection,
        "structuredLog": capture_structured_logs(&structured),
        "stderr": stderr,
        "typedRedactions": typed_redactions,
    });
    drop(connection);
    drop(fixture);

    observe_curated_evidence(&observation)
}

fn parse_canary(canary: &str) -> (&str, &str) {
    let mut parts = canary.split('.');
    let class = parts.next().unwrap();
    let sink = parts.next().unwrap();
    assert!(parts.next().is_some());
    assert!(parts.next().is_none());
    (class, sink)
}

fn diagnostic_code(class: &str) -> DiagnosticCode {
    match class {
        "secret" => DiagnosticCode::IdentityMismatch,
        "absolutePath" | "commandLine" | "environment" => DiagnosticCode::InvalidLaunchSpec,
        "rawFrame" => DiagnosticCode::ProtocolFailure,
        _ => panic!("unknown sensitive class"),
    }
}

fn persist_diagnostic(
    connection: &mut rusqlite::Connection,
    index: usize,
    sink: &str,
    diagnostic: SafeDiagnostic,
) {
    let message = serde_json::to_string(&diagnostic).unwrap();
    let payload = NormalizedPayload::Error {
        code: SafeText::new("errorCode", "sensitiveInputRedacted").unwrap(),
        message: SafeText::new("errorMessage", message).unwrap(),
        retryable: false,
    };
    let encoded = serde_json::to_vec(&payload).unwrap();
    apply_item_ingress(
        connection,
        ItemIngressCommand {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
            turn_id: crate::acp::contract::SageTurnId::parse("turn-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            external_item_id: ExternalId::new("item", format!("redaction-{sink}-{index}")).unwrap(),
            activity_id: None,
            payload,
            payload_digest: digest(&encoded),
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            now_millis: index as i64 + 10,
        },
    )
    .unwrap();
}

fn read_sqlite(connection: &rusqlite::Connection) -> Value {
    let item_limit = i64::try_from(CANARY_COUNT + 1).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT external_item_id,payload_json FROM items
             WHERE external_item_id LIKE 'redaction-%' ORDER BY ordinal LIMIT ?1",
        )
        .unwrap();
    let items = statement
        .query_map([item_limit], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "payload": row.get::<_, String>(1)?,
            }))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(items.len() <= CANARY_COUNT);
    let fact_limit = i64::try_from(LIMITS.durable.facts_per_session.saturating_add(1)).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT kind,payload_json FROM session_facts
             WHERE turn_id='turn-a' ORDER BY sequence LIMIT ?1",
        )
        .unwrap();
    let facts = statement
        .query_map([fact_limit], |row| {
            Ok(json!({
                "kind": row.get::<_, String>(0)?,
                "payload": row.get::<_, String>(1)?,
            }))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(u64::try_from(facts.len()).unwrap() <= LIMITS.durable.facts_per_session);
    json!({"items": items, "facts": facts})
}

fn capture_structured_logs(diagnostics: &[SafeDiagnostic]) -> String {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let sage_targets = tracing_subscriber::filter::filter_fn(crate::is_sage_target);
    let formatter = tracing_subscriber::fmt::layer()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_writer(Capture(Arc::clone(&bytes)))
        .with_filter(sage_targets);
    let subscriber = tracing_subscriber::registry().with(formatter);
    tracing::subscriber::with_default(subscriber, || {
        for diagnostic in diagnostics {
            tracing::info!(?diagnostic, "ACP sensitive input redacted");
        }
        tracing::info!(target: "unrelated_dependency", "must be filtered");
    });
    let captured = bytes.lock().unwrap().clone();
    let captured = String::from_utf8(captured).unwrap();
    assert!(!captured.contains("must be filtered"));
    assert_eq!(
        captured.matches("ACP sensitive input redacted").count(),
        diagnostics.len()
    );
    captured
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;

    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(Arc::clone(&self.0))
    }
}

impl Write for CaptureWriter {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        let mut bytes = self.0.lock().unwrap();
        if bytes.len().saturating_add(input.len()) > STRUCTURED_LOG_BYTES {
            return Err(std::io::Error::other("structured log capture limit"));
        }
        bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn observe_curated_evidence(observation: &Value) -> Value {
    let encoded = serde_json::to_vec(observation).unwrap();
    assert!(encoded.len() <= LIMITS.fixture.curated_evidence_bytes as usize);
    let evidence = TemporaryEvidence::create(&encoded);
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let script = workspace.join("scripts/acp-redaction-curated-observer.mjs");
    let child = Command::new("node")
        .arg(script)
        .arg("--input")
        .arg(&evidence.0)
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard {
        child,
        completed: false,
    };
    let stdout = read_node_stream(child.child.stdout.take().unwrap());
    let stderr = read_node_stream(child.child.stderr.take().unwrap());
    let deadline = Instant::now() + NODE_TIMEOUT;
    let status = child.wait_until(deadline);
    let output = receive_node_stream(stdout, deadline, "stdout");
    let stderr = receive_node_stream(stderr, deadline, "stderr");
    let stderr_overflow = stderr.len() as u64 > NODE_OUTPUT_BYTES;
    let stderr = SensitiveBytes::new(stderr).redact(DiagnosticCode::ProtocolFailure);
    assert!(
        !stderr_overflow,
        "curated observer stderr exceeded its bound"
    );
    assert!(status.success(), "curated observer failed: {stderr:?}");
    if output.len() as u64 > NODE_OUTPUT_BYTES {
        panic!("curated observer output exceeded its bound");
    }
    serde_json::from_slice(&output).unwrap()
}

fn read_node_stream<R>(reader: R) -> mpsc::Receiver<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = reader
            .take(NODE_OUTPUT_BYTES + 1)
            .read_to_end(&mut output)
            .map(|_| output);
        let _ = sender.send(result);
    });
    receiver
}

fn receive_node_stream(
    receiver: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    deadline: Instant,
    stream: &str,
) -> Vec<u8> {
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .unwrap_or_else(|_| panic!("curated observer {stream} did not close"))
        .unwrap_or_else(|_| panic!("curated observer {stream} could not be read"))
}

struct ChildGuard {
    child: Child,
    completed: bool,
}

impl ChildGuard {
    fn wait_until(&mut self, deadline: Instant) -> ExitStatus {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.completed = true;
                    return status;
                }
                Ok(None) => {}
                Err(_) => panic!("curated observer status unavailable"),
            }
            assert!(
                Instant::now() < deadline,
                "curated observer exceeded its time limit"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct TemporaryEvidence(PathBuf);

impl TemporaryEvidence {
    fn create(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!("sage-redaction-{}.json", Uuid::new_v4()));
        let evidence = Self(path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&evidence.0)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        evidence
    }
}

impl Drop for TemporaryEvidence {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn redaction_observer_emits_bounded_sink_observation() {
        let case_id = super::super::REDACTION_CASE_IDS[0];
        let case = super::super::super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == case_id)
            .unwrap();
        super::super::super::oracle::assert_literal(case, super::super::execute_redaction(case));
    }
}
