use std::{fs, path::Path};

use super::*;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_domain::resource::ResourceRef;
use sage_git::process::GitCommandCounts;

fn git(root: &Path, args: &[&str]) {
    assert!(
        sage_git::process::fixture_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn three_hundred_paths_use_one_preflight_one_mutation_and_one_refresh() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture",
                "commit",
                "--allow-empty",
                "-qm",
                "base",
            ],
        );
        let database = crate::database::open_database(&base.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = database
            .bootstrap_snapshot()
            .unwrap()
            .project
            .unwrap()
            .roots[0]
            .id
            .clone();
        let resources: Vec<_> = (0..300)
            .map(|index| {
                let path = format!("file-{index}.txt");
                fs::write(root.join(&path), "base\n").unwrap();
                ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: path,
                }
            })
            .collect();
        git(&root, &["add", "--all"]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture",
                "commit",
                "-qm",
                "files",
            ],
        );
        for resource in &resources {
            fs::write(root.join(&resource.relative_path), "changed\n").unwrap();
        }
        let state = AppState::new(database);
        for (action, command, expected_index, expected_worktree) in [
            (GitFileMutationAction::Stage, "add", "modified", "unchanged"),
            (
                GitFileMutationAction::Unstage,
                "reset",
                "unchanged",
                "modified",
            ),
            (
                GitFileMutationAction::Discard,
                "restore",
                "unchanged",
                "unchanged",
            ),
        ] {
            let counts = GitCommandCounts::observe(&root);
            let result = mutate_git_paths_inner(
                &state,
                GitPathsMutationRequest {
                    project_id: project.id.clone(),
                    resources: resources.clone(),
                    discard_confirmed: action == GitFileMutationAction::Discard,
                },
                action,
                0,
            )
            .await
            .unwrap();
            assert!(result.error.is_none());
            assert_eq!(counts.count("status"), 2, "one preflight and one refresh");
            assert_eq!(
                counts.count(command),
                1,
                "one mutating Git process for all paths"
            );
            let changes = &result.repositories.roots[0].changes;
            if action == GitFileMutationAction::Discard {
                assert!(changes.is_empty());
                assert_eq!(
                    fs::read_to_string(root.join("file-299.txt")).unwrap(),
                    "base\n"
                );
            } else {
                assert_eq!(changes.len(), 300);
                assert!(
                    changes
                        .iter()
                        .all(|change| change.index_status == expected_index
                            && change.worktree_status == expected_worktree)
                );
            }
        }
        drop(state);
        fs::remove_dir_all(base).unwrap();
    });
}

#[test]
fn rejected_requests_never_mutate_and_a_failed_git_process_refreshes_status() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base = std::env::temp_dir().join(format!("sage-git-batch-{}", uuid::Uuid::new_v4()));
        let root = base.join("repo");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("safe.txt"), "safe\n").unwrap();
        let database = crate::database::open_database(&base.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = database
            .bootstrap_snapshot()
            .unwrap()
            .project
            .unwrap()
            .roots[0]
            .id
            .clone();
        let resource = ResourceRef {
            root_id,
            relative_path: "safe.txt".into(),
        };
        let request = GitPathsMutationRequest {
            project_id: project.id,
            resources: vec![resource.clone()],
            discard_confirmed: false,
        };
        let state = AppState::new(database);
        let counts = GitCommandCounts::observe(&root);
        for (request, action, epoch) in [
            (
                GitPathsMutationRequest {
                    resources: vec![],
                    ..request.clone()
                },
                GitFileMutationAction::Stage,
                0,
            ),
            (
                GitPathsMutationRequest {
                    resources: vec![resource.clone(); 301],
                    ..request.clone()
                },
                GitFileMutationAction::Stage,
                0,
            ),
            (
                GitPathsMutationRequest {
                    project_id: "foreign".into(),
                    ..request.clone()
                },
                GitFileMutationAction::Stage,
                0,
            ),
            (
                GitPathsMutationRequest {
                    resources: vec![
                        resource.clone(),
                        ResourceRef {
                            root_id: "foreign".into(),
                            relative_path: "safe.txt".into(),
                        },
                    ],
                    ..request.clone()
                },
                GitFileMutationAction::Stage,
                0,
            ),
            (request.clone(), GitFileMutationAction::Discard, 0),
            (request.clone(), GitFileMutationAction::Stage, 1),
        ] {
            assert!(
                mutate_git_paths_inner(&state, request, action, epoch)
                    .await
                    .is_err()
            );
        }
        assert_eq!(counts.count("status"), 0);
        assert_eq!(counts.count("add"), 0);
        fs::write(root.join(".git/index.lock"), "locked").unwrap();
        let result = mutate_git_paths_inner(&state, request, GitFileMutationAction::Stage, 0)
            .await
            .unwrap();
        let error = result
            .error
            .expect("the failed Git process must reach the caller");
        assert_eq!(error.kind, GitFileMutationErrorKind::EffectUnknown);
        assert!(error.message.contains("index.lock"), "{}", error.message);
        assert_eq!(counts.count("status"), 2);
        assert_eq!(counts.count("add"), 1);
        assert_eq!(
            result.repositories.roots[0].changes[0].worktree_status,
            "untracked"
        );
        assert_eq!(
            result.repositories.roots[0].changes[0].index_status,
            "unchanged"
        );
        assert_eq!(
            fs::read_to_string(root.join(".git/index.lock")).unwrap(),
            "locked"
        );
        assert_eq!(fs::read_to_string(root.join("safe.txt")).unwrap(), "safe\n");
        drop(state);
        fs::remove_dir_all(base).unwrap();
    });
}
