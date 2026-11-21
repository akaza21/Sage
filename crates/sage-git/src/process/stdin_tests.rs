use std::fs;

use super::*;

fn repository() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sage-git-stdin-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    assert!(
        fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );
    root
}

#[test]
fn early_git_failure_preserves_stderr_when_stdin_exceeds_pipe_capacity() {
    let root = repository();
    fs::write(root.join("safe.txt"), "safe\n").unwrap();
    fs::write(root.join(".git/index.lock"), "held").unwrap();
    // Git locks the index before reading pathspecs. Exceed pipe capacity so
    // delivery fails regardless of which process the scheduler runs first.
    let policy =
        GitProcessPolicy::mutation(budget::MUTATION).with_stdin(b"safe.txt\0".repeat(128 * 1024));
    let result = sage_core::rt::block_on(run(
        &root,
        &["add", "--pathspec-from-file=-", "--pathspec-file-nul"],
        1024,
        policy,
    ));
    let error = result
        .and_then(|output| crate::ensure_success(&output))
        .unwrap_err();
    assert!(matches!(&error, AppError::Git(_)), "{error}");
    assert!(error.to_string().contains("index.lock"), "{error}");
    assert!(!root.join(".git/index").exists());
    assert_eq!(
        fs::read_to_string(root.join(".git/index.lock")).unwrap(),
        "held"
    );
    assert_eq!(fs::read_to_string(root.join("safe.txt")).unwrap(), "safe\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn successful_git_exit_does_not_hide_incomplete_stdin_delivery() {
    let root = repository();
    // `version` exits successfully without reading stdin.
    let policy = GitProcessPolicy::mutation(budget::MUTATION).with_stdin(vec![b'x'; 1024 * 1024]);
    let result = sage_core::rt::block_on(run(&root, &["version"], 1024, policy));
    let error = result
        .err()
        .expect("incomplete input must not report success");
    assert!(matches!(&error, AppError::GitEffectUnknown(_)), "{error}");
    assert!(error.to_string().contains("stdin write failed"), "{error}");
    fs::remove_dir_all(root).unwrap();
}
