use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

#[cfg(test)]
use std::sync::Arc;

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
    time::timeout,
};

#[cfg(test)]
use tokio::sync::Notify;

use crate::error::{AppError, AppResult};

mod containment;

const GIT_ADMISSION_TIMEOUT: Duration = Duration::from_secs(1);
const GIT_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_STDERR_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone)]
pub(crate) struct GitProcessPolicy {
    command_timeout: Duration,
    admission_timeout: Duration,
    reap_timeout: Option<Duration>,
    mutation: bool,
    #[cfg(test)]
    post_reap_failure: bool,
    #[cfg(test)]
    stdout_reader_failure: bool,
    #[cfg(test)]
    potentially_running: Option<PotentiallyRunningControl>,
}

#[cfg(test)]
#[derive(Clone)]
struct PotentiallyRunningControl {
    release: Arc<Notify>,
    exited: Arc<Notify>,
}

#[cfg(test)]
pub(crate) struct GitReaperTestHandle {
    control: PotentiallyRunningControl,
}

#[cfg(test)]
impl GitReaperTestHandle {
    pub(crate) fn release(&self) {
        self.control.release.notify_one();
    }

    pub(crate) async fn wait_for_exit(&self, deadline: Duration) -> AppResult<()> {
        timeout(deadline, self.control.exited.notified())
            .await
            .map_err(|_| AppError::Git("the injected Git reaper did not finish in time".into()))
    }
}

impl GitProcessPolicy {
    pub(crate) fn mutation(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            #[cfg(test)]
            post_reap_failure: false,
            #[cfg(test)]
            stdout_reader_failure: false,
            #[cfg(test)]
            potentially_running: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn nonconvergent_reap(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: None,
            mutation: true,
            post_reap_failure: false,
            stdout_reader_failure: false,
            potentially_running: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn post_reap_failure(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            post_reap_failure: true,
            stdout_reader_failure: false,
            potentially_running: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn stdout_reader_failure(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: true,
            post_reap_failure: false,
            stdout_reader_failure: true,
            potentially_running: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn potentially_running(command_timeout: Duration) -> (Self, GitReaperTestHandle) {
        let control = PotentiallyRunningControl {
            release: Arc::new(Notify::new()),
            exited: Arc::new(Notify::new()),
        };
        (
            Self {
                command_timeout,
                admission_timeout: GIT_ADMISSION_TIMEOUT,
                reap_timeout: Some(GIT_REAP_TIMEOUT),
                mutation: true,
                post_reap_failure: false,
                stdout_reader_failure: false,
                potentially_running: Some(control.clone()),
            },
            GitReaperTestHandle { control },
        )
    }

    #[cfg(test)]
    pub(crate) fn mutation_with_admission_timeout(
        command_timeout: Duration,
        admission_timeout: Duration,
    ) -> Self {
        Self {
            admission_timeout,
            ..Self::mutation(command_timeout)
        }
    }

    pub(crate) fn read(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: GIT_ADMISSION_TIMEOUT,
            reap_timeout: Some(GIT_REAP_TIMEOUT),
            mutation: false,
            #[cfg(test)]
            post_reap_failure: false,
            #[cfg(test)]
            stdout_reader_failure: false,
            #[cfg(test)]
            potentially_running: None,
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

#[derive(Default)]
struct ReaderTasks {
    stdout: Option<ReaderTask>,
    stderr: Option<ReaderTask>,
}

pub(crate) async fn run(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    if policy.mutation {
        containment::await_mutation_admission(policy.admission_timeout).await?;
    }
    let reaper_lease = containment::reserve_reaper_owner(
        containment::git_reaper_capacity(),
        policy.admission_timeout,
    )
    .await?;

    let mut command = configured_command(root, args);
    let mut child = command.spawn().map_err(AppError::Io)?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return containment::cleanup_failure(
                child,
                ReaderTasks::default(),
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stdout was not captured".into(),
                    reaped: false,
                },
                policy,
            )
            .await;
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return containment::cleanup_failure(
                child,
                ReaderTasks {
                    stdout: Some(tokio::spawn(read_bounded(stdout, stdout_limit))),
                    stderr: None,
                },
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stderr was not captured".into(),
                    reaped: false,
                },
                policy,
            )
            .await;
        }
    };
    let mut readers = ReaderTasks {
        stdout: Some(tokio::spawn(read_bounded(stdout, stdout_limit))),
        stderr: Some(tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES))),
    };
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
        let stdout = await_reader(
            &mut readers.stdout,
            "stdout",
            #[cfg(test)]
            policy.stdout_reader_failure,
            #[cfg(not(test))]
            false,
        )
        .await?;
        let stderr = await_reader(&mut readers.stderr, "stderr", false).await?;
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
            containment::cleanup_failure(child, readers, reaper_lease, failure, policy).await
        }
        Err(_) => {
            containment::cleanup_failure(
                child,
                readers,
                reaper_lease,
                PostSpawnFailure {
                    message: "the post-spawn Git deadline expired".into(),
                    reaped: false,
                },
                policy,
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

async fn await_reader(
    slot: &mut Option<ReaderTask>,
    stream: &str,
    inject_read_failure: bool,
) -> Result<BoundedBytes, PostSpawnFailure> {
    let result = slot
        .as_mut()
        .expect("reader slots are consumed exactly once")
        .await;
    let _completed = slot.take();
    let output = result
        .map_err(|error| PostSpawnFailure {
            message: format!("Git {stream} task failed: {error}"),
            reaped: true,
        })?
        .map_err(|error| PostSpawnFailure {
            message: format!("Git {stream} read failed: {error}"),
            reaped: true,
        })?;
    if inject_read_failure {
        return Err(PostSpawnFailure {
            message: format!("injected Git {stream} read failure after reader completion"),
            reaped: true,
        });
    }
    Ok(output)
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
