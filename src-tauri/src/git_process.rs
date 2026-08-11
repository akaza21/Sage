use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::{sleep, timeout},
};

use crate::error::{AppError, AppResult, GitContainmentState};

const GIT_ADMISSION_TIMEOUT: Duration = Duration::from_secs(1);
const GIT_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const GIT_REAPER_CAPACITY: usize = 8;
const IO_CLEANUP_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_STDERR_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
pub(crate) struct GitProcessPolicy {
    command_timeout: Duration,
    reap_timeout: Option<Duration>,
    mutation: bool,
    #[cfg(test)]
    post_reap_failure: bool,
}

impl GitProcessPolicy {
    pub(crate) fn mutation(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            #[cfg(test)]
            post_reap_failure: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn nonconvergent_reap(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            reap_timeout: None,
            mutation: true,
            post_reap_failure: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn post_reap_failure(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            post_reap_failure: true,
        }
    }

    pub(crate) fn read(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: false,
            #[cfg(test)]
            post_reap_failure: false,
        }
    }
}

pub(crate) struct GitOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) stdout_truncated: bool,
}

struct BoundedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

struct PostSpawnFailure {
    message: String,
    reaped: bool,
}

type ReaderTask = JoinHandle<std::io::Result<BoundedBytes>>;

pub(crate) async fn run(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    if policy.mutation {
        await_mutation_admission().await?;
    }
    let reaper_lease = timeout(GIT_ADMISSION_TIMEOUT, git_reaper_capacity().acquire_owned())
        .await
        .map_err(|_| AppError::ResourceLimit("the bounded Git reaper is at capacity".into()))?
        .map_err(|_| AppError::Git("the bounded Git reaper is unavailable".into()))?;

    let mut command = configured_command(root, args);
    let mut child = command.spawn().map_err(AppError::Io)?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return cleanup_failure(
                child,
                None,
                None,
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stdout was not captured".into(),
                    reaped: false,
                },
                policy.reap_timeout,
            )
            .await;
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return cleanup_failure(
                child,
                Some(tokio::spawn(read_bounded(stdout, stdout_limit))),
                None,
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stderr was not captured".into(),
                    reaped: false,
                },
                policy.reap_timeout,
            )
            .await;
        }
    };
    let mut stdout_task = tokio::spawn(read_bounded(stdout, stdout_limit));
    let mut stderr_task = tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES));
    let completed = timeout(policy.command_timeout, async {
        let status = child.wait().await.map_err(|error| PostSpawnFailure {
            message: format!("Git process wait failed: {error}"),
            reaped: false,
        })?;
        #[cfg(test)]
        if policy.post_reap_failure {
            return Err(PostSpawnFailure {
                message: "injected Git output failure after reap".into(),
                reaped: true,
            });
        }
        let stdout = (&mut stdout_task)
            .await
            .map_err(|error| PostSpawnFailure {
                message: format!("Git stdout task failed: {error}"),
                reaped: true,
            })?
            .map_err(|error| PostSpawnFailure {
                message: format!("Git stdout read failed: {error}"),
                reaped: true,
            })?;
        let stderr = (&mut stderr_task)
            .await
            .map_err(|error| PostSpawnFailure {
                message: format!("Git stderr task failed: {error}"),
                reaped: true,
            })?
            .map_err(|error| PostSpawnFailure {
                message: format!("Git stderr read failed: {error}"),
                reaped: true,
            })?;
        Ok::<GitOutput, PostSpawnFailure>(GitOutput {
            status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
        })
    })
    .await;

    match completed {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(failure)) => {
            cleanup_failure(
                child,
                Some(stdout_task),
                Some(stderr_task),
                reaper_lease,
                failure,
                policy.reap_timeout,
            )
            .await
        }
        Err(_) => {
            cleanup_failure(
                child,
                Some(stdout_task),
                Some(stderr_task),
                reaper_lease,
                PostSpawnFailure {
                    message: "the post-spawn Git deadline expired".into(),
                    reaped: false,
                },
                policy.reap_timeout,
            )
            .await
        }
    }
}

fn configured_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

