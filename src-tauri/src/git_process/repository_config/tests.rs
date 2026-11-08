use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
};

use uuid::Uuid;

use super::*;
use crate::error::RepositoryLayout;

mod remote_helpers;

static PROBES: LazyLock<Mutex<HashMap<PathBuf, usize>>> = LazyLock::new(Mutex::default);

pub(super) fn record_probe(root: &Path) {
    *PROBES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(root.to_path_buf())
        .or_default() += 1;
}

fn probes(root: &Path) -> usize {
    PROBES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(root)
        .copied()
        .unwrap_or_default()
}

struct Scratch {
    base: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-git-config-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        // Workspace roots are canonical, so fixtures are too (macOS `/tmp`).
        Self {
            base: base.canonicalize().unwrap(),
        }
    }

    fn repository(&self, name: &str) -> PathBuf {
        let root = self.directory(name);
        git(&root, &["init", "-q"]);
        root
    }

    fn directory(&self, name: &str) -> PathBuf {
        let root = self.base.join(name);
        fs::create_dir_all(&root).unwrap();
        root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

// Keep clone/worktree operands relative: Git for Windows accepts canonical
// verbatim paths for `-C`, but rejects them in these operands.
fn git(root: &Path, args: &[&str]) {
    let status = crate::git_process::fixture_git()
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn sage_git(root: &Path, args: &[&str]) -> AppResult<GitOutput> {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(super::super::run(
            root,
            args,
            64 * 1024,
            GitProcessPolicy::read(git_budget::READ),
        ))
}

fn refusal(result: AppResult<GitOutput>) -> String {
    match result {
        Err(AppError::UnsafeRepositoryConfig(message)) => message,
        Err(error) => panic!("expected a config refusal, got {error}"),
        Ok(_) => panic!("expected a config refusal, Git ran"),
    }
}

fn sentinel_filter(root: &Path) -> (PathBuf, String) {
    let sentinel = root.join("filter-ran");
    let script = root.join("sentinel-filter");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho ran > \"{}\"\ncat\n",
            sentinel.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let command = format!("\"{}\"", script.to_string_lossy().replace('\\', "/"));
    (sentinel, command)
}

#[test]
fn status_refuses_a_repository_filter_before_git_can_run_it() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    fs::write(root.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let (sentinel, filter) = sentinel_filter(&root);
    git(&root, &["config", "filter.sentinel.clean", &filter]);

    let message = refusal(sage_git(
        &root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    ));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
    assert!(!sentinel.exists());

    // Control: the same fixture runs the filter when Git is allowed to start.
    git(&root, &["add", "--", "a.txt"]);
    assert!(sentinel.exists(), "fixture filter is not live");
}

#[test]
fn includes_are_refused_because_their_targets_are_not_checked() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    git(&root, &["config", "include.path", "benign.inc"]);
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("include.path"), "{message}");
}

#[test]
fn ordinary_repository_config_is_admitted() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    for (key, value) in [
        ("core.hooksPath", ".husky"),
        ("filter.lfs.required", "true"),
        ("remote.origin.url", "https://example.invalid/repo.git"),
        ("user.name", "Sage Test"),
    ] {
        git(&root, &["config", key, value]);
    }
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
}

#[test]
fn cached_verdict_spawns_no_probe_until_the_config_changes() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");

    for _ in 0..3 {
        assert!(
            sage_git(&root, &["status", "--porcelain"])
                .unwrap()
                .succeeded()
        );
    }
    assert_eq!(probes(&root), 1);

    // Index and ref updates touch `.git` but not the config, so they hit.
    git(
        &root,
        &[
            "-c",
            "user.name=Sage Test",
            "-c",
            "user.email=sage@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    );
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    assert_eq!(probes(&root), 1);

    let (sentinel, filter) = sentinel_filter(&root);
    git(&root, &["config", "filter.sentinel.clean", &filter]);
    refusal(sage_git(&root, &["status", "--porcelain"]));
    assert_eq!(probes(&root), 2);
    refusal(sage_git(&root, &["status", "--porcelain"]));
    assert_eq!(probes(&root), 2);

    git(&root, &["config", "--unset", "filter.sentinel.clean"]);
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    assert_eq!(probes(&root), 3);
    assert!(!sentinel.exists());
}

#[test]
fn a_repository_created_after_the_first_check_is_checked() {
    let scratch = Scratch::new();
    let root = scratch.directory("plain");
    assert!(
        !sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );

    git(&root, &["init", "-q"]);
    git(&root, &["config", "core.sshCommand", "touch ssh-ran"]);
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("core.sshcommand"), "{message}");
}

