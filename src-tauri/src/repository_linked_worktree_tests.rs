//! A Project opened from a linked worktree (`git worktree add`) is a
//! repository like any other: Source Control reports its branch and changes,
//! and the config it shares with the main worktree is still admitted first.

use std::{
    fs,
    io::{ErrorKind, Read as _, Write as _},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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

/// An HTTP remote that answers every request with `401` and a Basic
/// challenge, so Git asks its credential helpers for a username.
struct Unauthorized {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}

impl Unauthorized {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let server = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let mut request = [0_u8; 4096];
                        let _ = stream.read(&mut request);
                        let _ = stream.write_all(
                            b"HTTP/1.1 401 Unauthorized\r\n\
                              WWW-Authenticate: Basic realm=\"sage\"\r\n\
                              Content-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback listener failed: {error}"),
                }
            }
        });
        Self {
            address,
            stop,
            server: Some(server),
        }
    }

    fn url(&self) -> String {
        format!("http://{}/repo", self.address)
    }
}

impl Drop for Unauthorized {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

/// The layout the bug was reported from: the main repository's own config
/// enables per-worktree config and sets shell credential helpers, plain and
/// URL-scoped. Sage clears every helper on its command line, so the
/// repository is admitted and no helper runs, even for the fetch, pull and
/// push that make Git ask for credentials.
#[test]
fn a_repository_credential_helper_is_admitted_and_never_runs() {
    let fixture = Fixture::new();
    let remote = Unauthorized::start();
    let sentinel = fixture.base.join("helper-ran");
    let helper = |name: &str| {
        format!(
            "!f() {{ echo {name} >> \"{}\"; }}; f",
            sentinel.to_string_lossy().replace('\\', "/")
        )
    };
    let main = &fixture.main;
    git(main, &["config", "extensions.worktreeConfig", "true"]);
    git(main, &["config", "credential.helper", &helper("plain")]);
    let scoped = format!("credential.{}.helper", remote.url());
    git(main, &["config", &scoped, &helper("scoped")]);
    git(main, &["remote", "add", "origin", &remote.url()]);
    // `topic` is one commit behind `origin/topic`, so a pull is admitted.
    let linked = &fixture.linked;
    git(linked, &["commit", "-q", "--allow-empty", "-m", "upstream"]);
    git(linked, &["update-ref", "refs/remotes/origin/topic", "HEAD"]);
    git(linked, &["reset", "-q", "--hard", "HEAD~1"]);
    git(linked, &["config", "branch.topic.remote", "origin"]);
    git(
        linked,
        &["config", "branch.topic.merge", "refs/heads/topic"],
    );

    let workspace = fixture.workspace();
    let status = block_on(crate::repository::status(&workspace)).unwrap();
    assert_eq!(status.state, "repository");
    assert_eq!(status.behind, 1);
    let fetch = block_on(crate::repository_remote_transport::fetch_listed_remote(
        &workspace, "origin",
    ));
    let pull = block_on(crate::repository_pull::pull_fast_forward(&workspace));
    let push = block_on(crate::repository_remote_transport::push_listed_remote(
        &workspace, "origin",
    ));
    for (operation, result) in [("fetch", fetch), ("pull", pull), ("push", push)] {
        assert!(
            matches!(result, Err(AppError::Git(_))),
            "{operation}: expected the remote to refuse, got {result:?}"
        );
    }
    assert!(!sentinel.exists(), "a repository credential helper ran");

    // Control: plain Git asks both configured helpers for this remote.
    let control = crate::git_process::fixture_git()
        .arg("-C")
        .arg(linked)
        .args(["fetch", "-q", "origin"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(!control.status.success());
    let ran = fs::read_to_string(&sentinel).unwrap_or_default();
    assert!(ran.contains("plain") && ran.contains("scoped"), "{ran:?}");
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
