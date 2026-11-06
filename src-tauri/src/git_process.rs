use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use std::sync::Arc;

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};

use crate::{
    error::{AppError, AppResult},
    git_budget,
};

mod containment;
mod process_tree;
mod readers;
mod root_authority;
use process_tree::{GitChild, ProcessTree};
use readers::{ReaderTask, ReaderTasks};
#[cfg(test)]
mod test_support;

pub(crate) use root_authority::GitRootAuthority;
#[cfg(test)]
pub(crate) use test_support::GitProcessTestHarness;

const MAX_STDERR_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(test)]
type GitProcessHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
type GitSpawnHook = Arc<dyn Fn(u32) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct GitProcessPolicy {
    command_timeout: Duration,
    admission_timeout: Duration,
    reap_timeout: Option<Duration>,
    mutation: bool,
    visible_effect: bool,
    #[cfg(test)]
    post_reap_failure: bool,
    #[cfg(test)]
    post_spawn_failure: bool,
    #[cfg(test)]
    stdout_reader_failure: bool,
    #[cfg(test)]
    reaper_control: Option<Arc<test_support::ReaperTestControl>>,
    #[cfg(test)]
    containment_registry: Option<Arc<containment::ContainmentRegistry>>,
    #[cfg(test)]
    before_spawn_hook: Option<GitProcessHook>,
    #[cfg(test)]
    after_process_hook: Option<GitProcessHook>,
    #[cfg(test)]
    spawned_hook: Option<GitSpawnHook>,
    #[cfg(test)]
    deadline_wait_gate: Option<Arc<test_support::DeadlineWaitGate>>,
}