async fn cleanup_failure(
    mut child: Child,
    stdout_task: Option<ReaderTask>,
    stderr_task: Option<ReaderTask>,
    reaper_lease: OwnedSemaphorePermit,
    failure: PostSpawnFailure,
    reap_timeout: Option<Duration>,
) -> AppResult<GitOutput> {
    if failure.reaped {
        settle_readers(stdout_task, stderr_task).await;
        return Err(AppError::GitEffectUnknown(format!(
            "{}; the Git process exited and was reaped",
            failure.message
        )));
    }

    let kill_succeeded = child.start_kill().is_ok();
    let already_reaped = !kill_succeeded && matches!(child.try_wait(), Ok(Some(_)));
    let termination_proven = kill_succeeded || already_reaped;
    let reaped = already_reaped
        || match reap_timeout {
            Some(deadline) => matches!(timeout(deadline, child.wait()).await, Ok(Ok(_))),
            None => false,
        };
    if reaped {
        settle_readers(stdout_task, stderr_task).await;
        return Err(AppError::GitEffectUnknown(format!(
            "{}; the Git process was terminated and reaped",
            failure.message
        )));
    }

    let state = if termination_proven {
        GitContainmentState::TerminatedUnreaped
    } else {
        GitContainmentState::PotentiallyRunning
    };
    if !termination_proven {
        potentially_running_git().fetch_add(1, Ordering::SeqCst);
    }
    handoff_reaper(
        child,
        stdout_task,
        stderr_task,
        reaper_lease,
        termination_proven,
    );
    Err(containment(
        state,
        if termination_proven {
            format!(
                "{}; Git termination is proven, but reap did not converge",
                failure.message
            )
        } else {
            format!(
                "{}; Git termination and reap could not be confirmed",
                failure.message
            )
        },
    ))
}

fn containment(state: GitContainmentState, message: impl Into<String>) -> AppError {
    AppError::GitContainment {
        state,
        message: message.into(),
    }
}

fn handoff_reaper(
    mut child: Child,
    stdout_task: Option<ReaderTask>,
    stderr_task: Option<ReaderTask>,
    reaper_lease: OwnedSemaphorePermit,
    mut termination_proven: bool,
) {
    tokio::spawn(async move {
        let _reaper_lease = reaper_lease;
        settle_readers(stdout_task, stderr_task).await;
        loop {
            if !termination_proven {
                termination_proven = child.start_kill().is_ok();
                if termination_proven {
                    potentially_running_git().fetch_sub(1, Ordering::SeqCst);
                }
            }
            if matches!(
                timeout(Duration::from_millis(100), child.wait()).await,
                Ok(Ok(_))
            ) {
                if !termination_proven {
                    potentially_running_git().fetch_sub(1, Ordering::SeqCst);
                }
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    });
}

async fn settle_readers(stdout_task: Option<ReaderTask>, stderr_task: Option<ReaderTask>) {
    if let Some(task) = &stdout_task {
        task.abort();
    }
    if let Some(task) = &stderr_task {
        task.abort();
    }
    let _ = timeout(IO_CLEANUP_TIMEOUT, async {
        if let Some(task) = stdout_task {
            let _ = task.await;
        }
        if let Some(task) = stderr_task {
            let _ = task.await;
        }
    })
    .await;
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<BoundedBytes> {
    let mut result = BoundedBytes {
        bytes: Vec::with_capacity(limit.min(64 * 1024)),
        truncated: false,
    };
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(result.bytes.len());
        let retained = remaining.min(read);
        result.bytes.extend_from_slice(&buffer[..retained]);
        result.truncated |= retained < read;
    }
    Ok(result)
}

fn git_reaper_capacity() -> Arc<Semaphore> {
    static GIT_REAPER: OnceLock<Arc<Semaphore>> = OnceLock::new();
    GIT_REAPER
        .get_or_init(|| Arc::new(Semaphore::new(GIT_REAPER_CAPACITY)))
        .clone()
}

async fn await_mutation_admission() -> AppResult<()> {
    timeout(GIT_ADMISSION_TIMEOUT, async {
        while potentially_running_git().load(Ordering::SeqCst) != 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| {
        containment(
            GitContainmentState::PotentiallyRunning,
            "a prior Git process is still under containment",
        )
    })
}

fn potentially_running_git() -> &'static AtomicUsize {
    static POTENTIALLY_RUNNING_GIT: AtomicUsize = AtomicUsize::new(0);
    &POTENTIALLY_RUNNING_GIT
}
