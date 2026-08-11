use std::{fs, path::Path, process::Command};

use uuid::Uuid;

use super::*;

struct TestRepository {
    base: std::path::PathBuf,
    root: std::path::PathBuf,
}

impl TestRepository {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-git-commit-{}", Uuid::new_v4()));
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        Self { base, root }
    }

    fn workspace(&self) -> Workspace {
        Workspace::open("project-a", "root-a", &self.root).unwrap()
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn commits_only_the_staged_index_with_one_weird_explicit_message() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("partial.txt"), "base\n").unwrap();
    git(&repository.root, &["add", "--", "partial.txt"]);
    git(&repository.root, &["commit", "-q", "-m", "fixture"]);
    fs::write(repository.root.join("partial.txt"), "staged\n").unwrap();
    git(&repository.root, &["add", "--", "partial.txt"]);
    fs::write(repository.root.join("partial.txt"), "unstaged\n").unwrap();
    fs::write(repository.root.join("untracked.txt"), "not committed\n").unwrap();
    let workspace = repository.workspace();

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(
            &workspace,
            "  --allow-empty # literal\n\nUnicode Δ body  ",
        ))
        .unwrap();

    assert_eq!(
        git_stdout(&repository.root, &["show", "HEAD:partial.txt"]),
        "staged\n"
    );
    assert_eq!(
        fs::read_to_string(repository.root.join("partial.txt")).unwrap(),
        "unstaged\n"
    );
    assert!(repository.root.join("untracked.txt").exists());
    assert_eq!(
        git_stdout(&repository.root, &["log", "-1", "--format=%B"]),
        "--allow-empty # literal\n\nUnicode Δ body\n\n"
    );
}

#[test]
fn accepts_exact_utf8_message_bound_and_rejects_plus_one_blank_and_nul() {
    let exact = "é".repeat(MAX_GIT_COMMIT_MESSAGE_BYTES / 2);
    assert_eq!(normalize_message(&exact).unwrap(), exact);
    for invalid in [format!("{exact}x"), " \t\n ".into(), "bad\0message".into()] {
        assert!(matches!(
            normalize_message(&invalid),
            Err(AppError::InvalidRequest(_))
        ));
    }
}

#[test]
fn creates_the_first_commit_in_an_unborn_repository() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("first.txt"), "first\n").unwrap();
    git(&repository.root, &["add", "--", "first.txt"]);

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&repository.workspace(), "first commit"))
        .unwrap();

    assert_eq!(
        git_stdout(&repository.root, &["show", "HEAD:first.txt"]),
        "first\n"
    );
}

#[test]
fn rejects_no_staged_change_and_missing_repository_local_identity() {
    let no_staged = TestRepository::new();
    fs::write(no_staged.root.join("untracked.txt"), "untracked\n").unwrap();
    let no_staged_error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&no_staged.workspace(), "must fail"))
        .unwrap_err();
    assert!(no_staged_error.to_string().contains("staged change"));

    let no_identity = TestRepository::new();
    git(&no_identity.root, &["config", "--unset", "user.name"]);
    fs::write(no_identity.root.join("staged.txt"), "staged\n").unwrap();
    git(&no_identity.root, &["add", "--", "staged.txt"]);
    let identity_error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&no_identity.workspace(), "must fail"))
        .unwrap_err();
    assert!(
        identity_error
            .to_string()
            .contains("repository-local user.name")
    );
    assert!(git_stdout(&no_identity.root, &["status", "--porcelain"]).starts_with("A  "));
}