#[test]
fn a_root_below_the_top_level_is_refused_before_git_runs() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    git(
        &root,
        &["config", "diff.sentinel.textconv", "touch textconv-ran"],
    );
    assert!(matches!(
        sage_git(&nested, &["rev-parse", "--show-toplevel"]),
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::NestedRoot
        ))
    ));
}

#[test]
fn worktree_config_is_checked() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    git(&root, &["config", "extensions.worktreeConfig", "true"]);
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    git(
        &root,
        &[
            "config",
            "--worktree",
            "merge.sentinel.driver",
            "touch merge-ran",
        ],
    );
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("merge.sentinel.driver"), "{message}");
}

#[test]
fn an_empty_git_directory_filled_later_is_checked() {
    let scratch = Scratch::new();
    let root = scratch.directory("hollow");
    fs::create_dir(root.join(".git")).unwrap();
    for _ in 0..2 {
        assert!(
            !sage_git(&root, &["status", "--porcelain"])
                .unwrap()
                .succeeded()
        );
    }
    assert_eq!(
        probes(&root),
        2,
        "an unused .git entry must not cache a verdict"
    );

    git(&root, &["init", "-q"]);
    git(
        &root,
        &["config", "filter.sentinel.clean", "touch filter-ran"],
    );
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
}

#[test]
fn a_same_length_rewrite_with_a_restored_mtime_is_detected() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let config = root.join(".git").join("config");
    let benign = fs::read_to_string(&config).unwrap();
    let original = format!("{benign}[user]\n\tname = {}\n", "n".repeat(40));
    let malicious = format!("{benign}[filter \"x\"]\n\tclean = touch ran\n");
    // A comment line pads the rewrite to exactly the original length.
    let padding = original.len() - malicious.len() - 2;
    let rewritten = format!("{malicious}#{}\n", "x".repeat(padding));
    assert_eq!(original.len(), rewritten.len());
    fs::write(&config, &original).unwrap();

    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    let modified = fs::metadata(&config).unwrap().modified().unwrap();
    fs::write(&config, &rewritten).unwrap();
    fs::File::options()
        .write(true)
        .open(&config)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), modified);

    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.x.clean"), "{message}");
}

#[cfg(unix)]
#[test]
fn a_non_utf8_filter_subsection_is_refused() {
    use std::os::unix::ffi::OsStrExt;

    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let driver = b"ev\xffil";
    let mut attributes = b"*.txt filter=".to_vec();
    attributes.extend_from_slice(driver);
    attributes.push(b'\n');
    fs::write(root.join(".gitattributes"), attributes).unwrap();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let (sentinel, filter) = sentinel_filter(&root);
    let mut key = b"filter.".to_vec();
    key.extend_from_slice(driver);
    key.extend_from_slice(b".clean");
    let status = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&root)
        .arg("config")
        .arg(std::ffi::OsStr::from_bytes(&key))
        .arg(&filter)
        .status()
        .unwrap();
    assert!(status.success());

    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.*.clean"), "{message}");
    assert!(!sentinel.exists());

    // Control: Git does run the non-UTF-8 driver when it is allowed to start.
    git(&root, &["add", "--", "a.txt"]);
    assert!(sentinel.exists(), "fixture filter is not live");
}

#[test]
fn the_cache_drops_idle_roots_once_full() {
    let mut admissions = HashMap::new();
    let held = slot_in(&mut admissions, Path::new("/held"));
    for index in 0..MAX_CACHED_ROOTS + 8 {
        drop(slot_in(
            &mut admissions,
            &PathBuf::from(format!("/idle-{index}")),
        ));
        assert!(admissions.len() <= MAX_CACHED_ROOTS);
    }
    assert!(admissions.contains_key(Path::new("/held")));
    drop(held);
}

