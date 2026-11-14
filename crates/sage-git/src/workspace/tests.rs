use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use uuid::Uuid;

use super::*;

fn git(root: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn pre_spawn_root_rebind_never_redirects_child_path_resolution() {
    let base = std::env::temp_dir().join(format!("sage-git-pre-spawn-{}", Uuid::new_v4()));
    let root = base.join("repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "sage@example.invalid"]);
    git(&root, &["config", "user.name", "Sage Test"]);
    fs::write(root.join("tracked.txt"), "first\n").unwrap();
    git(&root, &["add", "--", "tracked.txt"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let first = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    let branch = git(&root, &["symbolic-ref", "HEAD"]).trim().to_owned();
    fs::write(root.join("tracked.txt"), "second\n").unwrap();
    git(&root, &["commit", "-qam", "second"]);
    let second = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&root, &["update-ref", &branch, &first, &second]);
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    let moved = base.join("original");
    let swapped = Arc::new(AtomicBool::new(false));
    let hook_root = root.clone();
    let hook_moved = moved.clone();
    let hook_swapped = Arc::clone(&swapped);
    let policy =
        GitProcessPolicy::mutation(Duration::from_secs(3)).with_before_spawn_hook(move || {
            if fs::rename(&hook_root, &hook_moved).is_ok() {
                hook_swapped.store(true, Ordering::SeqCst);
                fs::create_dir(&hook_root).unwrap();
                git(&hook_root, &["init", "-q"]);
                git(
                    &hook_root,
                    &["config", "user.email", "replacement@example.invalid"],
                );
                git(&hook_root, &["config", "user.name", "Replacement"]);
                fs::write(hook_root.join("replacement.txt"), "replacement\n").unwrap();
                git(&hook_root, &["add", "--", "replacement.txt"]);
                git(&hook_root, &["commit", "-q", "-m", "replacement"]);
            }
        });

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy(
            &workspace,
            &[
                "update-ref",
                branch.as_str(),
                second.as_str(),
                first.as_str(),
            ],
            256,
            policy,
        ));

    if swapped.load(Ordering::SeqCst) {
        assert!(matches!(result, Err(AppError::GitEffectUnknown(_))));
        assert_eq!(git(&moved, &["rev-parse", "HEAD"]).trim(), second);
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]), "replacement\n");
    } else {
        crate::ensure_success(&result.unwrap()).unwrap();
        assert_eq!(git(&root, &["rev-parse", "HEAD"]).trim(), second);
    }
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn pre_spawn_root_rebind_never_runs_the_replacement_repository_config() {
    let base = std::env::temp_dir().join(format!("sage-git-pre-spawn-config-{}", Uuid::new_v4()));
    let root = base.join("repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    let moved = base.join("original");
    let sentinel = base.join("filter-ran");
    let filter = format!(
        "touch \"{}\"; cat",
        sentinel.to_string_lossy().replace('\\', "/")
    );
    let swapped = Arc::new(AtomicBool::new(false));
    let hook_root = root.clone();
    let hook_moved = moved.clone();
    let hook_swapped = Arc::clone(&swapped);
    let policy = GitProcessPolicy::read(Duration::from_secs(3)).with_before_spawn_hook(move || {
        // Runs after admission inspected the original repository.
        if fs::rename(&hook_root, &hook_moved).is_ok() {
            hook_swapped.store(true, Ordering::SeqCst);
            fs::create_dir(&hook_root).unwrap();
            git(&hook_root, &["init", "-q"]);
            git(
                &hook_root,
                &["config", "user.email", "replacement@example.invalid"],
            );
            git(&hook_root, &["config", "user.name", "Replacement"]);
            fs::write(hook_root.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
            fs::write(hook_root.join("a.txt"), "a\n").unwrap();
            git(&hook_root, &["add", "--", ".gitattributes", "a.txt"]);
            git(&hook_root, &["commit", "-q", "-m", "replacement"]);
            git(&hook_root, &["config", "filter.sentinel.clean", &filter]);
            // Same size as the committed content, so status has to hash it.
            fs::write(hook_root.join("a.txt"), "b\n").unwrap();
        }
    });

    let _ = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy(
            &workspace,
            &["status", "--porcelain"],
            64 * 1024,
            policy,
        ));

    if swapped.load(Ordering::SeqCst) {
        assert!(
            !sentinel.exists(),
            "the replacement repository's filter ran"
        );
        // Control: Git does run the replacement's filter when unpinned.
        git(&root, &["status", "--porcelain"]);
        assert!(sentinel.exists(), "fixture filter is not live");
    }
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn moving_a_linked_worktree_never_reaches_a_neighbouring_git_dir() {
    let base = std::env::temp_dir().join(format!("sage-git-linked-move-{}", Uuid::new_v4()));
    let safe = base.join("safe");
    let attacker = base.join("attacker");
    for repository in [&safe, &attacker] {
        let main = repository.join("main");
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["config", "user.email", "sage@example.invalid"]);
        git(&main, &["config", "user.name", "Sage Test"]);
        fs::write(main.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
        fs::write(main.join("a.txt"), "a\n").unwrap();
        git(&main, &["add", "--", ".gitattributes", "a.txt"]);
        git(&main, &["commit", "-q", "-m", "fixture"]);
    }
    let base = base.canonicalize().unwrap();
    let (safe, attacker) = (base.join("safe"), base.join("attacker"));
    let linked = safe.join("linked");
    git(
        &safe.join("main"),
        &["worktree", "add", "-q", "-b", "linked", "../linked"],
    );
    // A neighbouring git dir with the same relative shape and a hostile filter.
    git(
        &attacker.join("main"),
        &["worktree", "add", "-q", "-b", "linked", "../decoy/linked"],
    );
    fs::remove_dir_all(attacker.join("decoy")).unwrap();
    let sentinel = base.join("filter-ran");
    let filter = format!(
        "touch \"{}\"; cat",
        sentinel.to_string_lossy().replace('\\', "/")
    );
    git(
        &attacker.join("main"),
        &["config", "filter.sentinel.clean", &filter],
    );

    let workspace = Workspace::open("project-a", "root-a", &linked).unwrap();
    let moved = attacker.join("linked");
    let swapped = Arc::new(AtomicBool::new(false));
    let (hook_from, hook_to, hook_swapped) = (linked.clone(), moved.clone(), Arc::clone(&swapped));
    let policy = GitProcessPolicy::read(Duration::from_secs(3)).with_before_spawn_hook(move || {
        // On Windows the retained root handles make the move fail.
        hook_swapped.store(fs::rename(&hook_from, &hook_to).is_ok(), Ordering::SeqCst);
    });

    let _ = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy(
            &workspace,
            &["status", "--porcelain"],
            64 * 1024,
            policy,
        ));

    // Admission pins Git to the worktree's `.git` file, whose absolute target
    // names the safe repository's git dir, so a moved worktree never reaches
    // the neighbouring one.
    assert!(!sentinel.exists(), "the neighbouring git dir's filter ran"); // Control: a git dir relative to the moved worktree reaches the decoy.
    if !swapped.load(Ordering::SeqCst) {
        fs::rename(&linked, &moved).unwrap();
    }
    let control = crate::process::fixture_git()
        .arg("-C")
        .arg(&moved)
        .args([
            "--git-dir=../main/.git/worktrees/linked",
            "--work-tree=.",
            "status",
        ])
        .output()
        .unwrap();
    assert!(control.status.success());
    assert!(sentinel.exists(), "fixture decoy is not live");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn replacing_a_nested_roots_ancestor_never_reaches_git() {
    let base = std::env::temp_dir().join(format!("sage-git-nested-swap-{}", Uuid::new_v4()));
    let repository = base.join("repo");
    let root = repository.join("sub");
    fs::create_dir_all(&root).unwrap();
    git(&repository, &["init", "-q"]);
    let base = base.canonicalize().unwrap();
    let (repository, root) = (base.join("repo"), base.join("repo").join("sub"));
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    let sentinel = base.join("filter-ran");
    let filter = format!(
        "touch \"{}\"; cat",
        sentinel.to_string_lossy().replace('\\', "/")
    );
    let spawned = Arc::new(AtomicBool::new(false));
    let (hook_repository, hook_base, hook_spawned) =
        (repository.clone(), base.clone(), Arc::clone(&spawned));
    let policy = GitProcessPolicy::read(Duration::from_secs(3)).with_before_spawn_hook(move || {
        hook_spawned.store(true, Ordering::SeqCst);
        fs::rename(&hook_repository, hook_base.join("original")).unwrap();
        fs::create_dir_all(hook_repository.join("sub")).unwrap();
        git(&hook_repository, &["init", "-q"]);
        git(
            &hook_repository,
            &["config", "filter.sentinel.clean", &filter],
        );
    });

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy(
            &workspace,
            &["status", "--porcelain"],
            64 * 1024,
            policy,
        ));

    assert!(matches!(
        result,
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::NestedRoot
        ))
    ));
    assert!(!spawned.load(Ordering::SeqCst));
    assert!(!sentinel.exists());
    // The boundary still reports the nested state rather than a failure.
    assert!(matches!(
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(crate::workspace::repository_boundary(&workspace)),
        Ok(crate::RepositoryBoundary::Nested)
    ));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_root_moved_after_admission_is_refused_before_the_spawn() {
    let base = std::env::temp_dir().join(format!("sage-git-moved-root-{}", Uuid::new_v4()));
    let root = base.join("repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(crate::workspace::run_git(&workspace, &["status"], 1024))
        .unwrap();
    if fs::rename(&root, base.join("moved")).is_ok() {
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q"]);
        let result = runtime.block_on(crate::workspace::run_git(&workspace, &["status"], 1024));
        assert!(
            matches!(result, Err(AppError::InvalidRequest(_) | AppError::Io(_))),
            "a moved root reached Git"
        );
    }
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn post_update_ref_rebind_remains_effect_unknown_and_never_targets_replacement() {
    let base = std::env::temp_dir().join(format!("sage-git-root-rebind-{}", Uuid::new_v4()));
    let root = base.join("repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "sage@example.invalid"]);
    git(&root, &["config", "user.name", "Sage Test"]);
    fs::write(root.join("tracked.txt"), "first\n").unwrap();
    git(&root, &["add", "--", "tracked.txt"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let first = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    let branch = git(&root, &["symbolic-ref", "HEAD"]).trim().to_owned();
    fs::write(root.join("tracked.txt"), "second\n").unwrap();
    git(&root, &["commit", "-qam", "second"]);
    let second = git(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    git(&root, &["update-ref", &branch, &first, &second]);
    let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
    let moved = base.join("original");

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy_inner(
            &workspace,
            &[
                "update-ref",
                branch.as_str(),
                second.as_str(),
                first.as_str(),
            ],
            256,
            GitProcessPolicy::mutation(Duration::from_secs(3)),
            || {
                fs::rename(&root, &moved).unwrap();
                fs::create_dir(&root).unwrap();
                git(&root, &["init", "-q"]);
                git(
                    &root,
                    &["config", "user.email", "replacement@example.invalid"],
                );
                git(&root, &["config", "user.name", "Replacement"]);
                fs::write(root.join("replacement.txt"), "replacement\n").unwrap();
                git(&root, &["add", "--", "replacement.txt"]);
                git(&root, &["commit", "-q", "-m", "replacement"]);
            },
        ));
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("visible mutation plus root rebind must be effect-unknown"),
    };

    assert!(matches!(error, AppError::GitEffectUnknown(_)));
    assert_eq!(git(&moved, &["rev-parse", "HEAD"]).trim(), second);
    assert_eq!(git(&root, &["log", "-1", "--format=%s"]), "replacement\n");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn reaped_pre_ref_object_write_failure_never_claims_a_visible_commit_effect() {
    let base = std::env::temp_dir().join(format!("sage-git-pre-ref-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    git(&base, &["init", "-q"]);
    let workspace = Workspace::open("project-a", "root-a", &base).unwrap();

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(run_git_with_policy(
            &workspace,
            &["hash-object", "-w", "--stdin"],
            256,
            GitProcessPolicy::pre_ref_post_reap_failure(Duration::from_secs(3)),
        ));

    assert!(matches!(result, Err(AppError::Git(_))));
    fs::remove_dir_all(base).unwrap();
}
