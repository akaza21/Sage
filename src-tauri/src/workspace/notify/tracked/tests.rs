use super::*;
use std::{fs, path::PathBuf};

use super::super::relevance::{NoticeRelevance, Relevance};

struct Repository {
    root: PathBuf,
    git_root: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let path = crate::test_paths::temp_dir()
            .join(format!("sage-notify-tracked-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join("dist")).unwrap();
        // Sage receives canonical roots; Git's fixture operands use ordinary paths.
        let repo = Self {
            root: path.canonicalize().unwrap(),
            git_root: path,
        };
        repo.git(&["init"]);
        fs::write(repo.root.join(".gitignore"), "dist/\ntarget/\n").unwrap();
        fs::write(repo.root.join("dist/app.js"), "base\n").unwrap();
        repo.git(&["add", "-f", "--", "dist/app.js"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let output = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&self.git_root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn sustained_output_is_irrelevant_but_tracked_edits_and_directory_removals_are_relevant() {
    let repo = Repository::new();
    let mut tracked = TrackedPaths::default();
    for batch in 0..60 {
        assert!(!tracked.relevant(
            &repo.root,
            "target",
            &format!("target/debug/output-{batch}"),
            true,
        ));
        assert!(!tracked.relevant(&repo.root, "dist", "dist/generated.js", true));
        assert!(!tracked.relevant(&repo.root, "dist", "dist", false));
    }
    fs::write(repo.root.join("dist/app.js"), "edited\n").unwrap();
    assert!(tracked.relevant(&repo.root, "dist", "dist/app.js", false));
    fs::remove_file(repo.root.join("dist/app.js")).unwrap();
    assert!(tracked.relevant(&repo.root, "dist", "dist/app.js", true));
    assert!(tracked.relevant(&repo.root, "dist", "dist", true));
    assert!(!tracked.relevant(&repo.root, "dist", "dis", true));
}

#[test]
fn a_force_add_and_index_removal_refresh_cached_membership() {
    let repo = Repository::new();
    let mut tracked = TrackedPaths::default();
    assert!(!tracked.relevant(&repo.root, "dist", "dist/new.js", false));
    fs::write(repo.root.join("dist/new.js"), "new\n").unwrap();
    repo.git(&["add", "-f", "--", "dist/new.js"]);
    assert!(tracked.relevant(&repo.root, "dist", "dist/new.js", false));
    repo.git(&["rm", "--cached", "--", "dist/app.js"]);
    assert!(!tracked.relevant(&repo.root, "dist", "dist/app.js", false));
}

#[test]
fn ignored_folder_and_descendant_notices_keep_explorer_meaning_but_filter_git_output() {
    let repo = Repository::new();
    fs::create_dir_all(repo.root.join("target/debug")).unwrap();
    let mut relevance = NoticeRelevance::new(&repo.root);
    for batch in 0..60 {
        let relative = format!("target/debug/output-{batch}");
        assert_eq!(
            relevance.classify_record(&relative, 3),
            (Relevance::IgnoredBelow("target".into()), false)
        );
        assert_eq!(
            relevance.classify_record("target", 3),
            (Relevance::Keep, false)
        );
    }
    fs::write(repo.root.join("dist/app.js"), "edited\n").unwrap();
    assert_eq!(
        relevance.classify_record("dist/app.js", 3),
        (Relevance::IgnoredBelow("dist".into()), true)
    );
}

#[test]
fn ignored_directory_clean_and_recreate_stays_irrelevant_without_losing_tracked_deletions() {
    let repo = Repository::new();
    let mut relevance = NoticeRelevance::new(&repo.root);
    for _ in 0..3 {
        fs::create_dir(repo.root.join("target")).unwrap();
        assert_eq!(
            relevance.classify_record("target", 1),
            (Relevance::Keep, false)
        );
        fs::remove_dir(repo.root.join("target")).unwrap();
        assert_eq!(
            relevance.classify_record("target", 2),
            (Relevance::Keep, false)
        );
        assert_eq!(
            relevance.classify_record("target", 3),
            (Relevance::Keep, false)
        );
    }
    assert_eq!(
        relevance.classify_record("dist", 3),
        (Relevance::Keep, false)
    );
    fs::remove_dir_all(repo.root.join("dist")).unwrap();
    assert_eq!(
        relevance.classify_record("dist", 2),
        (Relevance::Keep, true)
    );
    fs::write(repo.root.join("target"), "ordinary untracked file\n").unwrap();
    assert_eq!(
        relevance.classify_record("target", 1),
        (Relevance::Keep, true)
    );
}

#[test]
fn unrelated_tracked_paths_do_not_consume_an_empty_subtrees_lookup_budget() {
    let repo = Repository::new();
    let (paths, bytes) = tauri::async_runtime::block_on(load(&repo.root, "target", 0)).unwrap();
    assert!(paths.is_empty());
    assert_eq!(bytes, 0);
}

#[test]
fn global_excludes_do_not_hide_files_reported_by_the_production_runner() {
    const CHILD_HOME: &str = "SAGE_NOTIFY_GLOBAL_EXCLUDES_HOME";
    // Git cannot read a verbatim home, so its own fixture call gets the ordinary one.
    const CHILD_GIT_HOME: &str = "SAGE_NOTIFY_GLOBAL_EXCLUDES_GIT_HOME";
    let Some(home) = std::env::var_os(CHILD_HOME) else {
        // A child gives the watcher a real synthetic user configuration
        // without racing other tests by mutating this process's environment.
        let home = Repository::new();
        // Keep exercising the resolver with a verbatim Windows home path.
        let canonical_home = &home.root;
        let excludes = home.root.join("global.ignore");
        fs::write(&excludes, "scratch/\n").unwrap();
        home.git(&[
            "config",
            "--file",
            ".gitconfig",
            "core.excludesFile",
            "~/global.ignore",
        ]);
        let test = concat!(
            module_path!(),
            "::global_excludes_do_not_hide_files_reported_by_the_production_runner"
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test.split_once("::").unwrap().1, "--nocapture"])
            .env(CHILD_HOME, canonical_home)
            .env(CHILD_GIT_HOME, &home.git_root)
            .env("HOME", canonical_home)
            .env("USERPROFILE", canonical_home)
            .env("XDG_CONFIG_HOME", home.root.join(".config"))
            .env("GIT_CONFIG_GLOBAL", home.git_root.join(".gitconfig"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?}\n{:?}",
            output.stdout,
            output.stderr
        );
        return;
    };
    assert_eq!(
        crate::workspace::ignore_rules::global_excludes_path(),
        Some(PathBuf::from(&home).join("global.ignore"))
    );
    assert_eq!(
        fs::read_to_string(crate::workspace::ignore_rules::global_excludes_path().unwrap())
            .unwrap(),
        "scratch/\n"
    );
    let repo = Repository::new();
    let global = std::env::var_os("GIT_CONFIG_GLOBAL").unwrap();
    let git_home = std::env::var_os(CHILD_GIT_HOME).unwrap();
    fs::create_dir(repo.root.join("scratch")).unwrap();
    fs::write(repo.root.join("scratch/new.txt"), "new\n").unwrap();
    let output = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&repo.git_root)
        .args(["status", "--porcelain=v2", "-z", "--untracked-files=all"])
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("HOME", &git_home)
        .env("USERPROFILE", &git_home)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(
        !output
            .stdout
            .split(|byte| *byte == 0)
            .any(|path| path == b"? scratch/new.txt")
    );
    assert_status_contains(&repo.root, "scratch/new.txt");

    let root = &repo.root;
    let mut relevance = NoticeRelevance::new(root);
    assert_eq!(
        relevance.classify_record("scratch/new.txt", 1),
        (Relevance::Keep, true)
    );
    // A repository-local override still applies in the production runner.
    fs::create_dir(repo.root.join("rules")).unwrap();
    let excludes = repo.root.join("rules").join("local.ignore");
    fs::write(&excludes, "scratch/\n").unwrap();
    repo.git(&["config", "core.excludesFile", "rules/local.ignore"]);
    let config =
        tauri::async_runtime::block_on(crate::git_process::IgnoreConfig::load(root)).unwrap();
    assert_eq!(
        config.excludes,
        Some(root.join("rules").join("local.ignore"))
    );
    assert_eq!(
        relevance.classify_record("scratch/new.txt", 1),
        (Relevance::IgnoredBelow("scratch".into()), false)
    );
    fs::write(excludes, "").unwrap();
    assert_status_contains(&repo.root, "scratch/new.txt");
    assert_eq!(
        relevance.classify_record("scratch/new.txt", 1),
        (Relevance::Keep, true)
    );
}

fn assert_status_contains(root: &Path, relative: &str) {
    let output = tauri::async_runtime::block_on(run_git(
        root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        64 * 1024,
    ))
    .unwrap();
    assert!(output.succeeded(), "{:?}", output.stderr);
    assert!(!output.stdout_truncated);
    let entry = format!("? {relative}");
    assert!(
        output
            .stdout
            .split(|byte| *byte == 0)
            .any(|path| path == entry.as_bytes())
    );
}

#[test]
fn linked_worktree_exclude_edits_invalidate_ignored_folder_decisions() {
    let repo = Repository::new();
    repo.git(&[
        "-c",
        "user.name=Sage Test",
        "-c",
        "user.email=sage@example.test",
        "commit",
        "-m",
        "base",
    ]);
    let worktree = repo.git_root.join("linked");
    repo.git(&["worktree", "add", "--detach", worktree.to_str().unwrap()]);
    let worktree = worktree.canonicalize().unwrap();
    // Only the main checkout's shared exclude file ignores this directory.
    fs::write(worktree.join(".gitignore"), "").unwrap();
    fs::write(repo.root.join(".git/info/exclude"), "dist/\n").unwrap();
    let mut relevance = NoticeRelevance::new(&worktree);
    fs::write(worktree.join("dist/first.txt"), "first\n").unwrap();
    assert_eq!(
        relevance.classify_record("dist/first.txt", 1),
        (Relevance::IgnoredBelow("dist".into()), false)
    );

    fs::write(repo.root.join(".git/info/exclude"), "").unwrap();
    fs::write(worktree.join("dist/new.txt"), "new\n").unwrap();
    assert_status_contains(&worktree, "dist/new.txt");
    assert_eq!(
        relevance.classify_record("dist/new.txt", 1),
        (Relevance::Keep, true)
    );

    // An explicit exclude file outside this worktree has the same lifetime
    // problem, including its deletion without any worktree watcher record.
    let external = repo.root.join("external.ignore");
    fs::write(&external, "cache/\n").unwrap();
    let external_operand = repo.git_root.join("external.ignore");
    repo.git(&[
        "config",
        "core.excludesFile",
        external_operand.to_str().unwrap(),
    ]);
    fs::create_dir(worktree.join("cache")).unwrap();
    fs::write(worktree.join("cache/first.txt"), "first\n").unwrap();
    assert_eq!(
        relevance.classify_record("cache/first.txt", 1),
        (Relevance::IgnoredBelow("cache".into()), false)
    );
    fs::remove_file(external).unwrap();
    fs::write(worktree.join("cache/new.txt"), "new\n").unwrap();
    assert_status_contains(&worktree, "cache/new.txt");
    assert_eq!(
        relevance.classify_record("cache/new.txt", 1),
        (Relevance::Keep, true)
    );
}

#[test]
#[cfg(windows)]
fn a_case_only_ignored_folder_rename_still_reports_tracked_edits() {
    let repo = Repository::new();
    fs::rename(repo.root.join("dist"), repo.root.join("rename-temp")).unwrap();
    fs::rename(repo.root.join("rename-temp"), repo.root.join("DIST")).unwrap();
    fs::write(repo.root.join("DIST/app.js"), "edited\n").unwrap();
    let mut relevance = NoticeRelevance::new(&repo.root);
    assert_eq!(
        relevance.classify_record("DIST/app.js", 3),
        (Relevance::IgnoredBelow("DIST".into()), true)
    );
}

#[test]
fn incomplete_or_over_budget_indexes_never_supply_partial_membership() {
    assert_eq!(parse(b"dist/a.js\0dist/b.js\0").unwrap().len(), 2);
    assert!(parse(b"dist/a.js\0dist/b.js").is_none());
    assert!(parse(&b"a\0".repeat(MAX_WORKSPACE_FILES as usize + 1)).is_none());
}