#[test]
fn an_admitted_repository_cannot_drift_to_a_hostile_ancestor() {
    let scratch = Scratch::new();
    let outer = scratch.directory("outer");
    let inner = outer.join("inner");
    fs::create_dir(&inner).unwrap();
    git(&inner, &["init", "-q"]);
    assert!(
        sage_git(&inner, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );

    git(&outer, &["init", "-q"]);
    git(&outer, &["config", "user.email", "sage@example.invalid"]);
    git(&outer, &["config", "user.name", "Sage Test"]);
    fs::write(outer.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    fs::write(outer.join("a.txt"), "a\n").unwrap();
    git(&outer, &["add", "--", ".gitattributes", "a.txt"]);
    git(&outer, &["commit", "-q", "-m", "fixture"]);
    let (sentinel, filter) = sentinel_filter(&outer);
    git(&outer, &["config", "filter.sentinel.clean", &filter]);
    // Same size as the committed content, so status has to hash it.
    fs::write(outer.join("a.txt"), "b\n").unwrap();
    // The inner repository's watched files are unchanged, but without HEAD
    // discovery would now fall through to `outer`.
    fs::remove_file(inner.join(".git").join("HEAD")).unwrap();

    let result = sage_git(&inner, &["status", "--porcelain"]);
    assert!(
        result.as_ref().map_or(true, |output| !output.succeeded()),
        "status ran against a repository admission did not inspect"
    );
    assert!(!sentinel.exists(), "the ancestor's filter ran");

    // Control: unpinned discovery does reach the hostile ancestor.
    let _ = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&inner)
        .args(["status", "--porcelain"])
        .status()
        .unwrap();
    assert!(sentinel.exists(), "fixture ancestor is not live");
}

#[test]
fn a_full_cache_of_active_roots_admits_without_caching() {
    let mut admissions = HashMap::new();
    let held = (0..MAX_CACHED_ROOTS)
        .map(|index| slot_in(&mut admissions, &PathBuf::from(format!("/active-{index}"))))
        .collect::<Vec<_>>();
    let overflow = slot_in(&mut admissions, Path::new("/overflow"));
    assert_eq!(admissions.len(), MAX_CACHED_ROOTS);
    assert!(!admissions.contains_key(Path::new("/overflow")));
    assert_eq!(Arc::strong_count(&overflow), 1);
    drop(held);
}

#[test]
fn only_git_data_inside_the_root_is_admitted() {
    let scratch = Scratch::new();
    let root = scratch.directory("repo");
    let outside = scratch.directory("main").join(".git");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let location = |git_dir: &Path, common_dir: &Path, work_tree: &Path| Location {
        git_dir: git_dir.to_path_buf(),
        common_dir: common_dir.to_path_buf(),
        work_tree: work_tree.to_path_buf(),
    };
    let inside = root.join(".git");
    let elsewhere_in_root = root.join("metadata").join("repo.git");
    fs::create_dir_all(&elsewhere_in_root).unwrap();

    let exact = layout::resolve(&root, &location(&inside, &inside, &root))
        .unwrap()
        .ok()
        .unwrap()
        .repository;
    assert_eq!(exact.git_dir(), Path::new(".git"));
    assert_eq!(exact.work_tree(), Path::new("."));

    for (git_dir, common_dir) in [
        (&outside, &outside),
        (&inside, &outside),
        (&elsewhere_in_root, &elsewhere_in_root),
        (&inside, &elsewhere_in_root),
    ] {
        assert_eq!(
            layout::resolve(&root, &location(git_dir, common_dir, &root))
                .unwrap()
                .err(),
            Some(RepositoryLayout::ExternalGitDir)
        );
    }
    assert_eq!(
        layout::resolve(&root.join(".git"), &location(&inside, &inside, &root))
            .unwrap()
            .err(),
        Some(RepositoryLayout::NestedRoot)
    );
}

/// A separate git dir reached through a junctioned (or symlinked) ancestor is
/// resolved the way Git resolves it, and the directory reached is inspected.
/// Each directory on the way is stamped by identity, so retargeting the link
/// at hostile Git data is checked again rather than served from the cache.
#[test]
fn a_separate_git_dir_reached_through_a_junctioned_ancestor_is_checked() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let real = scratch.directory("real-metadata");
    fs::rename(root.join(".git"), real.join("repo.git")).unwrap();
    let metadata = root.join("metadata");
    directory_link(&metadata, &real);
    fs::write(root.join(".git"), "gitdir: metadata/repo.git\n").unwrap();
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    assert_eq!(probes(&root), 1);

    // The same layout, now through a link to Git data with a filter.
    let hostile = scratch.directory("hostile-metadata");
    copy_tree(&real.join("repo.git"), &hostile.join("repo.git"));
    git(
        &root,
        &[
            "config",
            "--file",
            &config_operand(&hostile.join("repo.git")),
            "filter.sentinel.clean",
            "touch ran",
        ],
    );
    remove_link(&metadata);
    directory_link(&metadata, &hostile);
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
    assert_eq!(probes(&root), 2, "the retarget must invalidate the verdict");
}
/// `git init --separate-git-dir` keeps the git data outside the work tree.
/// Status and diff work, and a filter in that git dir's config is refused.
#[test]
fn a_separate_git_dir_repository_is_admitted_and_its_config_checked() {
    let scratch = Scratch::new();
    let root = scratch.directory("work");
    git(
        &scratch.base,
        &["init", "-q", "--separate-git-dir", "data.git", "work"],
    );
    fs::write(root.join("a.txt"), "a\n").unwrap();
    for args in [&["status", "--porcelain"][..], &["diff"][..]] {
        assert!(sage_git(&root, args).unwrap().succeeded(), "{args:?}");
    }
    git(
        &root,
        &["config", "filter.sentinel.clean", "touch filter-ran"],
    );
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
}
/// A `commondir` file redirects the shared config the same way.
#[test]
fn a_commondir_pointer_is_refused_even_when_it_points_inside_the_root() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    fs::create_dir_all(root.join("metadata")).unwrap();
    git(
        &root,
        &["clone", "-q", "--bare", ".", "metadata/common.git"],
    );
    fs::write(
        root.join(".git").join("commondir"),
        "../metadata/common.git\n",
    )
    .unwrap();
    // Control: Git itself reads the pointed-to common dir.
    git(&root, &["rev-parse", "--git-common-dir"]);

    assert!(matches!(
        sage_git(&root, &["status", "--porcelain"]),
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::ExternalGitDir
        ))
    ));
}

