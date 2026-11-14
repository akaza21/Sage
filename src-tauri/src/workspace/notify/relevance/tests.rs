use super::*;
use std::fs;

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-notify-relevance-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CHANGED: u32 = 3;
const RENAMED_OLD: u32 = 4;

fn ignored_below(folder: &str) -> Relevance {
    Relevance::IgnoredBelow(folder.to_string())
}

#[test]
fn drops_skipped_folders_and_folds_ignored_ones_into_their_top_folder() {
    let root = Root::new();
    root.write(".gitignore", "target/\n*.log\n");
    root.write("target/debug/deps/app.d", "");
    root.write("src/nested/.gitignore", "generated/\n");
    root.write("tools/.ignore", "fixtures/\n");
    let mut relevance = NoticeRelevance::new(&root.0);

    for (relative, expected) in [
        ("node_modules/react/index.js", Relevance::Drop),
        ("packages/web/node_modules/a.js", Relevance::Drop),
        (".git/objects/ab/cdef", Relevance::Drop),
        ("target/debug/deps/app.d", ignored_below("target")),
        (
            "target/debug/.fingerprint/app-1/out",
            ignored_below("target"),
        ),
        ("target/node_modules/x.js", Relevance::Drop),
        (
            "src/nested/generated/out.rs",
            ignored_below("src/nested/generated"),
        ),
        ("target", Relevance::Keep),
        ("src/main.rs", Relevance::Keep),
        ("src/nested/lib.rs", Relevance::Keep),
        ("build.log", Relevance::Keep),
        (".git", Relevance::Keep),
        ("tools/fixtures/case.json", Relevance::Keep),
    ] {
        assert_eq!(
            relevance.classify(relative, CHANGED),
            expected,
            "{relative}"
        );
    }
}

#[test]
fn a_gitignore_change_updates_the_filter() {
    let root = Root::new();
    root.write(".gitignore", "dist/\n");
    let mut relevance = NoticeRelevance::new(&root.0);
    assert_eq!(
        relevance.classify("dist/app.js", CHANGED),
        ignored_below("dist")
    );
    assert_eq!(relevance.classify("out/app.js", CHANGED), Relevance::Keep);

    root.write(".gitignore", "out/\n");
    assert_eq!(relevance.classify(".gitignore", CHANGED), Relevance::Keep);
    assert_eq!(relevance.classify("dist/app.js", CHANGED), Relevance::Keep);
    assert_eq!(
        relevance.classify("out/app.js", CHANGED),
        ignored_below("out")
    );
}

#[test]
fn rule_files_inside_dropped_or_ignored_folders_keep_the_cache() {
    let root = Root::new();
    root.write(".gitignore", "target/\n");
    let mut relevance = NoticeRelevance::new(&root.0);
    assert_eq!(
        relevance.classify("target/a", CHANGED),
        ignored_below("target")
    );

    // Unignoring `target` would show up only after a reload.
    root.write(".gitignore", "");
    for relative in [
        "node_modules/pkg/.gitignore",
        "target/vendor/.gitignore",
        "target/vendor/.git",
        "node_modules/pkg/.git/info/exclude",
        "target/sub/.git/info/exclude",
    ] {
        assert_ne!(
            relevance.classify(relative, RENAMED_OLD),
            Relevance::Keep,
            "{relative}"
        );
    }
    assert_eq!(
        relevance.classify("target/a", CHANGED),
        ignored_below("target")
    );
}

#[test]
fn a_repository_exclude_change_resyncs_consumers_and_updates_the_filter() {
    let root = Root::new();
    root.write(".git/info/exclude", "");
    root.write("sub/.git/info/exclude", "");
    let mut relevance = NoticeRelevance::new(&root.0);
    assert_eq!(
        relevance.classify("scratch/notes.md", CHANGED),
        Relevance::Keep
    );
    assert_eq!(
        relevance.classify("sub/cache/blob", CHANGED),
        Relevance::Keep
    );

    root.write(".git/info/exclude", "scratch/\n");
    assert_eq!(
        relevance.classify(".git/info/exclude", CHANGED),
        Relevance::Resync
    );
    assert_eq!(
        relevance.classify("scratch/notes.md", CHANGED),
        ignored_below("scratch")
    );

    root.write("sub/.git/info/exclude", "cache/\n");
    assert_eq!(
        relevance.classify("sub/.git/info/exclude", CHANGED),
        Relevance::Resync
    );
    assert_eq!(
        relevance.classify("sub/cache/blob", CHANGED),
        ignored_below("sub/cache")
    );
}

#[test]
fn bounded_cache_keeps_decisions_correct_when_an_ancestor_moves() {
    let root = Root::new();
    root.write(".gitignore", "target/\n");
    root.write("src/.gitignore", "generated/\n!generated-source/\n");
    root.write("lib/.gitignore", "cache/\n");
    let mut relevance = NoticeRelevance::new(&root.0);
    for index in 0..MAX_CACHED_FOLDERS - 1 {
        assert_eq!(
            relevance.classify(&format!("sibling-{index}/file"), CHANGED),
            Relevance::Keep
        );
    }
    assert_eq!(
        relevance.classify("lib/cache/blob", CHANGED),
        ignored_below("lib/cache")
    );
    assert!(relevance.folders.len() <= MAX_CACHED_FOLDERS);
    assert!(relevance.folder_bytes <= MAX_CACHED_FOLDER_BYTES);
    fs::remove_file(root.0.join("lib/.gitignore")).unwrap();
    assert_eq!(relevance.classify("lib", RENAMED_OLD), Relevance::Keep);
    assert_eq!(
        relevance.classify("lib/cache/blob", CHANGED),
        Relevance::Keep
    );
    for (path, expected) in [
        ("target/debug/out", ignored_below("target")),
        ("src/generated/out", ignored_below("src/generated")),
        ("src/generated-source/main.rs", Relevance::Keep),
        ("sibling-0/file", Relevance::Keep),
    ] {
        assert_eq!(relevance.classify(path, CHANGED), expected, "{path}");
    }
}

#[test]
fn a_moved_folder_is_decided_again() {
    let root = Root::new();
    root.write("lib/.gitignore", "cache/\n");
    let mut relevance = NoticeRelevance::new(&root.0);
    assert_eq!(
        relevance.classify("lib/cache/blob", CHANGED),
        ignored_below("lib/cache")
    );

    // Windows reports a changed parent for every entry created inside it.
    fs::remove_file(root.0.join("lib/.gitignore")).unwrap();
    assert_eq!(relevance.classify("lib", CHANGED), Relevance::Keep);
    assert_eq!(
        relevance.classify("lib/cache/blob", CHANGED),
        ignored_below("lib/cache")
    );

    assert_eq!(relevance.classify("lib", RENAMED_OLD), Relevance::Keep);
    assert_eq!(
        relevance.classify("lib/cache/blob", CHANGED),
        Relevance::Keep
    );
}
