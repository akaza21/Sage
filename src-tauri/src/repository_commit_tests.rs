use std::{ffi::OsStr, fs, path::Path, process::Command};

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

fn git_succeeds(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap()
        .success()
}

fn git_path(root: &Path, marker: &str) -> std::path::PathBuf {
    git_stdout(
        root,
        &["rev-parse", "--path-format=absolute", "--git-path", marker],
    )
    .trim()
    .into()
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn ambient_fixture(root: &Path, environment: &[(&str, &OsStr)]) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "repository_commit::tests::ambient_git_commit_fixture",
            "--nocapture",
        ])
        .env("SAGE_GIT_COMMIT_AMBIENT_FIXTURE_ROOT", root);
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

#[test]
fn ambient_git_commit_fixture() {
    let Some(root) = std::env::var_os("SAGE_GIT_COMMIT_AMBIENT_FIXTURE_ROOT") else {
        return;
    };
    let workspace = Workspace::open("project-a", "root-a", Path::new(&root)).unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&workspace, "ambient isolation"))
        .unwrap();
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

#[test]
fn repository_content_filters_cannot_launch_during_commit_projection() {
    let repository = TestRepository::new();
    fs::write(
        repository.root.join(".gitattributes"),
        "*.txt filter=sentinel\n",
    )
    .unwrap();
    fs::write(repository.root.join("partial.txt"), "base\n").unwrap();
    git(
        &repository.root,
        &["add", "--", ".gitattributes", "partial.txt"],
    );
    git(&repository.root, &["commit", "-q", "-m", "fixture"]);

    fs::write(repository.root.join("partial.txt"), "staged\n").unwrap();
    git(&repository.root, &["add", "--", "partial.txt"]);
    fs::write(repository.root.join("partial.txt"), "unstaged\n").unwrap();
    let sentinel = repository.root.join("filter-ran");
    let filter = repository.root.join("sentinel-filter");
    let sentinel_path = sentinel.to_string_lossy().replace('\\', "/");
    write_executable(
        &filter,
        &format!("#!/bin/sh\necho ran > \"{sentinel_path}\"\n/usr/bin/cat\n"),
    );
    let filter_command = format!("\"{}\"", filter.to_string_lossy().replace('\\', "/"));
    git(
        &repository.root,
        &["config", "filter.sentinel.clean", &filter_command],
    );
    git(
        &repository.root,
        &["config", "filter.sentinel.required", "true"],
    );
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&repository.workspace(), "safe commit"))
        .unwrap();

    assert!(!sentinel.exists());
    assert_eq!(
        git_stdout(&repository.root, &["show", "HEAD:partial.txt"]),
        "staged\n"
    );
    assert_eq!(
        fs::read_to_string(repository.root.join("partial.txt")).unwrap(),
        "unstaged\n"
    );
}