/// A `commondir` that aliases the root's own `.git` through a link passes a
/// canonical-path check, and the link can later be repointed at hostile
/// metadata without changing any watched bytes. Its presence is refused.
#[cfg(unix)]
#[test]
fn a_commondir_alias_of_the_own_git_dir_is_refused() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    fs::write(root.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let (sentinel, tool) = sentinel_filter(&root);
    let metadata = root.join("metadata");
    fs::create_dir_all(&metadata).unwrap();
    let alias = metadata.join("common.git");
    std::os::unix::fs::symlink(root.join(".git"), &alias).unwrap();
    fs::write(
        root.join(".git").join("commondir"),
        "../metadata/common.git\n",
    )
    .unwrap();
    // Control: Git resolves the alias to the root's own git dir.
    let output = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .unwrap();
    assert_eq!(
        Path::new(String::from_utf8(output.stdout).unwrap().trim())
            .canonicalize()
            .unwrap(),
        root.join(".git").canonicalize().unwrap()
    );
    assert!(matches!(
        sage_git(&root, &["status", "--porcelain"]),
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::ExternalGitDir
        ))
    ));

    // Repoint the alias at external metadata carrying a filter.
    let hostile = scratch.repository("hostile");
    git(&hostile, &["config", "filter.sentinel.clean", &tool]);
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(hostile.join(".git"), &alias).unwrap();
    assert!(matches!(
        sage_git(&root, &["status", "--porcelain"]),
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::ExternalGitDir
        ))
    ));
    assert!(!sentinel.exists(), "the external filter ran");

    // Control: plain Git now runs the external filter through the alias.
    git(&root, &["add", "--", "a.txt"]);
    assert!(sentinel.exists(), "the redirected filter is not live");
}

/// A pointer file created after admission invalidates the cached verdict.
#[test]
fn a_pointer_file_created_after_admission_is_refused() {
    for name in POINTER_FILES {
        let scratch = Scratch::new();
        let root = scratch.repository("repo");
        assert!(
            sage_git(&root, &["status", "--porcelain"])
                .unwrap()
                .succeeded()
        );
        fs::write(root.join(".git").join(name), ".\n").unwrap();
        assert!(
            matches!(
                sage_git(&root, &["status", "--porcelain"]),
                Err(AppError::RepositoryOutsideRoot(
                    RepositoryLayout::ExternalGitDir
                ))
            ),
            "{name}"
        );
        assert_eq!(probes(&root), 2, "{name}");
    }
}

