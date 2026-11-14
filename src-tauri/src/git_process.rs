use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use std::sync::Arc;

use sage_core::error::{AppError, AppResult};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use crate::git_budget;

mod cancellation;
mod containment;
mod parallel_limits;
#[cfg(all(test, windows))]
mod process_tree_tests;
mod readers;
mod repository_config;
mod root_authority;
mod submodule_isolation;
use readers::{ReaderTask, ReaderTasks};
use sage_process::process_tree::{GitChild, ProcessTree};
#[cfg(test)]
mod test_support;

pub(crate) use cancellation::wait_for_cleanup;
pub(crate) use containment::abandon_cleanup_on_exit;
#[cfg(test)]
pub(crate) use repository_config::trust_for_test as trust_repository_config_for_test;
pub(crate) use root_authority::GitRootAuthority;
#[cfg(test)]
pub(crate) use test_support::GitProcessTestHarness;

const MAX_STDERR_BYTES: usize = 64 * 1024;
/// Parent variables Git still needs after `env_clear`: the SSH agent socket for
/// SSH remotes, and `SystemRoot`, without which Winsock cannot initialise.
const INHERITED_ENV: &[&str] = &[
    "SSH_AUTH_SOCK",
    #[cfg(windows)]
    "SystemRoot",
];
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(test)]
type GitProcessHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
type GitSpawnHook = Arc<dyn Fn(u32) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct GitProcessPolicy {
    stdin: Option<Vec<u8>>,
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
    #[cfg(test)]
    trace2_event: Option<std::path::PathBuf>,
}

impl GitProcessPolicy {
    pub(crate) fn mutation(command_timeout: Duration) -> Self {
        Self {
            stdin: None,
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
            #[cfg(test)]
            trace2_event: None,
        }
    }

    pub(crate) fn read(command_timeout: Duration) -> Self {
        Self {
            stdin: None,
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
            #[cfg(test)]
            trace2_event: None,
        }
    }

    pub(crate) fn pre_ref_mutation(&self, command_timeout: Duration) -> Self {
        Self {
            stdin: None,
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
            #[cfg(test)]
            trace2_event: None,
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

pub(crate) async fn run_cancellable(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    input: Option<Vec<u8>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) -> AppResult<GitOutput> {
    let mut policy = GitProcessPolicy::read(git_budget::READ);
    policy.stdin = input;
    cancellation::CANCELLED
        .scope(cancelled, run(root, args, stdout_limit, policy))
        .await
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
    cancellation::ensure_active()?;
    let repository = repository_config::admit(root, root_authority.as_ref()).await?;
    if repository.is_some()
        && root_authority
            .as_ref()
            .is_some_and(|authority| !authority.anchors_repository())
    {
        return Err(AppError::InvalidRequest(
            "workspace root changed before Git access".into(),
        ));
    }
    run_admitted(
        root,
        args,
        stdout_limit,
        policy,
        root_authority,
        repository.as_ref(),
    )
    .await
}

/// Spawns Git without the repository-config admission check. Only the
/// admission probes call this directly; every other caller goes through
/// [`run_inner`]. `repository` pins Git to the repository admission inspected.
async fn run_admitted(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
    root_authority: Option<GitRootAuthority>,
    repository: Option<&repository_config::AdmittedRepository>,
) -> AppResult<GitOutput> {
    if let Some(root_authority) = &root_authority {
        root_authority.ensure_names(root)?;
    }
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
    let reaper_lease = tokio::select! {
        result = containment::reserve_reaper_owner(Arc::clone(&registry), policy.admission_timeout) => result?,
        () = cancellation::cancelled() => return Err(cancellation::error()),
    };

    cancellation::ensure_active()?;
    let mut command = configured_command(root, args, root_authority.as_ref(), repository);
    if policy.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    #[cfg(test)]
    if let Some(trace) = &policy.trace2_event {
        command.env("GIT_TRACE2_EVENT", trace);
    }
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
    let completed = tokio::select! {
        result = timeout(policy.command_timeout, async {
            if let Some(input) = &policy.stdin {
                let mut stdin = child.stdin.take().ok_or_else(|| PostSpawnFailure {
                    message: "Git stdin was not captured".into(), reaped: false,
                })?;
                stdin.write_all(input).await.map_err(|error| PostSpawnFailure {
                    message: format!("Git stdin write failed: {error}"), reaped: false,
                })?;
            }
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
        }) => result.unwrap_or_else(|_| Err(PostSpawnFailure {
            message: "the post-spawn Git deadline expired".into(), reaped: false,
        })),
        () = cancellation::cancelled() => Err(PostSpawnFailure {
            message: "Git watch window cancelled".into(), reaped: false,
        }),
    };

    match completed {
        Ok(output) => Ok(output),
        Err(failure) => {
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
    }
}

/// Git for test fixtures, isolated from the global and system config as
/// `configured_command` isolates Sage's own Git, so a developer's commit
/// signing, hooks or aliases cannot change what a fixture builds.
/// Repository-local config still applies. Repository and config variables a
/// Git hook exports are dropped, so tests run from a hook stay hermetic.
#[cfg(test)]
pub(crate) fn fixture_git() -> std::process::Command {
    let mut command = std::process::Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(name);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}

/// Subcommands whose operands are pathnames, not pathspecs. Git refuses
/// pathspec magic for them, so they run without `--literal-pathspecs`.
const PATHNAME_COMMANDS: &[&str] = &["check-ignore"];

fn configured_command(
    root: &Path,
    args: &[&str],
    root_authority: Option<&GitRootAuthority>,
    repository: Option<&repository_config::AdmittedRepository>,
) -> Command {
    let mut command = Command::new("git");
    command.arg("--no-optional-locks");
    if !args
        .first()
        .is_some_and(|name| PATHNAME_COMMANDS.contains(name))
    {
        command.arg("--literal-pathspecs");
    }
    command
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", "core.fsmonitor=false"])
        .args(["-c", "maintenance.auto=false"])
        .args(["-c", "gc.auto=0"])
        .args(["-c", "safe.bareRepository=explicit"]);
    for setting in submodule_isolation::CONFIG {
        command.arg("-c").arg(setting);
    }
    // The capped effective value of every setting that starts parallel
    // children; see `parallel_limits`. Command-line config wins over every
    // config file and reaches every Git child, submodules included.
    for setting in repository
        .into_iter()
        .flat_map(repository_config::AdmittedRepository::parallel_overrides)
    {
        command.arg("-c").arg(setting);
    }
    if let Some(root_authority) = root_authority {
        root_authority.bind_child(&mut command, root);
    } else {
        command.arg("-C").arg(repository_config::os_path(root));
    }
    command
        .args(submodule_isolation::arguments(args))
        .env_clear()
        // Git checks this allowlist before any protocol.*.allow config,
        // including for forced helpers and rewritten URLs. Children inherit
        // it. File access is admitted here; submodule_isolation prevents
        // Git from entering unadmitted submodule repositories.
        .env("GIT_ALLOW_PROTOCOL", "file:git:http:https:ssh")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .envs(
            INHERITED_ENV
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| (name, value))),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(repository) = repository {
        // Explicit locations stop discovery, so Git cannot drift to another
        // repository (an ancestor, say) between admission and this spawn.
        command
            .env("GIT_DIR", repository.git_dir())
            .env("GIT_WORK_TREE", repository.work_tree());
    }
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW | sage_process::process_tree::CREATION_FLAGS);
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