#[test]
fn ambient_git_redirections_cannot_escape_the_selected_repository() {
    let repository = TestRepository::new();
    seed_then_stage(&repository);
    let outside = repository.base.join("outside");
    fs::create_dir(&outside).unwrap();
    git(&outside, &["init", "-q"]);
    git(
        &outside,
        &["config", "user.email", "outside@example.invalid"],
    );
    git(&outside, &["config", "user.name", "Outside"]);
    fs::write(outside.join("outside.txt"), "outside\n").unwrap();
    git(&outside, &["add", "--", "outside.txt"]);

    let hooks = repository.base.join("ambient-hooks");
    fs::create_dir(&hooks).unwrap();
    let sentinel = repository.base.join("ambient-hook-ran");
    let sentinel_path = sentinel.to_string_lossy().replace('\\', "/");
    write_executable(
        &hooks.join("post-commit"),
        &format!("#!/bin/sh\necho ran > \"{sentinel_path}\"\n"),
    );
    let config = repository.base.join("ambient.gitconfig");
    fs::write(
        &config,
        format!(
            "[core]\n\thooksPath = {}\n",
            hooks.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    let outside_git = outside.join(".git");
    let outside_index = outside_git.join("index");
    let outside_objects = outside_git.join("objects");
    let hooks_text = hooks.to_string_lossy().into_owned();
    let output = ambient_fixture(
        &repository.root,
        &[
            ("GIT_DIR", outside_git.as_os_str()),
            ("GIT_WORK_TREE", outside.as_os_str()),
            ("GIT_INDEX_FILE", outside_index.as_os_str()),
            ("GIT_OBJECT_DIRECTORY", outside_objects.as_os_str()),
            (
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                outside_objects.as_os_str(),
            ),
            ("GIT_CONFIG", config.as_os_str()),
            ("GIT_CONFIG_GLOBAL", config.as_os_str()),
            ("GIT_CONFIG_SYSTEM", config.as_os_str()),
            ("GIT_CONFIG_COUNT", OsStr::new("1")),
            ("GIT_CONFIG_KEY_0", OsStr::new("core.hooksPath")),
            ("GIT_CONFIG_VALUE_0", OsStr::new(&hooks_text)),
        ],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git_stdout(&repository.root, &["log", "-1", "--format=%s"]),
        "ambient isolation\n"
    );
    assert!(!git_succeeds(
        &outside,
        &["rev-parse", "--verify", "--quiet", "HEAD"]
    ));
    assert!(!sentinel.exists());
}

#[test]
fn missing_promisor_object_cannot_launch_a_lazy_fetch() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("tracked.txt"), "base\n").unwrap();
    git(&repository.root, &["add", "--", "tracked.txt"]);
    git(&repository.root, &["commit", "-q", "-m", "fixture"]);
    fs::write(repository.root.join("tracked.txt"), "staged\n").unwrap();
    git(&repository.root, &["add", "--", "tracked.txt"]);
    let missing_oid = git_stdout(&repository.root, &["rev-parse", ":tracked.txt"])
        .trim()
        .to_owned();

    let sentinel = repository.root.join("lazy-fetch-ran");
    let upload_pack = repository.root.join("sentinel-upload-pack");
    let sentinel_path = sentinel.to_string_lossy().replace('\\', "/");
    write_executable(
        &upload_pack,
        &format!("#!/bin/sh\necho ran > \"{sentinel_path}\"\nexit 91\n"),
    );
    let upload_pack_command = format!("\"{}\"", upload_pack.to_string_lossy().replace('\\', "/"));
    let root_text = repository.root.to_string_lossy().into_owned();
    git(
        &repository.root,
        &["config", "remote.origin.url", &root_text],
    );
    git(
        &repository.root,
        &["config", "remote.origin.uploadpack", &upload_pack_command],
    );
    git(
        &repository.root,
        &["config", "remote.origin.promisor", "true"],
    );
    git(
        &repository.root,
        &["config", "remote.origin.partialclonefilter", "blob:none"],
    );
    let object_path = repository
        .root
        .join(".git/objects")
        .join(&missing_oid[..2])
        .join(&missing_oid[2..]);
    fs::remove_file(object_path).unwrap();

    let control = Command::new("git")
        .arg("-C")
        .arg(&repository.root)
        .args(["cat-file", "-p", &missing_oid])
        .output()
        .unwrap();
    assert!(!control.status.success());
    assert!(
        sentinel.exists(),
        "promisor fixture did not attempt its remote"
    );
    fs::remove_file(&sentinel).unwrap();

    let error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(&repository.workspace(), "must not fetch"))
        .unwrap_err();
    assert!(!error.to_string().is_empty());
    assert!(!sentinel.exists());
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
fn ordinary_branches_named_like_workflow_markers_remain_committable() {
    let repository = TestRepository::new();
    seed_then_stage(&repository);
    for branch in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
    ] {
        git(&repository.root, &["branch", branch]);
    }

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_staged(
            &repository.workspace(),
            "marker branches are ordinary refs",
        ))
        .unwrap();

    assert_eq!(
        git_stdout(&repository.root, &["log", "-1", "--format=%s"]),
        "marker branches are ordinary refs\n"
    );
}

#[test]
fn rejects_worktree_git_path_files_and_directories() {
    let repository = TestRepository::new();
    fs::write(repository.root.join("tracked.txt"), "base\n").unwrap();
    git(&repository.root, &["add", "--", "tracked.txt"]);
    git(&repository.root, &["commit", "-q", "-m", "fixture"]);
    let linked = repository.base.join("linked");
    let linked_text = linked.to_string_lossy().into_owned();
    git(
        &repository.root,
        &["worktree", "add", "-q", "-b", "linked-branch", &linked_text],
    );
    fs::write(linked.join("tracked.txt"), "staged\n").unwrap();
    git(&linked, &["add", "--", "tracked.txt"]);
    let workspace = Workspace::open("project-a", "root-a", &linked).unwrap();
    let head = git_stdout(&linked, &["rev-parse", "HEAD"]);

    for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        let path = git_path(&linked, marker);
        fs::write(&path, &head).unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(commit_staged(&workspace, "must fail"))
            .unwrap_err();
        assert!(
            error.to_string().contains("unfinished"),
            "{marker}: {error}"
        );
        fs::remove_file(path).unwrap();
    }
    for marker in ["rebase-merge", "rebase-apply"] {
        let path = git_path(&linked, marker);
        fs::create_dir(&path).unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(commit_staged(&workspace, "must fail"))
            .unwrap_err();
        assert!(
            error.to_string().contains("unfinished"),
            "{marker}: {error}"
        );
        fs::remove_dir(path).unwrap();
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
