use std::{fs, path::Path, process::Command, time::Duration};

use super::super::{GitProcessPolicy, run};

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
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

/// `checkout.workers=32` starts one worker per slot as soon as at least
/// 100 files change. Under the job, Sage's own bound must win, or the
/// tree's process limit makes Git abort the switch.
#[test]
fn a_wide_parallel_checkout_succeeds_inside_the_job() {
    let root = std::env::temp_dir().join(format!("sage-git-workers-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    let files = 160;
    for index in 0..files {
        fs::write(root.join(format!("f{index:03}.txt")), b"main\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "main"]);
    git(&root, &["switch", "-q", "-c", "wide"]);
    for index in 0..files {
        fs::write(root.join(format!("f{index:03}.txt")), b"wide\n").unwrap();
    }
    git(&root, &["commit", "-q", "-a", "-m", "wide"]);
    git(&root, &["switch", "-q", "main"]);
    git(&root, &["config", "checkout.workers", "32"]);
    git(&root, &["config", "checkout.thresholdForParallelism", "1"]);

    let output = tokio::runtime::Runtime::new()
        .unwrap()
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
    for index in [0, files - 1] {
        assert_eq!(
            fs::read(root.join(format!("f{index:03}.txt"))).unwrap(),
            b"wide\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}
