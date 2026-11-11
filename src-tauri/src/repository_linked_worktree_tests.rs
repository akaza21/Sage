//! A Project opened from a linked worktree (`git worktree add`) is a
//! repository like any other: Source Control reports its branch and changes,
//! and the config it shares with the main worktree is still admitted first.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{error::AppError, workspace::Workspace};

const ROOT_ID: &str = "root-a";

/// A main worktree with one commit and a linked worktree `linked` on branch
/// `topic`, both under one scratch folder.
struct Fixture {
    base: PathBuf,
    main: PathBuf,
    linked: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-linked-worktree-{}", Uuid::new_v4()));
        fs::create_dir_all(base.join("main")).unwrap();
        let base = base.canonicalize().unwrap();
        let main = base.join("main");
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.email", "sage@example.invalid"]);
        git(&main, &["config", "user.name", "Sage Test"]);
        fs::write(main.join("a.txt"), "a\n").unwrap();
        git(&main, &["add", "--", "a.txt"]);
        git(&main, &["commit", "-q", "-m", "fixture"]);
        // Relative operand: Git for Windows rejects verbatim paths here.
        git(
            &main,
            &["worktree", "add", "-q", "-b", "topic", "../linked"],
        );
        let linked = base.join("linked");
        Self { base, main, linked }
    }

    fn workspace(&self) -> Workspace {
        Workspace::open("project-a", ROOT_ID, &self.linked).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = crate::git_process::fixture_git()
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

#[test]
fn a_linked_worktree_reports_its_branch_and_changes() {
    let fixture = Fixture::new();
    fs::write(fixture.linked.join("a.txt"), "changed\n").unwrap();
    fs::write(fixture.linked.join("b.txt"), "new\n").unwrap();

    let projection = block_on(crate::repository::project_status(vec![(
        ROOT_ID.into(),
        Ok(fixture.workspace()),
    )]));
    let root = &projection.roots[0];
    assert_eq!(root.state, "repository", "{:?}", root.message);
    assert_eq!(root.branch.as_deref(), Some("topic"));
    let mut changes = root
        .changes
        .iter()
        .map(|change| {
            (
                change.resource.relative_path.as_str(),
                change.worktree_status.as_str(),
            )
        })
        .collect::<Vec<_>>();
    changes.sort_unstable();
    assert_eq!(changes, [("a.txt", "modified"), ("b.txt", "untracked")]);
    assert!(root.branches.iter().any(|branch| branch == "main"));
}

#[test]
fn a_linked_worktree_is_refused_when_the_shared_config_runs_a_command() {
    let fixture = Fixture::new();
    fs::write(
        fixture.linked.join(".gitattributes"),
        "*.txt filter=sentinel\n",
    )
    .unwrap();
    fs::write(fixture.linked.join("a.txt"), "changed\n").unwrap();
    let sentinel = fixture.base.join("filter-ran");
    let filter = format!(
        "touch \"{}\"; cat",
        sentinel.to_string_lossy().replace('\\', "/")
    );
    // The common dir's `config`, which every worktree of the repository reads.
    git(&fixture.main, &["config", "filter.sentinel.clean", &filter]);

    match block_on(crate::repository::status(&fixture.workspace())) {
        Err(AppError::UnsafeRepositoryConfig(message)) => {
            assert!(message.contains("filter.sentinel.clean"), "{message}");
        }
        Err(error) => panic!("expected a config refusal, got {error}"),
        Ok(_) => panic!("expected a config refusal, Git ran"),
    }
    assert!(!sentinel.exists(), "the shared filter ran");
    // Control: plain Git in the linked worktree runs the shared filter.
    git(&fixture.linked, &["add", "--", "a.txt"]);
    assert!(sentinel.exists(), "the shared filter is not live");
}

/// The layout the bug was reported from: the main repository's own config
/// enables per-worktree config and sets a shell credential helper. The
/// helper is a command-running key, so admission refuses every worktree of
/// that repository by design; the root reports why.
#[test]
fn a_shared_credential_helper_is_reported_as_the_reason_git_is_unavailable() {
    let fixture = Fixture::new();
    git(
        &fixture.main,
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        &fixture.main,
        &[
            "config",
            "credential.helper",
            "!f() { touch helper-ran; }; f",
        ],
    );

    let projection = block_on(crate::repository::project_status(vec![(
        ROOT_ID.into(),
        Ok(fixture.workspace()),
    )]));
    let root = &projection.roots[0];
    assert_eq!(root.state, "unavailable");
    let message = root.message.as_deref().unwrap_or_default();
    assert!(message.contains("credential.helper"), "{message}");
    assert!(!fixture.linked.join("helper-ran").exists());
}

#[test]
fn a_linked_worktree_is_refused_when_its_worktree_config_runs_a_command() {
    let fixture = Fixture::new();
    git(
        &fixture.main,
        &["config", "extensions.worktreeConfig", "true"],
    );
    git(
        &fixture.linked,
        &["config", "--worktree", "diff.sentinel.command", "false"],
    );

    match block_on(crate::repository::status(&fixture.workspace())) {
        Err(AppError::UnsafeRepositoryConfig(message)) => {
            assert!(message.contains("diff.sentinel.command"), "{message}");
        }
        Err(error) => panic!("expected a config refusal, got {error}"),
        Ok(_) => panic!("expected a config refusal, Git ran"),
    }
}
