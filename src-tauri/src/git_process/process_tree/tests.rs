use std::{fs, path::Path, time::Duration};

use super::super::{GitProcessPolicy, repository_config, run};
use super::MAX_PARALLEL_JOBS;

fn git(root: &Path, args: &[&str]) {
    let status = crate::git_process::fixture_git()
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Sage Test",
            "-c",
            "user.email=sage@example.invalid",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

const FILES: usize = 160;

/// Switches through Sage, under containment, to a branch that changes every
/// one of [`FILES`] files, with `checkout.workers` set to `workers` (or left
/// unset), and returns the caps admission keeps for the repository.
fn wide_switch(workers: Option<&str>) -> Vec<String> {
    let root = std::env::temp_dir().join(format!("sage-git-workers-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    for index in 0..FILES {
        fs::write(root.join(format!("f{index:03}.txt")), b"main\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "main"]);
    git(&root, &["switch", "-q", "-c", "wide"]);
    for index in 0..FILES {
        fs::write(root.join(format!("f{index:03}.txt")), b"wide\n").unwrap();
    }
    git(&root, &["commit", "-q", "-a", "-m", "wide"]);
    git(&root, &["switch", "-q", "main"]);
    if let Some(workers) = workers {
        git(&root, &["config", "checkout.workers", workers]);
    }
    // Parallel checkout applies from the first changed file.
    git(&root, &["config", "checkout.thresholdForParallelism", "1"]);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let output = runtime
        .block_on(run(
            &root,
            &["switch", "-q", "wide"],
            64 * 1024,
            GitProcessPolicy::mutation(Duration::from_secs(60)),
        ))
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for index in [0, FILES - 1] {
        assert_eq!(
            fs::read(root.join(format!("f{index:03}.txt"))).unwrap(),
            b"wide\n"
        );
    }
    let cached = runtime
        .block_on(repository_config::admit(&root, None))
        .unwrap()
        .expect("the repository is admitted")
        .parallel_overrides()
        .collect::<Vec<_>>();
    fs::remove_dir_all(root).unwrap();
    cached
}

/// `checkout.workers=32` starts one worker per slot as soon as enough files
/// change. Under the job, Sage's cap must win, or the tree's process limit
/// makes Git abort the switch.
#[test]
fn a_wide_parallel_checkout_succeeds_inside_the_job() {
    assert!(wide_switch(Some("32")).contains(&format!("checkout.workers={MAX_PARALLEL_JOBS}")));
}

/// A repository that leaves `checkout.workers` unset is pinned to Git's own
/// default of one worker: Sage caps, it never raises.
#[test]
fn an_unset_worker_count_keeps_gits_single_worker() {
    assert!(wide_switch(None).contains(&"checkout.workers=1".to_owned()));
}
