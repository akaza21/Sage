//! Branch and change count for each repository of a folder-of-repositories
//! list. One porcelain status per repository through the bounded Git runner,
//! with at most `MAX_CONCURRENT_SUMMARIES` in flight.

use sage_core::error::AppResult;
use sage_domain::repository::{RepositoryRef, RepositorySummary};
use sage_workspace::Workspace;
use tokio::task::JoinSet;

use crate::{
    RepositoryBoundary, ensure_success, parse::parse_status, repository_boundary, run_git,
};

pub const MAX_CONCURRENT_SUMMARIES: usize = 4;
const MAX_SUMMARY_STATUS_BYTES: usize = 4 * 1024 * 1024;

/// Summaries in input order. A repository whose workspace could not open or
/// whose status failed is listed as `unavailable` with the reason.
pub async fn summaries(
    repositories: Vec<(RepositoryRef, AppResult<Workspace>)>,
) -> Vec<RepositorySummary> {
    let mut results: Vec<Option<RepositorySummary>> = vec![None; repositories.len()];
    let mut pending = repositories.into_iter().enumerate();
    let mut running = JoinSet::new();
    loop {
        while running.len() < MAX_CONCURRENT_SUMMARIES {
            let Some((index, (repository, workspace))) = pending.next() else {
                break;
            };
            match workspace {
                Ok(workspace) => {
                    running.spawn(async move {
                        let summary = match summary(&repository, &workspace).await {
                            Ok(summary) => summary,
                            Err(error) => unavailable(repository, error.to_string()),
                        };
                        (index, summary)
                    });
                }
                Err(error) => results[index] = Some(unavailable(repository, error.to_string())),
            }
        }
        let Some(joined) = running.join_next().await else {
            break;
        };
        if let Ok((index, summary)) = joined {
            results[index] = Some(summary);
        }
    }
    results.into_iter().flatten().collect()
}

async fn summary(
    repository: &RepositoryRef,
    workspace: &Workspace,
) -> AppResult<RepositorySummary> {
    if !matches!(
        repository_boundary(workspace.root()).await?,
        RepositoryBoundary::Exact
    ) {
        return Ok(unavailable(
            repository.clone(),
            "This folder is not a Git repository top level.".into(),
        ));
    }
    let output = run_git(
        workspace.root(),
        &[
            "status",
            "--branch",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ],
        MAX_SUMMARY_STATUS_BYTES,
    )
    .await?;
    ensure_success(&output)?;
    let parsed = parse_status(workspace.root_id(), &output.stdout, output.stdout_truncated)?;
    Ok(RepositorySummary {
        r#ref: repository.clone(),
        state: "repository".into(),
        branch: parsed.branch,
        change_count: parsed.changes.len() as u64,
        changes_overflowed: parsed.overflowed || parsed.truncated,
        message: None,
    })
}

fn unavailable(repository: RepositoryRef, message: String) -> RepositorySummary {
    RepositorySummary {
        r#ref: repository,
        state: "unavailable".into(),
        branch: None,
        change_count: 0,
        changes_overflowed: false,
        message: Some(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn git(root: &Path, args: &[&str]) {
        let output = crate::process::fixture_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository(path: &Path, branch: &str) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q", "-b", branch]);
        git(path, &["config", "user.email", "sage@example.invalid"]);
        git(path, &["config", "user.name", "Sage Test"]);
        fs::write(path.join("a.txt"), "one\n").unwrap();
        fs::write(path.join("b.txt"), "one\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", "initial"]);
    }

    fn entry(root: &Path, relative: &str) -> (RepositoryRef, AppResult<Workspace>) {
        let path = root.join(relative).canonicalize().unwrap();
        (
            RepositoryRef {
                root_id: "root-a".into(),
                relative_path: relative.into(),
            },
            Workspace::open("project-a", "root-a", &path),
        )
    }

    #[tokio::test]
    async fn folder_of_repositories_lists_branches_and_change_counts() {
        let base = std::env::temp_dir().join(format!("sage-summary-{}", uuid::Uuid::new_v4()));
        repository(&base.join("api"), "main");
        repository(&base.join("web"), "develop");
        fs::create_dir_all(base.join("notes")).unwrap();
        repository(&base.join("group/deep"), "main");
        repository(&base.join("detached"), "main");
        fs::write(base.join("api/a.txt"), "two\n").unwrap();
        fs::write(base.join("api/new.txt"), "new\n").unwrap();
        fs::write(base.join("web/b.txt"), "two\n").unwrap();
        git(&base.join("detached"), &["checkout", "-q", "--detach"]);

        let discovered = crate::discovery::discover_root(
            "root-a",
            &base.canonicalize().unwrap(),
            crate::discovery::MAX_REPOSITORIES_PER_PROJECT,
        );
        let paths = discovered
            .refs
            .iter()
            .map(|r| r.relative_path.clone())
            .collect::<Vec<_>>();
        let listed = summaries(paths.iter().map(|p| entry(&base, p)).collect()).await;
        let _ = fs::remove_dir_all(&base);

        assert!(discovered.child_repositories);
        assert_eq!(paths, ["api", "detached", "web"]);
        let rows = listed
            .iter()
            .map(|s| {
                (
                    s.r#ref.relative_path.as_str(),
                    s.state.as_str(),
                    s.change_count,
                    s.changes_overflowed,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                ("api", "repository", 2, false),
                ("detached", "repository", 0, false),
                ("web", "repository", 1, false),
            ]
        );
        assert_eq!(listed[0].branch.as_deref(), Some("main"));
        assert!(
            listed[1]
                .branch
                .as_deref()
                .is_some_and(|branch| branch.ends_with(" (detached)")),
            "{:?}",
            listed[1].branch
        );
        assert_eq!(listed[2].branch.as_deref(), Some("develop"));
    }

    #[tokio::test]
    async fn unopenable_repository_is_listed_as_unavailable() {
        let base = std::env::temp_dir().join(format!("sage-summary-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(base.join("plain")).unwrap();
        let listed = summaries(vec![
            entry(&base, "plain"),
            (
                RepositoryRef {
                    root_id: "root-a".into(),
                    relative_path: "gone".into(),
                },
                Err(sage_core::error::AppError::InvalidRequest(
                    "selected repository path does not exist".into(),
                )),
            ),
        ])
        .await;
        let _ = fs::remove_dir_all(&base);

        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|s| s.state == "unavailable"));
        assert!(
            listed[1]
                .message
                .as_deref()
                .is_some_and(|m| m.contains("does not exist"))
        );
    }
}
