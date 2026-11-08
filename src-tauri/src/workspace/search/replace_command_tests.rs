use std::{fs, path::PathBuf};

use uuid::Uuid;

use super::*;
use crate::{domain::BufferSnapshotRequest, editor, workspace::Workspace};

struct Fixture {
    root: PathBuf,
    database: Option<Database>,
    workspace: Workspace,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-replace-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let workspace = Workspace::open(&project.id, &project.roots[0].id, &root).unwrap();
        Self {
            root,
            database: Some(database),
            workspace,
        }
    }

    fn database(&self) -> &Database {
        self.database.as_ref().unwrap()
    }

    fn write(&self, path: &str, content: &str) -> ResourceRef {
        let absolute = self.root.join(path);
        fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        fs::write(&absolute, content).unwrap();
        self.workspace.resource(path).unwrap()
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
    }

    fn preview(
        &self,
        spec: &ReplaceSpec,
        resources: &[ResourceRef],
    ) -> WorkspaceReplacePreviewResponse {
        preview(
            self.database(),
            &Replacer::new(spec).unwrap(),
            resources,
            None,
        )
    }

    fn apply(
        &self,
        spec: &ReplaceSpec,
        targets: &[ReplaceTarget],
    ) -> WorkspaceReplaceApplyResponse {
        apply(self.database(), &Replacer::new(spec).unwrap(), targets)
    }

    /// Previews `resources` and replaces everything the preview saw.
    fn replace_all(
        &self,
        spec: &ReplaceSpec,
        resources: &[ResourceRef],
    ) -> WorkspaceReplaceApplyResponse {
        let preview = self.preview(spec, resources);
        self.apply(spec, &targets(&preview))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.database.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn spec(query: &str, replacement: &str) -> ReplaceSpec {
    ReplaceSpec {
        query: query.into(),
        replacement: replacement.into(),
        case_sensitive: false,
        whole_word: false,
        use_regex: false,
        preserve_case: false,
    }
}

fn targets(preview: &WorkspaceReplacePreviewResponse) -> Vec<ReplaceTarget> {
    preview
        .files
        .iter()
        .map(|file| ReplaceTarget {
            resource: file.resource.clone(),
            fingerprint: file.fingerprint.clone(),
            only: None,
        })
        .collect()
}

#[test]
fn previews_then_replaces_each_file_whole() {
    let fixture = Fixture::new();
    let first = fixture.write("src/a.txt", "old one\r\nold two\r\n");
    let second = fixture.write("src/b.txt", "nothing\n");
    let third = fixture.write("src/deep/c.txt", "the old line\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.root.join("src/a.txt"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }

    let preview = fixture.preview(
        &spec("old", "new"),
        &[first.clone(), second.clone(), third.clone()],
    );
    assert_eq!(preview.files.len(), 2);
    assert_eq!(preview.files[0].count, 2);
    assert_eq!(preview.files[0].lines[1].before, "old two");
    assert_eq!(preview.files[0].lines[1].after, "new two");
    assert_eq!(preview.problems.len(), 1);
    assert_eq!(preview.problems[0].kind, ReplaceProblemKind::NoMatches);

    let applied = fixture.apply(&spec("old", "new"), &targets(&preview));
    assert!(applied.problems.is_empty(), "{:?}", applied.problems);
    assert_eq!(
        applied
            .replaced
            .iter()
            .map(|file| file.count)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    // Line ends are kept as the File had them.
    assert_eq!(fixture.read("src/a.txt"), "new one\r\nnew two\r\n");
    assert_eq!(fixture.read("src/deep/c.txt"), "the new line\n");
    // The new File replaced the old at once: no temporary file is left.
    let names = fs::read_dir(fixture.root.join("src"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert!(
        names.iter().all(|name| !name.ends_with(".tmp")),
        "{names:?}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(fixture.root.join("src/a.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
    }
    // The fingerprint reported is the File's now.
    let reread = editor::read_for_replace(fixture.database(), &fixture.workspace, &first).unwrap();
    assert_eq!(reread.fingerprint, applied.replaced[0].fingerprint);
}

#[test]
fn refuses_a_file_that_changed_since_the_preview() {
    let fixture = Fixture::new();
    let resource = fixture.write("notes.md", "keep old\n");
    let preview = fixture.preview(&spec("old", "new"), std::slice::from_ref(&resource));
    // Someone edits the File after the preview.
    fixture.write("notes.md", "inserted\nkeep old\n");

    let applied = fixture.apply(&spec("old", "new"), &targets(&preview));

    assert!(applied.replaced.is_empty());
    assert_eq!(applied.problems.len(), 1);
    assert_eq!(applied.problems[0].kind, ReplaceProblemKind::Stale);
    assert!(
        applied.problems[0]
            .message
            .contains("changed since the preview")
    );
    assert_eq!(fixture.read("notes.md"), "inserted\nkeep old\n");
}

#[test]
fn refuses_protected_folders_and_leaves_them_alone() {
    let fixture = Fixture::new();
    let git = fixture.write(".git/config", "url = old\n");
    let modules = fixture.write("node_modules/pkg/index.js", "old\n");
    let other = fixture.write("src/ok.txt", "old\n");

    let preview = fixture.preview(&spec("old", "new"), &[git.clone(), modules.clone(), other]);
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.problems.len(), 2);
    assert!(
        preview
            .problems
            .iter()
            .all(|problem| problem.kind == ReplaceProblemKind::Refused
                && problem.message.contains("protected folders"))
    );

    // Even with a fingerprint in hand, an apply is refused.
    let fingerprint = crate::editor_content::fingerprint(b"url = old\n");
    let applied = fixture.apply(
        &spec("old", "new"),
        &[ReplaceTarget {
            resource: git,
            fingerprint,
            only: None,
        }],
    );
    assert_eq!(applied.problems[0].kind, ReplaceProblemKind::Refused);
    assert_eq!(fixture.read(".git/config"), "url = old\n");
    assert_eq!(fixture.read("node_modules/pkg/index.js"), "old\n");
}

#[cfg(unix)]
#[test]
fn refuses_a_file_reached_through_a_link() {
    let fixture = Fixture::new();
    let outside = std::env::temp_dir().join(format!("sage-replace-outside-{}", Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("target.txt"), "old\n").unwrap();
    std::os::unix::fs::symlink(outside.join("target.txt"), fixture.root.join("link.txt")).unwrap();
    let resource = ResourceRef {
        root_id: fixture.workspace.root_id().into(),
        relative_path: "link.txt".into(),
    };

    let applied = fixture.replace_all(&spec("old", "new"), &[resource]);

    assert!(applied.replaced.is_empty());
    assert_eq!(
        fs::read_to_string(outside.join("target.txt")).unwrap(),
        "old\n"
    );
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn leaves_a_file_with_unsaved_editor_changes_to_the_editor() {
    let fixture = Fixture::new();
    let resource = fixture.write("draft.txt", "old\n");
    let preview = fixture.preview(&spec("old", "new"), std::slice::from_ref(&resource));
    let document =
        editor::open_document(fixture.database(), &fixture.workspace, &resource).unwrap();
    editor::snapshot_buffer(
        fixture.database(),
        &fixture.workspace,
        BufferSnapshotRequest {
            resource: resource.clone(),
            base: document.disk.clone(),
            line_ending: document.line_ending.clone(),
            content: "old, edited\n".into(),
            revision: 1,
        },
    )
    .unwrap();

    let applied = fixture.apply(&spec("old", "new"), &targets(&preview));

    assert_eq!(applied.problems[0].kind, ReplaceProblemKind::Refused);
    assert!(applied.problems[0].message.contains("unsaved changes"));
    assert_eq!(fixture.read("draft.txt"), "old\n");
}

#[test]
fn regex_groups_and_preserved_case_reach_the_file() {
    let fixture = Fixture::new();
    let resource = fixture.write(
        "lib.rs",
        "let Value = get_value();\nlet VALUE = get_value();\n",
    );
    let applied = fixture.replace_all(
        &ReplaceSpec {
            use_regex: true,
            preserve_case: true,
            ..spec(r"let (\w+) = get_(\w+)\(\)", "let $1 = fetch_$2()")
        },
        &[resource],
    );
    assert_eq!(applied.replaced[0].count, 2);
    assert_eq!(
        fixture.read("lib.rs"),
        "let Value = fetch_value();\nlet VALUE = fetch_value();\n"
    );
}

#[test]
fn replaces_only_the_named_match() {
    let fixture = Fixture::new();
    let resource = fixture.write("list.txt", "a a\na\n");
    let preview = fixture.preview(&spec("a", "b"), std::slice::from_ref(&resource));
    let applied = fixture.apply(
        &spec("a", "b"),
        &[ReplaceTarget {
            only: Some(MatchPosition {
                line_number: 1,
                column_number: 3,
            }),
            ..targets(&preview).remove(0)
        }],
    );
    assert_eq!(applied.replaced[0].count, 1);
    assert_eq!(fixture.read("list.txt"), "a b\na\n");
}

#[test]
fn a_file_past_the_editor_limit_is_refused_unread() {
    let fixture = Fixture::new();
    let big = "old\n".repeat((MAX_EDITABLE_BYTES as usize / 4) + 1);
    let resource = fixture.write("big.txt", &big);
    let preview = fixture.preview(&spec("old", "new"), &[resource]);
    assert!(preview.files.is_empty());
    assert_eq!(preview.problems[0].kind, ReplaceProblemKind::Refused);
    assert!(
        preview.problems[0].message.contains("limit"),
        "{}",
        preview.problems[0].message
    );
}

#[test]
fn text_replace_is_bounded_and_matches_the_disk_rules() {
    let done = replace_text(&TextReplaceRequest {
        spec: ReplaceSpec {
            use_regex: true,
            ..spec(r"(\d+)px", "${1}rem")
        },
        text: "a: 4px;\nb: 8px;\n".into(),
        only: None,
    })
    .unwrap();
    assert_eq!(done.content, "a: 4rem;\nb: 8rem;\n");
    assert_eq!(done.count, 2);
    assert!(
        replace_text(&TextReplaceRequest {
            spec: spec("x", "y"),
            text: "x".repeat(MAX_EDITABLE_BYTES as usize + 1),
            only: None,
        })
        .is_err()
    );
    assert!(validate_files("project", MAX_REPLACE_FILES + 1).is_err());
    assert!(validate_files("project", 0).is_err());
}

#[test]
fn the_panel_shares_the_replace_limits() {
    use super::super::{replace::MAX_REPLACEMENT_BYTES, types::MAX_SEARCH_PAGES};
    let limits = include_str!("../../../../src/features/search/workspaceSearchLimits.ts");
    for expected in [
        format!("results: {MAX_SEARCH_RESULTS},"),
        format!("continuationPages: {MAX_SEARCH_PAGES},"),
        format!("replacementBytes: {MAX_REPLACEMENT_BYTES},"),
    ] {
        assert!(limits.contains(&expected), "{expected}");
    }
    assert_eq!(MAX_REPLACE_FILES, MAX_SEARCH_RESULTS * MAX_SEARCH_PAGES);
}
