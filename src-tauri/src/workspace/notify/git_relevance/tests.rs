use super::*;
use crate::workspace::notify::{
    WorkspaceNotifyNotice,
    batch::NoticeBatch,
    relevance::{NoticeRelevance, Relevance},
};
use std::fs;

struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let root =
            crate::test_paths::temp_dir().join(format!("sage-notify-git-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("dist")).unwrap();
        let repo = Self(root);
        repo.git(&["init"]);
        repo.write(".gitignore", "dist/\n");
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

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn refresh(&self) -> GitRefresh {
        GitRefresh::new(vec![("root-a".into(), self.0.canonicalize().unwrap())])
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deliver_window(
    refresh: &mut GitRefresh,
    relevance: &mut NoticeRelevance,
    paths: &[(&str, u32)],
) -> Vec<WorkspaceNotifyBatch> {
    let mut batch = NoticeBatch::default();
    for &(path, action) in paths {
        let (relative, kind) = match relevance.classify(path, action) {
            Relevance::IgnoredBelow(folder) => (folder, "ignored"),
            Relevance::Keep => (
                path.to_string(),
                super::super::kind_for_action(action).unwrap(),
            ),
            Relevance::Resync => ("lost-notices".into(), "resync"),
            Relevance::Drop => continue,
        };
        let notice = WorkspaceNotifyNotice {
            project_id: "project-a".into(),
            project_generation: 1,
            transition_epoch: 1,
            root_id: "root-a".into(),
            relative_path: relative,
            kind: kind.into(),
            git_relevant: None,
        };
        batch.push_record(
            notice,
            path,
            refresh.roots["root-a"].path.join(path).is_dir(),
            action != 3,
        );
    }
    let windows = batch.take_git_windows();
    let (sent, received) = std::sync::mpsc::channel();
    refresh.emit_with(
        batch.take(),
        windows,
        &Arc::new(AtomicBool::new(false)),
        Arc::new(move |batch| {
            sent.send(batch).unwrap();
        }),
        crate::test_wait::BUDGET,
        |window, path, stop| async move { window.irrelevant(&path, stop).await },
    );
    tauri::async_runtime::block_on(async {
        for root in refresh.roots.values() {
            if let Some(gate) = &root.gate {
                let _settled = tokio::time::timeout(crate::test_wait::HANG_GUARD, gate.lock())
                    .await
                    .unwrap();
            }
        }
    });
    received.try_iter().collect()
}

fn window(
    refresh: &mut GitRefresh,
    relevance: &mut NoticeRelevance,
    paths: &[(&str, u32)],
) -> Vec<WorkspaceNotifyNotice> {
    let mut delivered = deliver_window(refresh, relevance, paths).into_iter();
    let mut listing = delivered.next().unwrap().notices;
    for decision in delivered.flat_map(|batch| batch.notices) {
        for notice in &mut listing {
            if notice.root_id == decision.root_id {
                notice.git_relevant = None;
            }
        }
    }
    listing
}

fn relevant(notices: &[WorkspaceNotifyNotice]) -> bool {
    notices
        .iter()
        .any(|notice| notice.git_relevant != Some(false))
}

#[test]
fn git_core_ignore_case_decides_even_when_the_watcher_folds_differently() {
    for ignore_case in [false, true] {
        let repo = Repository::new();
        repo.git(&[
            "config",
            "core.ignoreCase",
            if ignore_case { "true" } else { "false" },
        ]);
        repo.write(".gitignore", "DIST/\n");
        repo.write("dist/output.js", "output");
        let mut refresh = repo.refresh();
        let mut relevance = NoticeRelevance::new(&repo.0);
        assert_eq!(
            relevant(&window(
                &mut refresh,
                &mut relevance,
                &[("dist/output.js", 3)]
            )),
            !ignore_case
        );
    }
}

#[test]
fn removing_an_exclude_rule_makes_the_next_edit_relevant_without_a_rule_notice() {
    let repo = Repository::new();
    repo.write(".gitignore", "");
    repo.write(".git/info/exclude", "dist/\n");
    repo.write("dist/output.js", "first");
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    repo.write(".git/info/exclude", "");
    repo.write("dist/output.js", "later");
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
}

#[test]
fn force_added_files_and_tracked_directory_deletions_remain_relevant() {
    let repo = Repository::new();
    repo.write("dist/app.js", "base");
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/app.js", 3)]
    )));
    repo.git(&["add", "-f", "--", "dist/app.js"]);
    repo.write("dist/app.js", "edited");
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/app.js", 3)]
    )));
    fs::remove_dir_all(repo.0.join("dist")).unwrap();
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist", 2)]
    )));
}

