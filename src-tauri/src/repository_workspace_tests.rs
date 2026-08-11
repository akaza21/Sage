use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use uuid::Uuid;

use super::*;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
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
        repository::ensure_success(&result.unwrap()).unwrap();
        assert_eq!(git(&root, &["rev-parse", "HEAD"]).trim(), second);
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
