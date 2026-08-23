use std::{
    ffi::OsString,
    future::poll_fn,
    io::{Read, Write},
    process::Stdio,
    task::Poll,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::oneshot,
    time::{sleep, timeout},
};

use crate::domain::ResourceRef;

pub(crate) const LANGUAGE_FIXTURE_ARGUMENT: &str = "--sage-language-fixture-v1";
const MAX_LANGUAGE_INPUT_BYTES: u64 = 64 * 1024;
const MAX_LANGUAGE_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DOCUMENT_BYTES: usize = 48 * 1024;
const MAX_DIAGNOSTICS: usize = 64;
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_OUTLINE_SYMBOLS: usize = 64;
const MAX_SYMBOL_NAME_BYTES: usize = 256;
const RUN_DEADLINE: Duration = Duration::from_millis(2_200);
const REAP_DEADLINE: Duration = Duration::from_millis(250);
const TEST_REPLY_START: &[u8] = b"SAGE_LANGUAGE_TEST_REPLY_START\n";
const TEST_REPLY_END: &[u8] = b"\nSAGE_LANGUAGE_TEST_REPLY_END";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageFixtureRequest {
    pub(crate) resource: ResourceRef,
    pub(crate) generation: u32,
    pub(crate) document: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageDiagnostic {
    pub(crate) resource: ResourceRef,
    pub(crate) message: String,
    pub(crate) severity: String,
    pub(crate) start_line: u32,
    pub(crate) start_column: u32,
    pub(crate) end_line: u32,
    pub(crate) end_column: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageDiagnosticReport {
    pub(crate) resource: ResourceRef,
    pub(crate) generation: u32,
    pub(crate) diagnostics: Vec<LanguageDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageOutlineSymbol {
    pub(crate) resource: ResourceRef,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) start_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageOutlineReport {
    pub(crate) resource: ResourceRef,
    pub(crate) generation: u32,
    pub(crate) symbols: Vec<LanguageOutlineSymbol>,
}

#[derive(Debug, Clone)]
pub(crate) struct LanguageFixtureReports {
    pub(crate) diagnostics: LanguageDiagnosticReport,
    pub(crate) outline: LanguageOutlineReport,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LanguageFixtureReply {
    diagnostics: Option<LanguageDiagnosticReport>,
    outline: Option<LanguageOutlineReport>,
    error: Option<String>,
}

struct ProcessSpec {
    executable: std::path::PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
}

impl ProcessSpec {
    fn production() -> Result<Self, String> {
        Ok(Self {
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            arguments: vec![LANGUAGE_FIXTURE_ARGUMENT.into()],
            environment: Vec::new(),
        })
    }
}

pub(crate) async fn run(
    request: LanguageFixtureRequest,
    cancellation: oneshot::Receiver<()>,
) -> Result<LanguageFixtureReports, String> {
    run_with_spec(request, cancellation, ProcessSpec::production()?, false).await
}

async fn run_with_spec(
    request: LanguageFixtureRequest,
    cancellation: oneshot::Receiver<()>,
    spec: ProcessSpec,
    test_framing: bool,
) -> Result<LanguageFixtureReports, String> {
    validate_request(&request)?;
    let input = serde_json::to_vec(&request).map_err(|_| "language request encoding failed")?;
    if input.len() as u64 > MAX_LANGUAGE_INPUT_BYTES {
        return Err("language fixture input exceeded its bound".into());
    }
    let mut command = Command::new(spec.executable);
    command
        .args(spec.arguments)
        .env_clear()
        .envs(spec.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or("language fixture stdin is unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("language fixture stdout is unavailable")?;
    let writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let reader = tokio::spawn(async move {
        let mut output = Vec::new();
        stdout
            .take(MAX_LANGUAGE_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut output)
            .await?;
        std::io::Result::Ok(output)
    });

    enum Completion {
        Exited(std::process::ExitStatus),
        Cancelled,
        Deadline,
        WaitFailed,
    }
    let completion = {
        let mut wait = Box::pin(child.wait());
        let mut cancellation = Box::pin(cancellation);
        let mut deadline = Box::pin(sleep(RUN_DEADLINE));
        poll_fn(|context| {
            if let Poll::Ready(status) = wait.as_mut().poll(context) {
                return Poll::Ready(match status {
                    Ok(status) => Completion::Exited(status),
                    Err(_) => Completion::WaitFailed,
                });
            }
            if cancellation.as_mut().poll(context).is_ready() {
                return Poll::Ready(Completion::Cancelled);
            }
            if deadline.as_mut().poll(context).is_ready() {
                return Poll::Ready(Completion::Deadline);
            }
            Poll::Pending
        })
        .await
    };
    match completion {
        Completion::Exited(status) => {
            let output = collect_output(writer, reader).await?;
            if !status.success() || output.len() > MAX_LANGUAGE_OUTPUT_BYTES {
                return Err("language fixture failed or exceeded its output bound".into());
            }
            let reply = if test_framing {
                test_reply(&output)?
            } else {
                &output
            };
            decode_reply(reply, &request)
        }
        Completion::Cancelled | Completion::Deadline | Completion::WaitFailed => {
            writer.abort();
            reader.abort();
            let reaped = matches!(timeout(REAP_DEADLINE, child.kill()).await, Ok(Ok(())));
            let suffix = if reaped {
                ""
            } else {
                "; cleanup remains contained"
            };
            match completion {
                Completion::Cancelled => Err(format!("language session was cancelled{suffix}")),
                Completion::Deadline => {
                    Err(format!("language fixture exceeded its deadline{suffix}"))
                }
                Completion::WaitFailed => Err(format!("language fixture wait failed{suffix}")),
                Completion::Exited(_) => unreachable!(),
            }
        }
    }
}

async fn collect_output(
    writer: tokio::task::JoinHandle<std::io::Result<()>>,
    reader: tokio::task::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, String> {
    let output = timeout(Duration::from_millis(50), async {
        writer
            .await
            .map_err(|_| "language fixture writer failed")?
            .map_err(|_| "language fixture writer failed")?;
        reader
            .await
            .map_err(|_| "language fixture reader failed")?
            .map_err(|_| "language fixture reader failed")
    })
    .await
    .map_err(|_| "language fixture output did not close in time".to_string())?
    .map_err(str::to_owned)?;
    Ok(output)
}

pub(super) fn run_stdio_fixture_if_requested() -> bool {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 1 || arguments[0] != LANGUAGE_FIXTURE_ARGUMENT {
        return false;
    }
    write_stdio_reply();
    true
}

#[cfg(test)]
pub(crate) fn run_test_stdio_child() {
    if std::env::var_os("SAGE_LANGUAGE_FIXTURE_TEST_CHILD").is_some() {
        write_stdio_reply();
    }
}

fn write_stdio_reply() {
    let mut input = Vec::new();
    let result = std::io::stdin()
        .take(MAX_LANGUAGE_INPUT_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|_| "language fixture input is unavailable")
        .and_then(|_| {
            if input.len() as u64 > MAX_LANGUAGE_INPUT_BYTES {
                Err("language fixture input exceeded its bound")
            } else {
                serde_json::from_slice(&input).map_err(|_| "language fixture input is invalid")
            }
        })
        .and_then(execute);
    if matches!(&result, Err(error) if *error == "language fixture crash") {
        std::process::exit(1);
    }
    if matches!(&result, Err(error) if *error == "language fixture output overrun") {
        let _ = std::io::stdout().write_all(&vec![b'x'; MAX_LANGUAGE_OUTPUT_BYTES + 1]);
        return;
    }
    let reply = match result {
        Ok(report) => LanguageFixtureReply {
            diagnostics: Some(report.diagnostics),
            outline: Some(report.outline),
            error: None,
        },
        Err(error) => LanguageFixtureReply {
            diagnostics: None,
            outline: None,
            error: Some(error.into()),
        },
    };
    if let Ok(output) = serde_json::to_vec(&reply)
        && output.len() <= MAX_LANGUAGE_OUTPUT_BYTES
    {
        let framed = std::env::var_os("SAGE_LANGUAGE_FIXTURE_TEST_CHILD").is_some();
        if framed {
            let _ = std::io::stdout().write_all(TEST_REPLY_START);
        }
        let _ = std::io::stdout().write_all(&output);
        if framed {
            let _ = std::io::stdout().write_all(TEST_REPLY_END);
        }
    }
}

fn execute(request: LanguageFixtureRequest) -> Result<LanguageFixtureReports, &'static str> {
    validate_request(&request).map_err(|_| "language fixture input is invalid")?;
    if request.document.contains("SAGE_TS_FIXTURE_SLEEP") {
        std::thread::sleep(Duration::from_secs(30));
    }
    if request.document.contains("SAGE_TS_FIXTURE_CRASH") {
        return Err("language fixture crash");
    }
    if request.document.contains("SAGE_TS_FIXTURE_OVERRUN") {
        return Err("language fixture output overrun");
    }
    let diagnostics = request
        .document
        .find("SAGE_TS_DIAGNOSTIC")
        .map(|offset| LanguageDiagnostic {
            resource: request.resource.clone(),
            message: "TypeScript fixture diagnostic".into(),
            severity: "error".into(),
            start_line: request.document[..offset]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1,
            start_column: 1,
            end_line: request.document[..offset]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1,
            end_column: "SAGE_TS_DIAGNOSTIC".len() as u32 + 1,
        })
        .into_iter()
        .collect();
    let outline = outline_report(&request);
    Ok(LanguageFixtureReports {
        diagnostics: LanguageDiagnosticReport {
            resource: request.resource,
            generation: request.generation,
            diagnostics,
        },
        outline,
    })
}

fn outline_report(request: &LanguageFixtureRequest) -> LanguageOutlineReport {
    let symbols = request
        .document
        .find("function ")
        .and_then(|offset| {
            let name = request.document[offset + "function ".len()..]
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$')
                .collect::<Vec<_>>();
            (!name.is_empty()).then(|| LanguageOutlineSymbol {
                resource: request.resource.clone(),
                name: String::from_utf8(name).unwrap_or_default(),
                kind: "function".into(),
                start_line: request.document[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u32
                    + 1,
            })
        })
        .into_iter()
        .collect();
    LanguageOutlineReport {
        resource: request.resource.clone(),
        generation: request.generation,
        symbols,
    }
}

fn validate_request(request: &LanguageFixtureRequest) -> Result<(), String> {
    if request.resource.root_id.is_empty()
        || request.resource.relative_path.is_empty()
        || request.resource.root_id.len() > 512
        || request.resource.relative_path.len() > 8 * 1024
        || request.document.len() > MAX_DOCUMENT_BYTES
    {
        return Err("language fixture request is invalid".into());
    }
    Ok(())
}

fn decode_reply(
    bytes: &[u8],
    request: &LanguageFixtureRequest,
) -> Result<LanguageFixtureReports, String> {
    let reply: LanguageFixtureReply =
        serde_json::from_slice(bytes).map_err(|_| "language fixture returned invalid output")?;
    match (reply.diagnostics, reply.outline, reply.error) {
        (Some(diagnostics), Some(outline), None)
            if diagnostics.resource == request.resource
                && diagnostics.generation == request.generation
                && valid_report(&diagnostics)
                && outline.resource == request.resource
                && outline.generation == request.generation
                && valid_outline_report(&outline) =>
        {
            Ok(LanguageFixtureReports {
                diagnostics,
                outline,
            })
        }
        (None, None, Some(error)) if !error.is_empty() && error.len() <= MAX_MESSAGE_BYTES => {
            Err(error)
        }
        _ => Err("language fixture returned a contradictory result".into()),
    }
}

fn valid_outline_report(report: &LanguageOutlineReport) -> bool {
    report.symbols.len() <= MAX_OUTLINE_SYMBOLS
        && report.symbols.iter().all(|symbol| {
            symbol.resource == report.resource
                && !symbol.name.is_empty()
                && symbol.name.len() <= MAX_SYMBOL_NAME_BYTES
                && matches!(
                    symbol.kind.as_str(),
                    "file" | "function" | "type" | "unknown"
                )
                && symbol.start_line > 0
        })
}

fn test_reply(bytes: &[u8]) -> Result<&[u8], String> {
    let start = bytes
        .windows(TEST_REPLY_START.len())
        .position(|window| window == TEST_REPLY_START)
        .ok_or("language fixture test reply is missing")?
        + TEST_REPLY_START.len();
    let end = bytes[start..]
        .windows(TEST_REPLY_END.len())
        .position(|window| window == TEST_REPLY_END)
        .ok_or("language fixture test reply is incomplete")?
        + start;
    Ok(&bytes[start..end])
}

fn valid_report(report: &LanguageDiagnosticReport) -> bool {
    report.diagnostics.len() <= MAX_DIAGNOSTICS
        && report.diagnostics.iter().all(|diagnostic| {
            diagnostic.resource == report.resource
                && diagnostic.message.len() <= MAX_MESSAGE_BYTES
                && matches!(diagnostic.severity.as_str(), "error" | "warning" | "info")
                && diagnostic.start_line > 0
                && diagnostic.start_column > 0
                && diagnostic.end_line >= diagnostic.start_line
                && diagnostic.end_column > 0
        })
}

#[cfg(test)]
fn test_process_spec() -> ProcessSpec {
    ProcessSpec {
        executable: std::env::current_exe().unwrap(),
        arguments: vec![
            "--exact".into(),
            "editor::language::tests::language_fixture_test_child".into(),
            "--nocapture".into(),
            "--quiet".into(),
        ],
        environment: vec![("SAGE_LANGUAGE_FIXTURE_TEST_CHILD".into(), "1".into())],
    }
}

#[cfg(test)]
pub(crate) async fn run_test_fixture(
    request: LanguageFixtureRequest,
    cancellation: oneshot::Receiver<()>,
) -> Result<LanguageFixtureReports, String> {
    run_with_spec(request, cancellation, test_process_spec(), true).await
}
