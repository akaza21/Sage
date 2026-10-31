use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex, PoisonError},
};

use uuid::Uuid;

use super::*;
use crate::repository_public_error::MAX_PUBLIC_REPOSITORY_ERROR_BYTES;

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

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
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
fn classifies_command_running_keys() {
    for key in [
        "filter.lfs.clean",
        "filter.Ev.il.smudge",
        "filter.x.process",
        "diff.x.textconv",
        "diff.x.command",
        "diff.external",
        "merge.x.driver",
        "core.sshCommand",
        "core.askpass",
        "core.gitproxy",
        "core.alternateRefsCommand",
        "core.editor",
        "core.pager",
        "credential.helper",
        "credential.https://user:token@host.helper",
        "remote.origin.uploadpack",
        "remote.origin.receivepack",
        "sequence.editor",
        "gpg.program",
        "gpg.ssh.program",
        "gpg.ssh.defaultKeyCommand",
        "include.path",
        "includeIf.gitdir:/x/.path",
    ] {
        let parsed = ConfigKey::parse(key.as_bytes()).unwrap();
        assert!(parsed.runs_command(), "{key} should be refused");
    }
    for key in [
        "core.hooksPath",
        "core.fsmonitor",
        "core.bare",
        "filter.lfs.required",
        "filter.clean",
        "diff.x.binary",
        "diff.algorithm",
        "merge.x.name",
        "merge.conflictStyle",
        "remote.origin.url",
        "remote.uploadpack",
        "user.name",
        "branch.main.remote",
        "sequence.x.editor",
    ] {
        let parsed = ConfigKey::parse(key.as_bytes()).unwrap();
        assert!(!parsed.runs_command(), "{key} should be allowed");
    }
    assert_eq!(ConfigKey::parse(b"nodot"), None);
    assert_eq!(ConfigKey::parse(b".name"), None);
}

#[test]
fn refusal_names_bounded_redacted_keys() {
    let listing = b"core.bare\0credential.https://user:token@host.helper\0filter.lfs.clean\0\
filter.lfs.clean\0filter.lfs.smudge\0merge.x\x1b[2J.driver\0"
        .to_vec();
    let message = refusal_message(&listing).unwrap();
    assert!(message.contains("credential.*.helper, filter.lfs.clean, filter.lfs.smudge"));
    assert!(message.contains("and 1 more"), "{message}");
    assert!(!message.contains("token"));
    assert!(!message.contains('\x1b'));

    let worst = (0..8)
        .map(|index| {
            let padding = "p".repeat(MAX_SUBSECTION_CHARS - 1);
            format!("filter.{index}{padding}.process\0")
        })
        .collect::<String>();
    assert!(refusal_message(worst.as_bytes()).unwrap().len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
    assert_eq!(refusal_message(b"core.bare\0user.name\0"), None);
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
    git(&root, &["commit", "-q", "--allow-empty", "-m", "x"]);
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
fn a_root_inside_a_repository_is_checked_against_that_repository() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    git(
        &root,
        &["config", "diff.sentinel.textconv", "touch textconv-ran"],
    );
    let message = refusal(sage_git(&nested, &["rev-parse", "--show-toplevel"]));
    assert!(message.contains("diff.sentinel.textconv"), "{message}");
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
    let status = Command::new("git")
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
    let _ = Command::new("git")
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
