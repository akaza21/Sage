#[cfg(unix)]
use std::path::PathBuf;
use std::{fs, time::Duration};

use super::{GitProcessTestHarness, run as run_git_with_policy};
use crate::error::{AppError, GitContainmentState};

#[cfg(unix)]
struct ReleaseBackend(PathBuf);

#[cfg(unix)]
impl Drop for ReleaseBackend {
    fn drop(&mut self) {
        if fs::write(&self.0, b"release").is_err() || !self.0.with_file_name("entered").exists() {
            return;
        }
        let completed = self.0.with_file_name("completed");
        let watchdog = std::time::Instant::now() + crate::test_wait::HANG_GUARD;
        while !completed.exists() {
            if std::time::Instant::now() >= watchdog {
                eprintln!("Git descendant fixture did not finish after gate release");
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(windows)]
fn terminate_proxy(pid: u32) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
            WaitForSingleObject,
        },
    };
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!handle.is_null(), "open the owned Git proxy");
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    assert_ne!(unsafe { TerminateProcess(handle.as_raw_handle(), 1) }, 0);
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 60_000) },
        WAIT_OBJECT_0,
        "owned Git proxy exit watchdog"
    );
}

#[cfg(unix)]
fn terminate_proxy(pid: u32) {
    let pid = libc::pid_t::try_from(pid).expect("owned Git process id");
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
}

/// Without a job, a killed proxy's backend keeps running, so its inherited
/// pipes keep mutation admission until it finishes.
#[cfg(unix)]
#[test]
fn inherited_stdio_keeps_mutation_admission_after_proxy_exit() {
    crate::test_wait::guarded("Git descendant settlement", async {
        let root =
            std::env::temp_dir().join(format!("sage-git-descendant-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let init = crate::git_process::fixture_git()
            .args(["init", "-q"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(init.status.success());
        fs::write(root.join("new.txt"), b"new\n").unwrap();
        let shell_root = root
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        let alias = format!(
            "alias.sage-held=!printf ready > '{shell_root}/entered'; \
             while ! test -f '{shell_root}/release'; do sleep 0.01; done; \
             git add -- new.txt; printf done > '{shell_root}/completed'"
        );
        let release = ReleaseBackend(root.join("release"));
        let harness = GitProcessTestHarness::new();
        let entered = root.join("entered");
        let mut policy =
            harness.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET);
        policy.post_reap_failure = true;
        // The post-reap failure runs after root exit. Only its descendant
        // remains behind the file gate, so a zero timeout is deterministic.
        policy.reap_timeout = Some(Duration::ZERO);
        let policy = policy.with_spawned_hook(move |pid| {
            crate::test_wait::until("Git backend entered", || entered.exists());
            terminate_proxy(pid);
        });
        let Err(failure) =
            run_git_with_policy(&root, &["-c", &alias, "sage-held"], 1_024, policy).await
        else {
            panic!("an exited proxy cannot settle its held backend");
        };
        assert!(
            matches!(
                failure,
                AppError::GitContainment {
                    state: GitContainmentState::PotentiallyRunning,
                    ..
                }
            ),
            "{failure:?}"
        );
        let Err(blocked) = run_git_with_policy(
            &root,
            &["add", "--", "new.txt"],
            1_024,
            harness.mutation_policy(crate::test_wait::BUDGET, Duration::ZERO),
        )
        .await
        else {
            panic!("the live backend must retain mutation admission");
        };
        assert!(
            matches!(blocked, AppError::GitContainment { .. }),
            "{blocked:?}"
        );
        drop(release);
        crate::test_wait::until("Git backend completed", || root.join("completed").exists());
        crate::test_wait::until("Git descendants released admission", || {
            harness.observation().unconfirmed_exits == 0
        });
        let staged = run_git_with_policy(
            &root,
            &["diff", "--cached", "--name-only"],
            1_024,
            harness.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET),
        )
        .await
        .unwrap();
        assert!(staged.status.success());
        assert_eq!(String::from_utf8(staged.stdout).unwrap().trim(), "new.txt");
        fs::remove_dir_all(root).unwrap();
    });
}

/// The same parked backend on Windows: once Sage holds the Git tree in a job,
/// losing the proxy no longer leaves the backend behind. Sage terminates the
/// whole tree, proves it, and the backend never stages anything.
#[cfg(windows)]
#[test]
fn a_killed_proxy_takes_its_parked_backend_with_it_on_windows() {
    crate::test_wait::guarded("Git tree termination", async {
        let root =
            std::env::temp_dir().join(format!("sage-git-descendant-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let init = crate::git_process::fixture_git()
            .args(["init", "-q"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(init.status.success());
        fs::write(root.join("new.txt"), b"new\n").unwrap();
        let shell_root = root
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        // Never released: only terminating the tree ends this backend.
        let alias = format!(
            "alias.sage-held=!printf ready > '{shell_root}/entered'; \
             while ! test -f '{shell_root}/release'; do sleep 0.01; done; \
             git add -- new.txt; printf done > '{shell_root}/completed'"
        );
        let harness = GitProcessTestHarness::new();
        let entered = root.join("entered");
        let mut policy =
            harness.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET);
        policy.post_reap_failure = true;
        policy.reap_timeout = Some(Duration::ZERO);
        let policy = policy.with_spawned_hook(move |pid| {
            crate::test_wait::until("Git backend entered", || entered.exists());
            terminate_proxy(pid);
        });
        let Err(failure) =
            run_git_with_policy(&root, &["-c", &alias, "sage-held"], 1_024, policy).await
        else {
            panic!("an exited proxy cannot report success for its held backend");
        };
        assert!(
            matches!(
                failure,
                AppError::GitContainment {
                    state: GitContainmentState::TerminatedUnreaped,
                    ..
                } | AppError::GitEffectUnknown(_)
            ),
            "termination of the whole tree must be proven: {failure:?}"
        );
        crate::test_wait::until("the Git tree's exit to be confirmed", || {
            harness.observation().unconfirmed_exits == 0
        });
        // Job accounting reached zero, so the parked backend is gone and can
        // never stage the File.
        let staged = run_git_with_policy(
            &root,
            &["diff", "--cached", "--name-only"],
            1_024,
            harness.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET),
        )
        .await
        .unwrap();
        assert!(staged.status.success());
        assert_eq!(String::from_utf8(staged.stdout).unwrap().trim(), "");
        assert!(!root.join("completed").exists());
        fs::remove_dir_all(root).unwrap();
    });
}
