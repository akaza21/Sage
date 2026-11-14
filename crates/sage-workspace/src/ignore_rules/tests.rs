use std::{fs, path::PathBuf};

use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-ignore-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, contents: &str) -> &Self {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        self
    }

    fn dir(&self, relative: &str) -> &Self {
        fs::create_dir_all(self.0.join(relative)).unwrap();
        self
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ignored(rules: &IgnoreRules, root: &Path, relative: &str, is_dir: bool) -> bool {
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    let mut chain = rules.enter(&FolderChain::default(), "", RuleFiles::probe(root));
    let mut path = String::new();
    for component in parent.split('/').filter(|part| !part.is_empty()) {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(component);
        assert!(!rules.is_ignored(&chain, &path, true), "{path} is ignored");
        chain = rules.enter(&chain, &path, RuleFiles::probe(&root.join(&path)));
    }
    rules.is_ignored(&chain, relative, is_dir)
}

#[test]
fn applies_nested_rules_with_negation_and_deeper_files_winning() {
    let fixture = Fixture::new("nested");
    fixture
        .write(".gitignore", "*.log\n!important.log\n")
        .write("sub/.gitignore", "!keep.log\nimportant.log\n")
        .write("top.log", "")
        .write("important.log", "")
        .write("sub/keep.log", "")
        .write("sub/other.log", "")
        .write("sub/important.log", "");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    let root = &fixture.0;
    assert!(ignored(&rules, root, "top.log", false));
    assert!(!ignored(&rules, root, "important.log", false));
    assert!(!ignored(&rules, root, "sub/keep.log", false));
    assert!(ignored(&rules, root, "sub/other.log", false));
    assert!(ignored(&rules, root, "sub/important.log", false));
    assert!(!ignored(&rules, root, "sub", true));
}

#[test]
fn directory_rules_match_only_directories_and_anchors_hold() {
    let fixture = Fixture::new("dir-only");
    fixture
        .write(".gitignore", "build/\n/dist\n# comment\n\n[unclosed\n")
        .dir("build")
        .dir("src/dist");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    let root = &fixture.0;
    assert!(ignored(&rules, root, "build", true));
    assert!(!ignored(&rules, root, "build", false));
    assert!(ignored(&rules, root, "src/build", true));
    assert!(ignored(&rules, root, "dist", true));
    assert!(!ignored(&rules, root, "src/dist", true));
}

#[test]
fn a_dot_ignore_match_beats_any_gitignore_match() {
    let fixture = Fixture::new("dot-ignore");
    fixture
        .write(".ignore", "!generated.rs\nscratch/\n")
        .write("src/.gitignore", "generated.rs\n");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    let root = &fixture.0;
    assert!(!ignored(&rules, root, "src/generated.rs", false));
    assert!(ignored(&rules, root, "scratch", true));
}

#[test]
fn applies_the_repository_exclude_file_and_the_global_excludes_file() {
    let fixture = Fixture::new("exclude");
    fixture
        .write(".git/info/exclude", "local-only/\n")
        .write(".gitignore", "!keep.swp\n")
        .write("global-ignore", "*.swp\n.DS_Store\n");
    let global = fixture.0.join("global-ignore");
    let rules = IgnoreRules::load(&fixture.0, true, Some(&global));
    assert!(
        IgnoreRules::load(&fixture.0, true, Some(Path::new("global-ignore")))
            .global
            .is_none()
    );
    let root = &fixture.0;
    assert!(ignored(&rules, root, "local-only", true));
    assert!(ignored(&rules, root, "notes.swp", false));
    assert!(ignored(&rules, root, "src/.DS_Store", false));
    assert!(!ignored(&rules, root, "keep.swp", false));
    let without_global = IgnoreRules::load(&fixture.0, true, None);
    assert!(!ignored(&without_global, root, "notes.swp", false));
}

#[test]
fn reads_the_exclude_file_of_a_worktree_through_its_common_directory() {
    let fixture = Fixture::new("worktree");
    fixture
        .write("main/.git/info/exclude", "from-common/\n")
        .write("main/.git/worktrees/tree/commondir", "../..\n")
        .write("tree/.git", "gitdir: ../main/.git/worktrees/tree\n");
    let root = fixture.0.join("tree");
    let rules = IgnoreRules::load(&root, true, None);
    assert!(ignored(&rules, &root, "from-common", true));
}

#[test]
fn rule_files_above_the_root_never_apply() {
    let fixture = Fixture::new("above");
    fixture
        .write(".gitignore", "*\n")
        .write(".ignore", "*\n")
        .write(".git/info/exclude", "*\n")
        .dir("project/src");
    let root = fixture.0.join("project");
    let rules = IgnoreRules::load(&root, true, None);
    assert!(!ignored(&rules, &root, "src", true));
    assert!(!ignored(&rules, &root, "src/a.txt", false));
    assert!(!rules.chain_for("src", RuleFiles::default()).1);
}

#[test]
fn git_directory_targets_stay_local_and_inside_the_root_unless_the_root_owns_them() {
    let root = std::env::temp_dir().join("sage-git-target-root");
    let nested = root.join("vendor/lib");
    assert_eq!(
        git_target(&nested, " ../../.git/modules/lib \n", &root, false),
        Some(root.join(".git/modules/lib"))
    );
    for target in ["//host/share", "\\\\host\\share", "", "../../../x"] {
        assert_eq!(git_target(&nested, target, &root, false), None, "{target}");
    }
    let escape = format!("{}net/host/x", "../".repeat(40));
    assert_eq!(git_target(&nested, &escape, &root, false), None);
    // `git worktree add ./wt` writes an absolute path inside the Project.
    let inside = root.join(".git/worktrees/wt");
    assert_eq!(
        git_target(&root.join("wt"), inside.to_str().unwrap(), &root, false),
        Some(inside)
    );
    let outside = std::env::temp_dir().join("sage-main-repo/.git/worktrees/x");
    let outside_text = outside.to_str().unwrap();
    assert_eq!(git_target(&root, outside_text, &root, false), None);
    assert_eq!(git_target(&root, outside_text, &root, true), Some(outside));
    assert_eq!(git_target(&root, "//host/share", &root, true), None);
}

#[test]
fn a_nested_git_file_pointing_outside_the_root_is_not_followed() {
    let outside = Fixture::new("gitdir-outside");
    outside.write("info/exclude", "secret.txt\n");
    let fixture = Fixture::new("gitdir");
    fixture.write(
        "vendor/lib/.git",
        &format!("gitdir: {}\n", outside.0.display()),
    );
    fixture.write(
        "wt/.git",
        &format!(
            "gitdir: {}\n",
            fixture.0.join(".git/worktrees/wt").display()
        ),
    );
    fixture.write(".git/worktrees/wt/commondir", "../..\n");
    fixture.write(".git/info/exclude", "local.txt\n");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    assert!(!ignored(&rules, &fixture.0, "vendor/lib/secret.txt", false));
    assert!(ignored(&rules, &fixture.0, "wt/local.txt", false));
}

#[test]
fn a_nested_repository_stops_outer_gitignore_files_but_not_dot_ignore() {
    let fixture = Fixture::new("nested-repo");
    fixture
        .write(".gitignore", "*.txt\n")
        .write(".ignore", "*.bak\n")
        .write("vendor/lib/.git/info/exclude", "local.md\n");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    let root = &fixture.0;
    assert!(ignored(&rules, root, "a.txt", false));
    assert!(!ignored(&rules, root, "vendor/lib/a.txt", false));
    assert!(ignored(&rules, root, "vendor/lib/a.bak", false));
    assert!(ignored(&rules, root, "vendor/lib/local.md", false));
}

#[test]
fn disabled_rules_read_nothing_and_ignore_nothing() {
    let fixture = Fixture::new("disabled");
    fixture.write(".gitignore", "*\n");
    let rules = IgnoreRules::load(&fixture.0, false, None);
    assert!(!ignored(&rules, &fixture.0, "anything.txt", false));
    assert!(!rules.chain_for("a/b", RuleFiles::default()).1);
}

#[test]
fn chain_for_reports_a_folder_inside_an_ignored_folder() {
    let fixture = Fixture::new("chain-for");
    fixture
        .write(".gitignore", "target/\n")
        .write("src/.gitignore", "*.gen\n")
        .dir("target/debug")
        .dir("src/deep");
    let rules = IgnoreRules::load(&fixture.0, true, None);
    assert!(rules.chain_for("target", RuleFiles::default()).1);
    assert!(rules.chain_for("target/debug", RuleFiles::default()).1);
    let (chain, ignored_folder) = rules.chain_for("src/deep", RuleFiles::default());
    assert!(!ignored_folder);
    assert!(rules.is_ignored(&chain, "src/deep/x.gen", false));
    let (root_chain, root_ignored) = rules.chain_for("", RuleFiles::probe(&fixture.0));
    assert!(!root_ignored);
    assert!(rules.is_ignored(&root_chain, "target", true));
}

#[test]
fn skips_a_rule_file_over_the_size_cap() {
    let fixture = Fixture::new("oversize");
    let mut huge = "*.big\n".to_string();
    huge.push_str(&"#".repeat(MAX_RULES_FILE_BYTES as usize));
    fixture.write(".gitignore", &huge);
    let rules = IgnoreRules::load(&fixture.0, true, None);
    assert!(!ignored(&rules, &fixture.0, "a.big", false));
}

#[cfg(unix)]
#[test]
fn never_follows_a_rule_file_link_out_of_the_root() {
    let outside = Fixture::new("escape-outside");
    outside.write("rules", "*\n");
    let fixture = Fixture::new("escape");
    fixture.dir("sub").write("sub/file.txt", "");
    std::os::unix::fs::symlink(outside.0.join("rules"), fixture.0.join("sub/.gitignore")).unwrap();
    std::os::unix::fs::symlink(outside.0.join("rules"), fixture.0.join(".ignore")).unwrap();
    let rules = IgnoreRules::load(&fixture.0, true, None);
    assert!(!ignored(&rules, &fixture.0, "sub/file.txt", false));
    assert!(!ignored(&rules, &fixture.0, "top.txt", false));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_fifo_named_like_a_rule_file_does_not_stall_the_walk() {
    let fixture = Fixture::new("fifo");
    let path = std::ffi::CString::new(fixture.0.join(".gitignore").as_os_str().as_encoded_bytes())
        .unwrap();
    // SAFETY: `path` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let rules = IgnoreRules::load(&fixture.0, true, None);
    assert!(!ignored(&rules, &fixture.0, "a.txt", false));
}

#[test]
fn notes_rule_names_from_a_listing() {
    let mut files = RuleFiles::default();
    for name in [".gitignore", "src", ".git", "README.md"] {
        files.note(OsStr::new(name));
    }
    assert_eq!(
        files,
        RuleFiles {
            ignore: false,
            gitignore: true,
            repository: true
        }
    );
}

#[test]
fn only_a_missing_dot_git_means_no_repository() {
    let dot_git = Path::new("project").join(".git");
    let missing = std::io::Error::from(ErrorKind::NotFound);
    assert!(matches!(present(&dot_git, Err(missing)), Ok(None)));
    let denied = std::io::Error::from(ErrorKind::PermissionDenied);
    assert!(matches!(present(&dot_git, Err(denied)), Err(Skipped)));
}
