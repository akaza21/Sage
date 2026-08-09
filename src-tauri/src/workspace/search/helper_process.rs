use std::{
    ffi::OsString,
    future::{Future, poll_fn},
    path::PathBuf,
    process::Stdio,
    task::Poll,
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::oneshot,
    time::sleep,
};

use crate::error::{AppError, AppResult};

use super::{
    helper_protocol::{
        HELPER_ARGUMENT, HelperRequest, MAX_HELPER_INPUT_BYTES, MAX_HELPER_OUTPUT_BYTES,
        decode_reply,
    },
    types::WorkspaceSearchResponse,
};

pub(super) const COMMAND_DEADLINE: Duration = Duration::from_millis(2_500);

struct ProcessSpec {
    executable: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
}

impl ProcessSpec {
    fn production() -> AppResult<Self> {
        Ok(Self {
            executable: std::env::current_exe()?,
            arguments: vec![HELPER_ARGUMENT.into()],
            environment: Vec::new(),
        })
    }
}

pub(super) async fn run(
    request: HelperRequest,
    cancellation: oneshot::Receiver<()>,
) -> AppResult<WorkspaceSearchResponse> {
    run_with_spec(
        request,
        cancellation,
        ProcessSpec::production()?,
        COMMAND_DEADLINE,
    )
    .await
}

async fn run_with_spec(
    request: HelperRequest,
    cancellation: oneshot::Receiver<()>,
    spec: ProcessSpec,
    deadline: Duration,
) -> AppResult<WorkspaceSearchResponse> {
    let input = serde_json::to_vec(&request).map_err(|_| {
        AppError::InvalidRequest("workspace search helper request could not be encoded".into())
    })?;
    if input.len() as u64 > MAX_HELPER_INPUT_BYTES {
        return Err(AppError::ResourceLimit(
            "workspace search helper request exceeded its input bound".into(),
        ));
    }
    let mut command = Command::new(spec.executable);
    command
        .args(spec.arguments)
        .envs(spec.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        AppError::InvalidRequest("workspace search helper stdin is unavailable".into())
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        AppError::InvalidRequest("workspace search helper stdout is unavailable".into())
    })?;
    let writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let reader = tokio::spawn(async move {
        let mut output = Vec::new();
        stdout
            .take(MAX_HELPER_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut output)
            .await?;
        std::io::Result::Ok(output)
    });

    enum Completion {
        Exited(std::process::ExitStatus),
        Cancelled,
        Deadline,
    }
    let completion = {
        let mut wait = Box::pin(child.wait());
        let mut cancellation = Box::pin(cancellation);
        let mut deadline = Box::pin(sleep(deadline));
        poll_fn(|context| {
            if let Poll::Ready(status) = wait.as_mut().poll(context) {
                return Poll::Ready(status.map(Completion::Exited));
            }
            if cancellation.as_mut().poll(context).is_ready() {
                return Poll::Ready(Ok(Completion::Cancelled));
            }
            if deadline.as_mut().poll(context).is_ready() {
                return Poll::Ready(Ok(Completion::Deadline));
            }
            Poll::Pending
        })
        .await?
    };
    match completion {
        Completion::Exited(status) => {
            let write_result = writer.await.map_err(|error| {
                AppError::InvalidRequest(format!("workspace search helper writer failed: {error}"))
            })?;
            write_result?;
            let output = reader.await.map_err(|error| {
                AppError::InvalidRequest(format!("workspace search helper reader failed: {error}"))
            })??;
            if !status.success() || output.len() > MAX_HELPER_OUTPUT_BYTES {
                return Err(AppError::ResourceLimit(
                    "workspace search helper failed or exceeded its output bound".into(),
                ));
            }
            decode_reply(&output)
        }
        Completion::Cancelled | Completion::Deadline => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            writer.abort();
            reader.abort();
            let _ = writer.await;
            let _ = reader.await;
            match completion {
                Completion::Cancelled => Err(AppError::InvalidRequest(
                    "workspace search was cancelled".into(),
                )),
                Completion::Deadline => Err(AppError::ResourceLimit(
                    "workspace search exceeded its 2500 ms command deadline".into(),
                )),
                Completion::Exited(_) => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::search::{helper_protocol::HelperRequest, tests::request};

    #[test]
    fn sleeping_child_fixture() {
        if std::env::var_os("SAGE_SEARCH_SLEEP_FIXTURE").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn deadline_kills_and_reaps_the_actual_helper_process() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let executable = std::env::current_exe().unwrap();
            let spec = ProcessSpec {
                executable,
                arguments: vec![
                    "--exact".into(),
                    "workspace::search::helper_process::tests::sleeping_child_fixture".into(),
                    "--nocapture".into(),
                ],
                environment: vec![("SAGE_SEARCH_SLEEP_FIXTURE".into(), "1".into())],
            };
            let (_cancel, receiver) = oneshot::channel();
            let error = run_with_spec(
                HelperRequest::new(request("needle"), Vec::new()),
                receiver,
                spec,
                Duration::from_millis(100),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("deadline"));
        });
    }

    #[test]
    fn cancellation_kills_and_reaps_the_actual_helper_process() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let spec = ProcessSpec {
                executable: std::env::current_exe().unwrap(),
                arguments: vec![
                    "--exact".into(),
                    "workspace::search::helper_process::tests::sleeping_child_fixture".into(),
                    "--nocapture".into(),
                ],
                environment: vec![("SAGE_SEARCH_SLEEP_FIXTURE".into(), "1".into())],
            };
            let (cancel, receiver) = oneshot::channel();
            let cancellation = tokio::spawn(async move {
                sleep(Duration::from_millis(100)).await;
                let _ = cancel.send(());
            });
            let error = run_with_spec(
                HelperRequest::new(request("needle"), Vec::new()),
                receiver,
                spec,
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            cancellation.await.unwrap();
            assert!(error.to_string().contains("cancelled"));
        });
    }
}