#[test]
fn tracked_edit_then_new_root_file_both_refresh_after_separate_windows() {
    let repo = Repository::new();
    repo.write(".gitignore", "target/\n");
    repo.write("target/tracked.txt", "base");
    repo.git(&["add", "-f", "--", "target/tracked.txt"]);
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);

    repo.write("target/tracked.txt", "edited");
    let first = deliver_window(&mut refresh, &mut relevance, &[("target/tracked.txt", 3)]);
    assert_eq!(first[0].notices[0].relative_path, "target");
    assert_eq!(first[0].notices[0].kind, "ignored");
    assert_eq!(first[0].notices[0].git_relevant, Some(false));
    assert_eq!(first[1].notices[0].kind, "repository");
    assert!(relevant(&first[1].notices));

    // A status read can leave a .git directory record ahead of the new file
    // in the next window. The decision must still cover that whole window.
    repo.write("control.txt", "new");
    let next = deliver_window(
        &mut refresh,
        &mut relevance,
        &[(".git", 3), ("control.txt", 1)],
    );
    assert_eq!(
        next[1]
            .notices
            .iter()
            .map(|notice| (notice.relative_path.as_str(), notice.kind.as_str()))
            .collect::<Vec<_>>(),
        [(".git", "repository"), ("control.txt", "repository")]
    );
    assert!(
        next[1]
            .notices
            .iter()
            .all(|notice| notice.git_relevant.is_none())
    );
}

#[test]
fn ignored_paths_over_the_batch_cap_refresh_git_even_when_they_fold_to_one_notice() {
    let repo = Repository::new();
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    let paths: Vec<_> = (0..=MAX_GIT_PATHS)
        .map(|i| format!("dist/output-{i}"))
        .collect();
    let input: Vec<_> = paths.iter().map(|path| (path.as_str(), 3)).collect();
    let sent = window(&mut refresh, &mut relevance, &input);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, "ignored");
    assert!(relevant(&sent));
}

#[test]
fn sustained_ignored_output_keeps_listing_notices_without_git_refreshes() {
    let repo = Repository::new();
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    for i in 0..12 {
        let path = format!("dist/output-{i}");
        repo.write(&path, "build output");
        let sent = window(
            &mut refresh,
            &mut relevance,
            &[(path.as_str(), 1), ("dist", 3)],
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].relative_path, "dist");
        assert_eq!(sent[0].kind, "ignored");
        assert_eq!(sent[1].kind, "changed");
        assert!(!relevant(&sent), "window {i}");
    }
}

#[test]
fn removing_a_path_from_the_index_allows_later_ignored_output_to_be_suppressed() {
    let repo = Repository::new();
    repo.write("dist/app.js", "base");
    repo.git(&["add", "-f", "--", "dist/app.js"]);
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/app.js", 3)]
    )));
    repo.git(&["rm", "--cached", "--", "dist/app.js"]);
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/app.js", 3)]
    )));
}

#[test]
fn a_relevant_path_in_the_same_window_prevents_suppression() {
    let repo = Repository::new();
    repo.write("dist/output.js", "output");
    repo.write("source.js", "source");
    let sent = window(
        &mut repo.refresh(),
        &mut NoticeRelevance::new(&repo.0),
        &[("dist/output.js", 3), ("source.js", 3)],
    );
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|notice| notice.git_relevant.is_none()));
}

#[test]
fn repository_local_excludes_and_their_removal_apply_to_later_windows() {
    let repo = Repository::new();
    repo.write(".gitignore", "");
    repo.write("rules/local.ignore", "dist/\n");
    repo.git(&["config", "core.excludesFile", "rules/local.ignore"]);
    repo.write("dist/output.js", "output");
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    repo.write("rules/local.ignore", "");
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
}