/// A real `git worktree add` worktree is admitted: status and diff work, and
/// a filter in the common config it shares with the main worktree is refused.
#[test]
fn a_linked_worktree_is_admitted_and_its_common_config_checked() {
    let scratch = Scratch::new();
    let main = scratch.repository("main");
    git(&main, &["config", "user.email", "sage@example.invalid"]);
    git(&main, &["config", "user.name", "Sage Test"]);
    fs::write(main.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "x"]);
    let worktree = scratch.base.join("linked");
    git(
        &main,
        &["worktree", "add", "-q", "../linked", "-b", "linked"],
    );
    fs::write(worktree.join("a.txt"), "changed\n").unwrap();
    for args in [&["status", "--porcelain"][..], &["diff"][..]] {
        assert!(sage_git(&worktree, args).unwrap().succeeded(), "{args:?}");
    }

    let (sentinel, filter) = sentinel_filter(&scratch.base);
    git(&main, &["config", "filter.sentinel.clean", &filter]);
    for args in [&["status", "--porcelain"][..], &["diff"][..]] {
        let message = refusal(sage_git(&worktree, args));
        assert!(message.contains("filter.sentinel.clean"), "{message}");
    }
    assert!(!sentinel.exists());
    // The main worktree reads the same config and is refused too.
    refusal(sage_git(&main, &["status", "--porcelain"]));
    // Control: plain Git in the linked worktree runs the shared filter.
    git(&worktree, &["add", "--", "a.txt"]);
    assert!(sentinel.exists(), "the shared filter is not live");
}
/// A `.git` that is itself a link (a junction on Windows, which is also how a
/// reparse point such as a cloud placeholder looks to the filesystem) is
/// followed the way Git follows it, and admitted. Retargeting it at another
/// directory changes its identity, so the new target is checked rather than
/// served from the cache.
#[test]
fn a_git_dir_reached_through_a_link_is_admitted_and_checked_again_on_retarget() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    let outside = scratch.base.join("outside.git");
    fs::rename(root.join(".git"), &outside).unwrap();
    directory_link(&root.join(".git"), &outside);
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    // The link reaches the very directory that was admitted (a rename keeps
    // its identity), so the verdict stands.
    assert_eq!(probes(&root), 1);

    let hostile = scratch.base.join("hostile.git");
    copy_tree(&outside, &hostile);
    git(
        &root,
        &[
            "config",
            "--file",
            &config_operand(&hostile),
            "filter.sentinel.clean",
            "touch ran",
        ],
    );
    remove_link(&root.join(".git"));
    directory_link(&root.join(".git"), &hostile);
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
    assert_eq!(probes(&root), 2, "the retarget must invalidate the verdict");
}

/// A linked config file is followed the way Git follows it and checked.
#[cfg(unix)]
#[test]
fn a_linked_config_file_is_followed_and_checked() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let config = root.join(".git").join("config");
    let outside = scratch.base.join("outside-config");
    fs::rename(&config, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &config).unwrap();
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
    git(
        &root,
        &[
            "config",
            "--file",
            outside.to_str().unwrap(),
            "filter.sentinel.clean",
            "touch ran",
        ],
    );
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
}
/// Keeps `path` unreadable to every other opener until dropped.
struct ReadDenied {
    #[cfg(windows)]
    _exclusive: fs::File,
    #[cfg(unix)]
    path: PathBuf,
    /// Where the file waits while `path` is a link to itself.
    #[cfg(unix)]
    held: Option<PathBuf>,
}

impl ReadDenied {
    /// Its metadata still reads; opening it fails. On Unix the permission
    /// bits deny any process that does not override them, as root does, so
    /// this is `None` when this process can still read the file.
    fn on_open(path: &Path) -> Option<Self> {
        #[cfg(windows)]
        let denied = {
            use std::os::windows::fs::OpenOptionsExt;
            let exclusive = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(path)
                .unwrap();
            Self {
                _exclusive: exclusive,
            }
        };
        #[cfg(unix)]
        let denied = {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            Self {
                path: path.to_path_buf(),
                held: None,
            }
        };
        assert!(fs::metadata(path).is_ok());
        fs::read(path).is_err().then_some(denied)
    }

