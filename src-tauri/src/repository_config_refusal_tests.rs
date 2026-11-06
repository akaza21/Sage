//! Every Source Control entry point refuses a repository whose own config
//! names a program, before Git can launch it.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use uuid::Uuid;

use crate::{
    domain::{GitFileMutationAction, ResourceRef},
    error::{AppError, AppResult},
    workspace::Workspace,
};

const ROOT_ID: &str = "root-a";

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    sentinel: PathBuf,
    tool: String,
}

impl Fixture {
    /// A repository with a commit, a second branch and a local remote, so
    /// each operation would reach Git if the config were not refused.
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-git-refusal-{}", Uuid::new_v4()));
        let remote = base.join("remote.git");
        let root = base.join("repo");
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&root).unwrap();
        let base = base.canonicalize().unwrap();
        let root = root.canonicalize().unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(
            root.join(".gitattributes"),
            "*.txt filter=sentinel diff=sentinel merge=sentinel\n",
        )
        .unwrap();
        fs::write(root.join("a.txt"), "a\n").unwrap();
        git(&root, &["add", "--", ".gitattributes", "a.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        git(&root, &["branch", "other"]);
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                &remote.to_string_lossy().replace('\\', "/"),
            ],
        );
        fs::write(root.join("a.txt"), "changed\n").unwrap();

        let sentinel = base.join("tool-ran");
        let script = base.join("sentinel-tool");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\necho ran > \"{}\"\nexit 91\n",
                sentinel.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let tool = format!("\"{}\"", script.to_string_lossy().replace('\\', "/"));
        Self {
            base,
            root,
            sentinel,
            tool,
        }
    }

    fn configure(&self, key: &str) {
        git(&self.root, &["config", key, &self.tool]);
    }

    fn workspace(&self) -> Workspace {
        Workspace::open("project-a", ROOT_ID, &self.root).unwrap()
    }

    fn assert_refused(&self, key: &str, result: AppResult<()>) {
        match result {
            Err(AppError::UnsafeRepositoryConfig(message)) => {
                assert!(message.contains(&key.to_ascii_lowercase()), "{message}");
            }
            Err(error) => panic!("{key}: expected a config refusal, got {error}"),
            Ok(()) => panic!("{key}: expected a config refusal, the operation ran"),
        }
        assert!(!self.sentinel.exists(), "{key}: the configured tool ran");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
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
fn status_reports_the_refusal_as_the_root_message() {
    let fixture = Fixture::new();
    fixture.configure("filter.sentinel.clean");
    let workspace = fixture.workspace();
    fixture.assert_refused(
        "filter.sentinel.clean",
        block_on(crate::repository::status(&workspace)).map(drop),
    );

    let projection = block_on(crate::repository::project_status(vec![(
        ROOT_ID.into(),
        Ok(fixture.workspace()),
    )]));
    let root = &projection.roots[0];
    assert_eq!(root.state, "unavailable");
    assert!(
        root.message
            .as_deref()
            .is_some_and(|message| message.contains("filter.sentinel.clean"))
    );
    assert!(!fixture.sentinel.exists());
}

#[test]
fn stage_refuses_a_clean_filter() {
    let fixture = Fixture::new();
    fixture.configure("filter.sentinel.clean");
    let resource = ResourceRef {
        root_id: ROOT_ID.into(),
        relative_path: "a.txt".into(),
    };
    fixture.assert_refused(
        "filter.sentinel.clean",
        block_on(crate::repository_mutation::mutate_file(
            &fixture.workspace(),
            &resource,
            GitFileMutationAction::Stage,
        )),
    );
}

#[test]
fn checkout_refuses_a_smudge_filter() {
    let fixture = Fixture::new();
    fixture.configure("filter.sentinel.smudge");
    fixture.assert_refused(
        "filter.sentinel.smudge",
        block_on(crate::repository_checkout::checkout_listed_local_branch(
            &fixture.workspace(),
            "other",
        )),
    );
}

#[test]
fn cherry_pick_continue_refuses_a_merge_driver() {
    let fixture = Fixture::new();
    fixture.configure("merge.sentinel.driver");
    fixture.assert_refused(
        "merge.sentinel.driver",
        block_on(crate::repository_conflict::continue_cherry_pick(
            &fixture.workspace(),
        )),
    );
}

#[test]
fn fetch_refuses_an_upload_pack_command() {
    let fixture = Fixture::new();
    fixture.configure("remote.origin.uploadpack");
    fixture.assert_refused(
        "remote.origin.uploadpack",
        block_on(crate::repository_remote_transport::fetch_listed_remote(
            &fixture.workspace(),
            "origin",
        )),
    );
}

#[test]
fn push_refuses_an_ssh_command() {
    let fixture = Fixture::new();
    fixture.configure("core.sshCommand");
    fixture.assert_refused(
        "core.sshCommand",
        block_on(crate::repository_remote_transport::push_listed_remote(
            &fixture.workspace(),
            "origin",
        )),
    );
}