#[test]
fn repository_config_cannot_launch_hooks_signing_editor_or_fsmonitor() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("staged.txt"), "staged\n").unwrap();
    git(&repository.root, &["add", "--", "staged.txt"]);
    let hooks = repository.root.join(".sage-hooks");
    fs::create_dir(&hooks).unwrap();
    for hook in [
        "pre-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
        "reference-transaction",
    ] {
        write_executable(&hooks.join(hook), "#!/bin/sh\ntouch hook-ran\nexit 91\n");
    }
    write_executable(
        &repository.root.join("sentinel-tool"),
        "#!/bin/sh\ntouch tool-ran\nexit 91\n",
    );
    fs::write(
        repository.root.join(".git").join("evil.inc"),
        "[core]\n\thooksPath = ../.sage-hooks\n\tfsmonitor = ./sentinel-tool\n\teditor = ./sentinel-tool\n[commit]\n\tgpgSign = true\n[gpg]\n\tprogram = ./sentinel-tool\n[maintenance]\n\tauto = true\n[gc]\n\tauto = 1\n",
    )
    .unwrap();
    git(&repository.root, &["config", "include.path", "evil.inc"]);

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&repository.workspace(), "safe commit"))
        .unwrap();

    assert!(!repository.root.join("hook-ran").exists());
    assert!(!repository.root.join("tool-ran").exists());
    assert_eq!(
        git_stdout(&repository.root, &["log", "-1", "--format=%s"]),
        "safe commit\n"
    );
}

fn seed_then_stage(repository: &TestRepository) -> String {
    fs::write(repository.root.join("tracked.txt"), "base\n").unwrap();
    git(&repository.root, &["add", "--", "tracked.txt"]);
    git(&repository.root, &["commit", "-q", "-m", "fixture"]);
    fs::write(repository.root.join("tracked.txt"), "staged\n").unwrap();
    git(&repository.root, &["add", "--", "tracked.txt"]);
    git_stdout(&repository.root, &["rev-parse", "HEAD"])
        .trim()
        .into()
}

#[test]
fn rejects_detached_and_unfinished_history_workflows_before_commit() {
    let detached = TestRepository::new();
    let detached_head = seed_then_stage(&detached);
    git(&detached.root, &["checkout", "--detach", "-q"]);
    let detached_error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&detached.workspace(), "must fail"))
        .unwrap_err();
    assert!(detached_error.to_string().contains("detached"));
    assert_eq!(
        git_stdout(&detached.root, &["rev-parse", "HEAD"]).trim(),
        detached_head
    );

    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
    ] {
        let repository = TestRepository::new();
        let head = seed_then_stage(&repository);
        fs::write(
            repository.root.join(".git").join(marker),
            format!("{head}\n"),
        )
        .unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(commit_staged(&repository.workspace(), "must fail"))
            .unwrap_err();
        assert!(
            error.to_string().contains("unfinished"),
            "{marker}: {error}"
        );
        assert_eq!(
            git_stdout(&repository.root, &["rev-parse", "HEAD"]).trim(),
            head
        );
    }
}

#[test]
fn timeout_is_effect_unknown_and_repository_state_stops_changing_after_reap() {
    let repository = TestRepository::new();
    let before = seed_then_stage(&repository);
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let error = runtime
        .block_on(commit_staged_with_timeout(
            &repository.workspace(),
            "timeout candidate",
            Duration::ZERO,
        ))
        .unwrap_err();
    assert!(matches!(error, AppError::GitEffectUnknown(_)));
    let after_reap = git_stdout(&repository.root, &["rev-parse", "HEAD"]);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        git_stdout(&repository.root, &["rev-parse", "HEAD"]),
        after_reap
    );
    let subject = git_stdout(&repository.root, &["log", "-1", "--format=%s"]);
    assert!(
        (after_reap.trim() == before && subject == "fixture\n")
            || (after_reap.trim() != before && subject == "timeout candidate\n")
    );
}

#[test]
fn unconfirmed_exit_returns_containment_and_keeps_mutation_admission_charged() {
    let repository = TestRepository::new();
    seed_then_stage(&repository);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (policy, reaper) =
            repository::GitProcessPolicy::terminated_unreaped_withheld_wait(Duration::ZERO);

        let error =
            commit_staged_with_policy(&repository.workspace(), "contained candidate", policy)
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            AppError::GitContainment {
                state: crate::error::GitContainmentState::TerminatedUnreaped,
                ..
            }
        ));

        let blocked = repository::run_git_with_policy(
            &repository.root,
            &["status", "--short"],
            256,
            reaper.mutation_policy(Duration::from_secs(3), Duration::from_millis(10)),
        )
        .await;
        assert!(matches!(blocked, Err(AppError::GitContainment { .. })));

        reaper.release_wait();
        reaper.wait_for_exit(Duration::from_secs(1)).await.unwrap();
    });
}