    /// Even its metadata fails, whatever this process's uid: `path` becomes a
    /// symbolic link to itself, which no opener can resolve.
    #[cfg(unix)]
    fn on_metadata(path: &Path) -> Self {
        let held = path.with_extension("held");
        fs::rename(path, &held).unwrap();
        std::os::unix::fs::symlink(path.file_name().unwrap(), path).unwrap();
        assert!(fs::metadata(path).is_err(), "fixture config still resolves");
        Self {
            path: path.to_path_buf(),
            held: Some(held),
        }
    }
}

#[cfg(unix)]
impl Drop for ReadDenied {
    fn drop(&mut self) {
        match &self.held {
            Some(held) => {
                let _ = fs::remove_file(&self.path);
                let _ = fs::rename(held, &self.path);
            }
            None => {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o644));
            }
        }
    }
}

#[test]
fn an_unreadable_repository_config_fails_closed_until_it_can_be_checked() {
    // A process that reads through permission bits cannot be denied this way;
    // the unresolvable case below still runs for it.
    unreadable_config_fails_closed(ReadDenied::on_open);
}

#[cfg(unix)]
#[test]
fn an_unresolvable_repository_config_fails_closed_until_it_can_be_checked() {
    unreadable_config_fails_closed(|config| Some(ReadDenied::on_metadata(config)));
}

fn unreadable_config_fails_closed(deny: impl FnOnce(&Path) -> Option<ReadDenied>) {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    git(
        &root,
        &["config", "filter.sentinel.clean", "touch filter-ran"],
    );
    let config = root.join(".git").join("config");

    let Some(denied) = deny(&config) else {
        return;
    };
    for _ in 0..2 {
        let message = refusal(sage_git(&root, &["status", "--porcelain"]));
        assert_eq!(message, UNREADABLE);
    }
    assert_eq!(probes(&root), 2, "an unreadable config must not cache");
    drop(denied);

    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("filter.sentinel.clean"), "{message}");
    assert!(!root.join("filter-ran").exists());
}

const IDENTITY: [&str; 4] = [
    "-c",
    "user.name=Sage Test",
    "-c",
    "user.email=sage@example.invalid",
];

/// A repository with an absorbed submodule `sub` whose `*.txt` files use the
/// `sentinel` filter, which no config defines yet.
fn submodule_fixture(scratch: &Scratch) -> PathBuf {
    let source = scratch.repository("source");
    fs::write(source.join(".gitattributes"), "*.txt filter=sentinel\n").unwrap();
    fs::write(source.join("a.txt"), "a\n").unwrap();
    git(&source, &["add", "."]);
    git(
        &source,
        &[&IDENTITY[..], &["commit", "-q", "-m", "source"]].concat(),
    );
    let root = scratch.repository("repo");
    git(
        &root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "--quiet",
            "add",
            "../source",
            "sub",
        ],
    );
    git(
        &root,
        &[&IDENTITY[..], &["commit", "-q", "-m", "submodule"]].concat(),
    );
    root
}

/// Rewrites the submodule's checked-out bytes (line endings included) with a
/// new timestamp, so only cleaning the file again tells Git whether it changed.
fn touch_submodule_file(root: &Path) {
    let file = root.join("sub").join("a.txt");
    let checked_out = fs::read(&file).unwrap();
    fs::write(&file, checked_out).unwrap();
}

/// A directory symlink on Unix, a junction on Windows.
fn directory_link(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let plain = |path: &Path| {
            path.to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned()
        };
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", &plain(link), &plain(target)])
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }
}

/// Copies a directory tree, for Git data that must be byte-identical.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A `config` path Git accepts as an operand (Git for Windows rejects
/// verbatim `\\?\` paths there).
fn config_operand(git_dir: &Path) -> String {
    let path = git_dir.join("config");
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

/// Removes a directory link made by [`directory_link`] without touching its
/// target.
fn remove_link(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
}

#[test]
fn a_switched_off_path_protection_is_refused_until_restored() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    for key in ["core.protectNTFS", "core.protectHFS"] {
        git(&root, &["config", key, "false"]);
        let message = refusal(sage_git(&root, &["status", "--porcelain"]));
        assert!(message.contains(key), "{message}");
        git(&root, &["config", key, "true"]);
        assert!(
            sage_git(&root, &["status", "--porcelain"])
                .unwrap()
                .succeeded(),
            "{key}=true is admitted"
        );
    }
}