#[test]
fn linked_worktree_excludes_and_external_excludes_are_read_fresh() {
    let repo = Repository::new();
    repo.write(".gitignore", "");
    repo.write("base.txt", "base");
    repo.git(&["add", "--", "base.txt"]);
    repo.git(&[
        "-c",
        "user.name=Sage Test",
        "-c",
        "user.email=sage@example.test",
        "commit",
        "-m",
        "base",
    ]);
    let linked = repo.0.join("linked");
    repo.git(&["worktree", "add", "--detach", linked.to_str().unwrap()]);
    let mut refresh = GitRefresh::new(vec![("root-a".into(), linked.canonicalize().unwrap())]);
    let mut relevance = NoticeRelevance::new(&linked);
    repo.write(".git/info/exclude", "dist/\n");
    repo.write("linked/dist/output.js", "output");
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    repo.write(".git/info/exclude", "");
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    let excludes = repo.0.join("external.ignore");
    repo.write("external.ignore", "dist/\n");
    repo.git(&["config", "core.excludesFile", excludes.to_str().unwrap()]);
    assert!(!relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    fs::remove_file(excludes).unwrap();
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
}

#[test]
fn git_errors_and_cancelled_windows_retain_the_refresh() {
    let repo = Repository::new();
    repo.write("dist/output.js", "output");
    let mut refresh = repo.refresh();
    let mut relevance = NoticeRelevance::new(&repo.0);
    repo.write(".git/config", "[invalid config\n");
    assert!(relevant(&window(
        &mut refresh,
        &mut relevance,
        &[("dist/output.js", 3)]
    )));
    let mut batch = NoticeBatch::default();
    batch.push_record(
        WorkspaceNotifyNotice {
            project_id: "project-a".into(),
            project_generation: 1,
            transition_epoch: 1,
            root_id: "root-a".into(),
            relative_path: "dist".into(),
            kind: "ignored".into(),
            git_relevant: None,
        },
        "dist/output.js",
        false,
        false,
    );
    let windows = batch.take_git_windows();
    let (sent, received) = std::sync::mpsc::channel();
    refresh.emit(
        batch.take(),
        windows,
        &Arc::new(AtomicBool::new(true)),
        Arc::new(move |batch| {
            sent.send(batch).unwrap();
        }),
    );
    assert!(
        received.try_recv().is_err(),
        "cancelled windows emit no decisions"
    );
}

#[test]
fn user_global_excludes_do_not_hide_paths_reported_by_sages_git() {
    const CHILD_HOME: &str = "SAGE_NOTIFY_GLOBAL_EXCLUDES_HOME";
    if std::env::var_os(CHILD_HOME).is_none() {
        // Isolate HOME in a child so parallel tests never share mutated env.
        let home = Repository::new();
        home.write("global.ignore", "scratch/\n");
        home.git(&[
            "config",
            "--file",
            ".gitconfig",
            "core.excludesFile",
            "~/global.ignore",
        ]);
        let canonical = home.0.canonicalize().unwrap();
        let test = concat!(
            module_path!(),
            "::user_global_excludes_do_not_hide_paths_reported_by_sages_git"
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test.split_once("::").unwrap().1, "--nocapture"])
            .env(CHILD_HOME, &canonical)
            .env("HOME", &canonical)
            .env("USERPROFILE", &canonical)
            .env("XDG_CONFIG_HOME", canonical.join(".config"))
            .env("GIT_CONFIG_GLOBAL", home.0.join(".gitconfig"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{:?}\n{:?}",
            output.stdout,
            output.stderr
        );
        return;
    }
    let repo = Repository::new();
    repo.write("scratch/new.txt", "new");
    let excludes = explorer_excludes(&repo.0, Arc::new(AtomicBool::new(false)));
    assert!(excludes.is_none());
    let mut relevance = NoticeRelevance::new(&repo.0).with_excludes(excludes);
    assert!(relevant(&window(
        &mut repo.refresh(),
        &mut relevance,
        &[("scratch/new.txt", 1)]
    )));
    let status = tauri::async_runtime::block_on(crate::repository_git::run_git(
        &repo.0.canonicalize().unwrap(),
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        64 * 1024,
    ))
    .unwrap();
    assert!(status.succeeded());
    assert!(
        status
            .stdout
            .split(|byte| *byte == 0)
            .any(|entry| entry == b"? scratch/new.txt")
    );
}

#[cfg(windows)]
#[test]
fn a_case_only_ignored_folder_rename_keeps_force_added_edits_relevant() {
    let repo = Repository::new();
    repo.git(&["config", "core.ignoreCase", "true"]);
    repo.write("dist/app.js", "base");
    repo.git(&["add", "-f", "--", "dist/app.js"]);
    fs::rename(repo.0.join("dist"), repo.0.join("rename-temp")).unwrap();
    fs::rename(repo.0.join("rename-temp"), repo.0.join("DIST")).unwrap();
    repo.write("DIST/app.js", "edited");
    assert!(relevant(&window(
        &mut repo.refresh(),
        &mut NoticeRelevance::new(&repo.0),
        &[("DIST/app.js", 3)]
    )));
}
