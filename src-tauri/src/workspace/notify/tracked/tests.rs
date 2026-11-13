use super::*;
use std::{fs, path::PathBuf};

use super::super::relevance::{NoticeRelevance, Relevance};

struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-notify-tracked-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join("dist")).unwrap();
        let repo = Self(path.canonicalize().unwrap());
        repo.git(&["init"]);
        fs::write(repo.0.join(".gitignore"), "dist/\ntarget/\n").unwrap();
        fs::write(repo.0.join("dist/app.js"), "base\n").unwrap();
        repo.git(&["add", "-f", "--", "dist/app.js"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        let output = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sustained_output_is_irrelevant_but_tracked_edits_and_directory_removals_are_relevant() {
    let repo = Repository::new();
    let mut tracked = TrackedPaths::default();
    for batch in 0..60 {
        assert!(!tracked.relevant(
            &repo.0,
            "target",
            &format!("target/debug/output-{batch}"),
            true,
        ));
        assert!(!tracked.relevant(&repo.0, "dist", "dist/generated.js", true));
        assert!(!tracked.relevant(&repo.0, "dist", "dist", false));
    }
    fs::write(repo.0.join("dist/app.js"), "edited\n").unwrap();
    assert!(tracked.relevant(&repo.0, "dist", "dist/app.js", false));
    fs::remove_file(repo.0.join("dist/app.js")).unwrap();
    assert!(tracked.relevant(&repo.0, "dist", "dist/app.js", true));
    assert!(tracked.relevant(&repo.0, "dist", "dist", true));
    assert!(!tracked.relevant(&repo.0, "dist", "dis", true));
}

#[test]
fn a_force_add_and_index_removal_refresh_cached_membership() {
    let repo = Repository::new();
    let mut tracked = TrackedPaths::default();
    assert!(!tracked.relevant(&repo.0, "dist", "dist/new.js", false));
    fs::write(repo.0.join("dist/new.js"), "new\n").unwrap();
    repo.git(&["add", "-f", "--", "dist/new.js"]);
    assert!(tracked.relevant(&repo.0, "dist", "dist/new.js", false));
    repo.git(&["rm", "--cached", "--", "dist/app.js"]);
    assert!(!tracked.relevant(&repo.0, "dist", "dist/app.js", false));
}

#[test]
fn ignored_folder_and_descendant_notices_keep_explorer_meaning_but_filter_git_output() {
    let repo = Repository::new();
    fs::create_dir_all(repo.0.join("target/debug")).unwrap();
    let mut relevance = NoticeRelevance::new(&repo.0, None);
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
    fs::write(repo.0.join("dist/app.js"), "edited\n").unwrap();
    assert_eq!(
        relevance.classify_record("dist/app.js", 3),
        (Relevance::IgnoredBelow("dist".into()), true)
    );
}

#[test]
fn ignored_directory_clean_and_recreate_stays_irrelevant_without_losing_tracked_deletions() {
    let repo = Repository::new();
    let mut relevance = NoticeRelevance::new(&repo.0, None);
    for _ in 0..3 {
        fs::create_dir(repo.0.join("target")).unwrap();
        assert_eq!(
            relevance.classify_record("target", 1),
            (Relevance::Keep, false)
        );
        fs::remove_dir(repo.0.join("target")).unwrap();
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
    fs::remove_dir_all(repo.0.join("dist")).unwrap();
    assert_eq!(
        relevance.classify_record("dist", 2),
        (Relevance::Keep, true)
    );
    fs::write(repo.0.join("target"), "ordinary untracked file\n").unwrap();
    assert_eq!(
        relevance.classify_record("target", 1),
        (Relevance::Keep, true)
    );
}

#[test]
fn unrelated_tracked_paths_do_not_consume_an_empty_subtrees_lookup_budget() {
    let repo = Repository::new();
    let (paths, bytes) = tauri::async_runtime::block_on(load(&repo.0, "target", 0)).unwrap();
    assert!(paths.is_empty());
    assert_eq!(bytes, 0);
}

#[test]
#[cfg(windows)]
fn a_case_only_ignored_folder_rename_still_reports_tracked_edits() {
    let repo = Repository::new();
    fs::rename(repo.0.join("dist"), repo.0.join("rename-temp")).unwrap();
    fs::rename(repo.0.join("rename-temp"), repo.0.join("DIST")).unwrap();
    fs::write(repo.0.join("DIST/app.js"), "edited\n").unwrap();
    let mut relevance = NoticeRelevance::new(&repo.0, None);
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