/// With `core.protectNTFS` off, the index can name the gitlink `sub.`. A
/// verbatim path finds nothing there, but Win32 strips the dot, so Git opens
/// the populated `sub` and runs its filter.
#[cfg(windows)]
#[test]
fn a_gitlink_win32_would_rename_cannot_reach_a_hostile_submodule() {
    let scratch = Scratch::new();
    let root = submodule_fixture(&scratch);
    let gitlink = crate::git_process::fixture_git()
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD:sub"])
        .output()
        .unwrap();
    let cacheinfo = format!(
        "160000,{},sub.",
        String::from_utf8(gitlink.stdout).unwrap().trim()
    );
    git(
        &root,
        &[
            "-c",
            "core.protectNTFS=false",
            "update-index",
            "--add",
            "--cacheinfo",
            &cacheinfo,
        ],
    );
    git(&root, &["update-index", "--force-remove", "sub"]);
    git(&root, &["config", "core.protectNTFS", "false"]);
    let (sentinel, filter) = sentinel_filter(&scratch.base);
    git(
        &root.join("sub"),
        &["config", "filter.sentinel.clean", &filter],
    );
    touch_submodule_file(&root);

    for args in [
        &["status", "--porcelain"][..],
        &["diff", "--no-ext-diff"][..],
    ] {
        refusal(sage_git(&root, args));
    }
    assert!(!sentinel.exists());

    // Control: Git without the file-system cache, as Sage runs it, opens
    // `sub` for the gitlink `sub.` and runs the filter.
    git(
        &root,
        &["-c", "core.fscache=false", "status", "--porcelain"],
    );
    assert!(sentinel.exists(), "the renamed gitlink is not live");
}

/// A repository with a populated submodule, an upstream to fetch from, and
/// config that would make Git recurse into the submodule everywhere: in the
/// parent, in `.gitmodules`, and in the submodule's own config, which also
/// carries a live clean filter and a hostile `core.worktree`.
fn isolation_fixture(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let root = submodule_fixture(scratch);
    git(
        &scratch.base,
        &["clone", "-q", "--bare", "repo", "upstream.git"],
    );
    git(&root, &["remote", "add", "origin", "../upstream.git"]);
    for (key, value) in [
        ("submodule.recurse", "true"),
        ("fetch.recurseSubmodules", "yes"),
        ("diff.submodule", "diff"),
        ("status.submoduleSummary", "true"),
    ] {
        git(&root, &["config", key, value]);
    }
    for (key, value) in [
        ("submodule.sub.ignore", "none"),
        ("submodule.sub.fetchRecurseSubmodules", "yes"),
    ] {
        git(&root, &["config", "--file", ".gitmodules", key, value]);
    }
    let (sentinel, filter) = sentinel_filter(&scratch.base);
    // A worktree elsewhere with the submodule's files, stat-dirty, so Git in
    // the submodule would clean them through the filter.
    let hostile = scratch.base.join("hostile-worktree");
    copy_tree(&root.join("sub"), &hostile);
    let bytes = fs::read(hostile.join("a.txt")).unwrap();
    fs::write(hostile.join("a.txt"), bytes).unwrap();
    let submodule = root.join(".git").join("modules").join("sub");
    let submodule_config = config_operand(&submodule);
    git(
        &root,
        &[
            "config",
            "--file",
            &submodule_config,
            "filter.sentinel.clean",
            &filter,
        ],
    );
    git(
        &root,
        &[
            "config",
            "--file",
            &submodule_config,
            "core.worktree",
            &plain(&hostile).replace('\\', "/"),
        ],
    );
    (root, sentinel)
}

/// Runs Sage's Git with a trace2 target and returns its events.
fn traced_git(root: &Path, args: &[&str], trace: &Path) -> (AppResult<GitOutput>, String) {
    let _ = fs::remove_file(trace);
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(super::super::run(
            root,
            args,
            64 * 1024,
            GitProcessPolicy::read(git_budget::READ).with_trace2_event(PathBuf::from(plain(trace))),
        ));
    (result, fs::read_to_string(trace).unwrap_or_default())
}