impl GitProcessPolicy {
    pub(crate) fn mutation(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: git_budget::ADMISSION,
            reap_timeout: Some(git_budget::REAP),
            mutation: true,
            visible_effect: true,
            #[cfg(test)]
            post_reap_failure: false,
            #[cfg(test)]
            post_spawn_failure: false,
            #[cfg(test)]
            stdout_reader_failure: false,
            #[cfg(test)]
            reaper_control: None,
            #[cfg(test)]
            containment_registry: Some(containment::ContainmentRegistry::isolated()),
            #[cfg(test)]
            before_spawn_hook: None,
            #[cfg(test)]
            after_process_hook: None,
            #[cfg(test)]
            spawned_hook: None,
            #[cfg(test)]
            deadline_wait_gate: None,
        }
    }

    pub(crate) fn read(command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: git_budget::ADMISSION,
            reap_timeout: Some(git_budget::REAP),
            mutation: false,
            visible_effect: false,
            #[cfg(test)]
            post_reap_failure: false,
            #[cfg(test)]
            post_spawn_failure: false,
            #[cfg(test)]
            stdout_reader_failure: false,
            #[cfg(test)]
            reaper_control: None,
            #[cfg(test)]
            containment_registry: Some(containment::ContainmentRegistry::isolated()),
            #[cfg(test)]
            before_spawn_hook: None,
            #[cfg(test)]
            after_process_hook: None,
            #[cfg(test)]
            spawned_hook: None,
            #[cfg(test)]
            deadline_wait_gate: None,
        }
    }

    pub(crate) fn pre_ref_mutation(&self, command_timeout: Duration) -> Self {
        Self {
            command_timeout,
            admission_timeout: self.admission_timeout,
            reap_timeout: Some(git_budget::REAP),
            mutation: true,
            visible_effect: false,
            #[cfg(test)]
            post_reap_failure: false,
            #[cfg(test)]
            post_spawn_failure: false,
            #[cfg(test)]
            stdout_reader_failure: false,
            #[cfg(test)]
            reaper_control: None,
            #[cfg(test)]
            containment_registry: self.containment_registry.clone(),
            #[cfg(test)]
            before_spawn_hook: None,
            #[cfg(test)]
            after_process_hook: None,
            #[cfg(test)]
            spawned_hook: None,
            #[cfg(test)]
            deadline_wait_gate: None,
        }
    }

    pub(crate) fn has_visible_effect(&self) -> bool {
        self.visible_effect
    }

    #[cfg(test)]
    pub(crate) fn after_process_hook(&self) -> Option<GitProcessHook> {
        self.after_process_hook.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_test_policies_do_not_share_git_containment_capacity() {
        let first = GitProcessPolicy::mutation(Duration::from_secs(1));
        let second = GitProcessPolicy::read(Duration::from_secs(1));

        assert!(!Arc::ptr_eq(
            first
                .containment_registry
                .as_ref()
                .expect("test mutation policy has isolated containment"),
            second
                .containment_registry
                .as_ref()
                .expect("test read policy has isolated containment"),
        ));
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

pub(crate) async fn run(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    run_inner(root, args, stdout_limit, policy, None).await
}

pub(crate) async fn run_with_authority(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
    root_authority: GitRootAuthority,
) -> AppResult<GitOutput> {
    run_inner(root, args, stdout_limit, policy, Some(root_authority)).await
}

async fn run_inner(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
    root_authority: Option<GitRootAuthority>,
) -> AppResult<GitOutput> {
    #[cfg(test)]
    let registry = policy
        .containment_registry
        .clone()
        .unwrap_or_else(containment::git_containment);
    #[cfg(not(test))]
    let registry = containment::git_containment();
    if policy.mutation {
        containment::await_mutation_admission(&registry, policy.admission_timeout).await?;
    }
    let reaper_lease =
        containment::reserve_reaper_owner(Arc::clone(&registry), policy.admission_timeout).await?;

    let mut command = configured_command(root, args, root_authority.as_ref());
    #[cfg(test)]
    if let Some(hook) = &policy.before_spawn_hook {
        hook();
    }
    let mut child = command.spawn().map_err(AppError::Io)?;
    // Created suspended on Windows: it runs only once its whole tree is held.
    let tree = match ProcessTree::contain(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.start_kill();
            return Err(AppError::Io(error));
        }
    };
    #[cfg(test)]
    if let (Some(hook), Some(pid)) = (&policy.spawned_hook, child.id()) {
        hook(pid);
    }
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return containment::cleanup_failure(
                GitChild { root: child, tree },
                ReaderTasks::default(),
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stdout was not captured".into(),
                    reaped: false,
                },
                policy,
                registry,
                root_authority,
            )
            .await;
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return containment::cleanup_failure(
                GitChild { root: child, tree },
                ReaderTasks {
                    stdout: Some(tokio::spawn(read_bounded(stdout, stdout_limit))),
                    stderr: None,
                    ..ReaderTasks::default()
                },
                reaper_lease,
                PostSpawnFailure {
                    message: "Git stderr was not captured".into(),
                    reaped: false,
                },
                policy,
                registry,
                root_authority,
            )
            .await;
        }
    };
    let mut readers = ReaderTasks {
        stdout: Some(tokio::spawn(read_bounded(stdout, stdout_limit))),
        stderr: Some(tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES))),
        ..ReaderTasks::default()
    };
    #[cfg(test)]
    if policy.post_spawn_failure {
        return containment::cleanup_failure(
            GitChild { root: child, tree },
            readers,
            reaper_lease,
            PostSpawnFailure {
                message: "injected Git failure after spawn and reader setup".into(),
                reaped: false,
            },
            policy,
            registry,
            root_authority,
        )
        .await;
    }
    let completed = timeout(policy.command_timeout, async {
        #[cfg(test)]
        if let Some(gate) = &policy.deadline_wait_gate {
            gate.wait().await;
        }
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
            &mut readers.failed,
            "stdout",
            #[cfg(test)]
            policy.stdout_reader_failure,
            #[cfg(not(test))]
            false,
        )
        .await?;
        let stderr =
            await_reader(&mut readers.stderr, &mut readers.failed, "stderr", false).await?;
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
            containment::cleanup_failure(
                GitChild { root: child, tree },
                readers,
                reaper_lease,
                failure,
                policy,
                registry,
                root_authority,
            )
            .await
        }
        Err(_) => {
            containment::cleanup_failure(
                GitChild { root: child, tree },
                readers,
                reaper_lease,
                PostSpawnFailure {
                    message: "the post-spawn Git deadline expired".into(),
                    reaped: false,
                },
                policy,
                registry,
                root_authority,
            )
            .await
        }
    }
}

fn configured_command(
    root: &Path,
    args: &[&str],
    root_authority: Option<&GitRootAuthority>,
) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("--literal-pathspecs")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "maintenance.auto=false"])
        .args(["-c", "gc.auto=0"]);
    for setting in process_tree::parallel_job_overrides() {
        command.arg("-c").arg(setting);
    }
    if let Some(root_authority) = root_authority {
        root_authority.bind_child(&mut command, root);
    } else {
        command.arg("-C").arg(root);
    }
    command
        .args(args)
        .env_clear()
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW | process_tree::CREATION_FLAGS);
    command
}

async fn await_reader(
    slot: &mut Option<ReaderTask>,
    failed: &mut bool,
    stream: &str,
    inject_read_failure: bool,
) -> Result<BoundedBytes, PostSpawnFailure> {
    let result = slot
        .as_mut()
        .expect("reader slots are consumed exactly once")
        .await;
    let _completed = slot.take();
    *failed |= !matches!(&result, Ok(Ok(_)));
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

#[cfg(test)]
mod descendant_tests;
