use std::{
    ffi::OsString,
    future::{Future, poll_fn},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    task::Poll,
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{OwnedSemaphorePermit, Semaphore, oneshot},
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::error::{AppError, AppResult};

use super::{
    helper_protocol::{
        HELPER_ARGUMENT, HelperRequest, MAX_HELPER_INPUT_BYTES, MAX_HELPER_OUTPUT_BYTES,
        decode_reply,
    },
    types::WorkspaceSearchResponse,
};

pub(super) const HELPER_RUN_DEADLINE: Duration = Duration::from_millis(2_200);
const REAP_DEADLINE: Duration = Duration::from_millis(250);
const IO_CLEANUP_DEADLINE: Duration = Duration::from_millis(50);
const LIFECYCLE_QUEUE_WAIT: Duration = Duration::from_millis(250);
static HELPER_LIFECYCLE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static REAPER_TASK: OnceLock<StdMutex<Option<JoinHandle<()>>>> = OnceLock::new();

struct ProcessSpec {
    executable: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    clear_environment: bool,
}

impl ProcessSpec {
    fn production() -> AppResult<Self> {
        Ok(Self {
            executable: std::env::current_exe()?,
            arguments: vec![HELPER_ARGUMENT.into()],
            environment: Vec::new(),
            clear_environment: true,
        })
    }
}

pub(super) async fn run(
    request: HelperRequest,
    cancellation: oneshot::Receiver<()>,
) -> AppResult<WorkspaceSearchResponse> {
    let lifecycle_lease = timeout(LIFECYCLE_QUEUE_WAIT, helper_lifecycle().acquire_owned())
        .await
        .map_err(|_| {
            AppError::ResourceLimit("workspace search helper cleanup is still in progress".into())
        })?
        .map_err(|_| AppError::InvalidRequest("workspace search helper is unavailable".into()))?;
    run_with_spec(
        request,
        cancellation,
        ProcessSpec::production()?,
        HELPER_RUN_DEADLINE,
        Some(REAP_DEADLINE),
        lifecycle_lease,
    )
    .await
}

async fn run_with_spec(
    request: HelperRequest,
    cancellation: oneshot::Receiver<()>,
    spec: ProcessSpec,
    deadline: Duration,
    reap_deadline: Option<Duration>,
    lifecycle_lease: OwnedSemaphorePermit,
) -> AppResult<WorkspaceSearchResponse> {
    let input = serde_json::to_vec(&request).map_err(|_| {
        AppError::InvalidRequest("workspace search helper request could not be encoded".into())
    })?;
    if input.len() as u64 > MAX_HELPER_INPUT_BYTES {
        return Err(AppError::ResourceLimit(
            "workspace search helper request exceeded its input bound".into(),
        ));
    }
    let mut command = configured_command(&spec);
    command
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
        WaitFailed,
    }
    let completion = {
        let mut wait = Box::pin(child.wait());
        let mut cancellation = Box::pin(cancellation);
        let mut deadline = Box::pin(sleep(deadline));
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
            if !status.success() || output.len() > MAX_HELPER_OUTPUT_BYTES {
                return Err(AppError::ResourceLimit(
                    "workspace search helper failed or exceeded its output bound".into(),
                ));
            }
            decode_reply(&output)
        }
        Completion::Cancelled | Completion::Deadline | Completion::WaitFailed => {
            writer.abort();
            reader.abort();
            let reaped =
                terminate_and_reap(child, writer, reader, lifecycle_lease, reap_deadline).await;
            match completion {
                Completion::Cancelled => Err(AppError::InvalidRequest(
                    if reaped {
                        "workspace search was cancelled"
                    } else {
                        "workspace search was cancelled; helper cleanup remains contained"
                    }
                    .into(),
                )),
                Completion::Deadline => Err(AppError::ResourceLimit(deadline_error(reaped))),
                Completion::WaitFailed => Err(AppError::InvalidRequest(
                    if reaped {
                        "workspace search helper wait failed"
                    } else {
                        "workspace search helper wait failed; cleanup remains contained"
                    }
                    .into(),
                )),
                Completion::Exited(_) => unreachable!(),
            }
        }
    }
}

fn deadline_error(reaped: bool) -> String {
    let mut message = format!(
        "workspace search exceeded its {} ms helper run deadline",
        HELPER_RUN_DEADLINE.as_millis()
    );
    if !reaped {
        message.push_str("; cleanup remains contained");
    }
    message
}

fn configured_command(spec: &ProcessSpec) -> Command {
    let mut command = Command::new(&spec.executable);
    if spec.clear_environment {
        command.env_clear();
    }
    command.args(&spec.arguments).envs(spec.environment.clone());
    command
}

async fn terminate_and_reap(
    mut child: tokio::process::Child,
    writer: JoinHandle<std::io::Result<()>>,
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
    lifecycle_lease: OwnedSemaphorePermit,
    deadline: Option<Duration>,
) -> bool {
    let kill_failed = child.start_kill().is_err();
    if let Some(deadline) = deadline
        && bounded_reap(child.wait(), deadline).await
    {
        settle_io(writer, reader).await;
        return true;
    }
    tracing::warn!(
        kill_failed,
        "workspace search helper cleanup moved to the bounded reaper owner"
    );
    handoff_reaper(child, writer, reader, lifecycle_lease);
    false
}

async fn bounded_reap<F>(reap: F, deadline: Duration) -> bool
where
    F: Future<Output = std::io::Result<std::process::ExitStatus>>,
{
    matches!(timeout(deadline, reap).await, Ok(Ok(_)))
}

fn handoff_reaper(
    mut child: tokio::process::Child,
    writer: JoinHandle<std::io::Result<()>>,
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
    lifecycle_lease: OwnedSemaphorePermit,
) {
    let previous = reaper_task()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let task = tokio::spawn(async move {
        if let Some(previous) = previous {
            let _ = previous.await;
        }
        let _lifecycle_lease = lifecycle_lease;
        settle_io(writer, reader).await;
        loop {
            let _ = child.start_kill();
            if child.wait().await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    });
    *reaper_task()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(task);
}

async fn settle_io(
    writer: JoinHandle<std::io::Result<()>>,
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
) {
    let _ = timeout(IO_CLEANUP_DEADLINE, async {
        let _ = writer.await;
        let _ = reader.await;
    })
    .await;
}

async fn collect_output(
    writer: JoinHandle<std::io::Result<()>>,
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
) -> AppResult<Vec<u8>> {
    timeout(IO_CLEANUP_DEADLINE, async {
        writer.await.map_err(|error| {
            AppError::InvalidRequest(format!("workspace search helper writer failed: {error}"))
        })??;
        reader
            .await
            .map_err(|error| {
                AppError::InvalidRequest(format!("workspace search helper reader failed: {error}"))
            })?
            .map_err(AppError::from)
    })
    .await
    .map_err(|_| {
        AppError::ResourceLimit("workspace search helper output did not close in time".into())
    })?
}

fn helper_lifecycle() -> Arc<Semaphore> {
    HELPER_LIFECYCLE
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn reaper_task() -> &'static StdMutex<Option<JoinHandle<()>>> {
    REAPER_TASK.get_or_init(|| StdMutex::new(None))
}

#[cfg(test)]
mod tests;