/// Whether any Git process opened `repository` as its own repository: its
/// `def_repo` event (repository 1) names that work tree, or a process was
/// started to fetch a submodule.
fn git_opened(events: &str, repository: &str) -> bool {
    events.lines().any(|event| {
        (event.contains(r#""event":"def_repo""#)
            && event.contains(r#""repo":1,"#)
            && event.contains(repository))
            || event.contains("--submodule-prefix")
    })
}

/// `path` without a verbatim `\\?\` prefix, which Git for Windows rejects in
/// operands and trace targets.
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

/// Status, diff and fetch work in a repository whose submodule config would
/// run a filter (through a hostile `core.worktree`) as soon as Git ran in
/// the submodule, and no Git process ever opens the submodule.
#[test]
fn status_diff_and_fetch_never_run_git_in_a_submodule() {
    let scratch = Scratch::new();
    let (root, sentinel) = isolation_fixture(&scratch);
    touch_submodule_file(&root);
    let trace = scratch.base.join("trace2.json");
    for args in [
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"][..],
        &["diff"][..],
        &["fetch", "-q", "origin"][..],
    ] {
        let (result, events) = traced_git(&root, args, &trace);
        assert!(result.unwrap().succeeded(), "{args:?}");
        assert!(
            events.contains(r#""event":"def_repo""#),
            "trace2 is not live"
        );
        assert!(
            !git_opened(&events, r#"/sub""#),
            "{args:?} ran Git in the submodule"
        );
        assert!(
            !git_opened(&events, "/modules/sub"),
            "{args:?} opened its git dir"
        );
    }
    assert!(!sentinel.exists(), "the submodule's filter ran");
    assert!(!root.join(".git/modules/sub/FETCH_HEAD").exists());

    // Control: plain Git, honouring the same config, runs it in the submodule.
    git(&root, &["status", "--porcelain"]);
    assert!(sentinel.exists(), "the submodule's filter is not live");
}

/// With the checkout gone but its git dir retained under `.git/modules`, a
/// recursive fetch would still open that git dir. Sage's never does.
#[test]
fn a_retained_submodule_git_dir_without_a_checkout_is_never_opened() {
    let scratch = Scratch::new();
    let (root, sentinel) = isolation_fixture(&scratch);
    fs::remove_dir_all(root.join("sub")).unwrap();
    let trace = scratch.base.join("trace2.json");
    for args in [
        &["fetch", "-q", "origin"][..],
        &["status", "--porcelain=v2", "-z"][..],
    ] {
        let (result, events) = traced_git(&root, args, &trace);
        assert!(result.unwrap().succeeded(), "{args:?}");
        assert!(
            !git_opened(&events, "/modules/sub"),
            "{args:?} opened its git dir"
        );
    }
    assert!(!root.join(".git/modules/sub/FETCH_HEAD").exists());
    assert!(!sentinel.exists());
}

/// A layout refusal is never served from the cache: once the cause is
/// repaired, the next call probes again and is admitted.
#[test]
fn a_repaired_layout_refusal_is_admitted_on_the_next_call() {
    let scratch = Scratch::new();
    // `core.worktree` pointing elsewhere makes the root no longer the top level.
    let root = scratch.repository("worktree-elsewhere");
    let elsewhere = scratch.directory("elsewhere");
    git(
        &root,
        &[
            "config",
            "core.worktree",
            &plain(&elsewhere).replace('\\', "/"),
        ],
    );
    assert!(matches!(
        sage_git(&root, &["status", "--porcelain"]),
        Err(AppError::RepositoryOutsideRoot(_))
    ));
    git(&root, &["config", "--unset", "core.worktree"]);
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );

    // A stray `gitdir` pointer file in the root's own `.git` directory.
    let root = scratch.repository("stray-pointer");
    let pointer = root.join(".git").join("gitdir");
    fs::write(&pointer, ".\n").unwrap();
    assert!(matches!(
        sage_git(&root, &["status", "--porcelain"]),
        Err(AppError::RepositoryOutsideRoot(
            RepositoryLayout::ExternalGitDir
        ))
    ));
    fs::remove_file(&pointer).unwrap();
    assert!(
        sage_git(&root, &["status", "--porcelain"])
            .unwrap()
            .succeeded()
    );
}
