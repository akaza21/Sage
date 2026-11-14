//! The staged (index) version of a File, which the editor gutter compares the
//! live document against (EP-14, AD-7). As in VS Code, the gutter shows what
//! is not yet staged, so staging a hunk removes its marker.

use sage_core::error::AppResult;
use serde::Serialize;
use tauri::State;

use crate::{
    editor_content::MAX_EDITABLE_BYTES,
    repository::{RepositoryBoundary, ensure_success, repository_boundary},
    repository_workspace::run_git,
    state::AppState,
    workspace::Workspace,
};
use sage_domain::resource::ResourceRef;

/// One `ls-files --stage` record: mode, id, stage and a capped path.
const MAX_INDEX_RECORD_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum FileGitBase {
    /// No markers: untracked, ignored, conflicted, not a text blob, over the
    /// editable cap, or the root is not a repository top level.
    Unavailable,
    /// The staged blob is still `oid`, which the caller already holds.
    Unchanged {
        oid: String,
    },
    Ready {
        oid: String,
        content: String,
    },
}

#[tauri::command]
pub async fn get_file_git_base(
    state: State<'_, AppState>,
    resource: ResourceRef,
    known_oid: Option<String>,
) -> Result<FileGitBase, String> {
    let workspace = state
        .database
        .resolve_active_resource(&resource)
        .map_err(|error| error.to_string())?;
    file_git_base(&workspace, &resource, known_oid.as_deref())
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn file_git_base(
    workspace: &Workspace,
    resource: &ResourceRef,
    known_oid: Option<&str>,
) -> AppResult<FileGitBase> {
    let relative_path = workspace.validate_resource(resource, false)?;
    if !matches!(
        repository_boundary(workspace.root()).await?,
        RepositoryBoundary::Exact
    ) {
        return Ok(FileGitBase::Unavailable);
    }
    // The boundary is classified first, as status does: the anchored runner
    // needs a repository root and would fail on a plain folder. Both reads
    // then go through it, holding the root and checking its identity, so a
    // swapped root reads nothing.
    // Sage runs Git with `--literal-pathspecs`, so the path is never a glob.
    let entry = run_git(
        workspace,
        &["ls-files", "--stage", "-z", "--", &relative_path],
        MAX_INDEX_RECORD_BYTES,
    )
    .await?;
    ensure_success(&entry)?;
    let Some(oid) = staged_text_blob(&entry.stdout, &relative_path) else {
        return Ok(FileGitBase::Unavailable);
    };
    if known_oid == Some(oid.as_str()) {
        return Ok(FileGitBase::Unchanged { oid });
    }
    let blob = run_git(
        workspace,
        &["cat-file", "blob", &oid],
        MAX_EDITABLE_BYTES as usize,
    )
    .await?;
    ensure_success(&blob)?;
    if blob.stdout_truncated {
        return Ok(FileGitBase::Unavailable);
    }
    Ok(
        text_content(blob.stdout).map_or(FileGitBase::Unavailable, |content| FileGitBase::Ready {
            oid,
            content,
        }),
    )
}

/// The object id of the one stage-0 regular-file entry for `path`, if any.
/// A conflicted path has stages 1–3; a symlink or submodule has no text.
fn staged_text_blob(output: &[u8], path: &str) -> Option<String> {
    let mut records = output.split(|byte| *byte == 0).filter(|r| !r.is_empty());
    let record = std::str::from_utf8(records.next()?).ok()?;
    if records.next().is_some() {
        return None;
    }
    let (meta, entry_path) = record.split_once('\t')?;
    let mut fields = meta.split(' ');
    let (mode, oid, stage) = (fields.next()?, fields.next()?, fields.next()?);
    let regular = mode == "100644" || mode == "100755";
    let valid_oid = matches!(oid.len(), 40 | 64) && oid.bytes().all(|b| b.is_ascii_hexdigit());
    (regular && valid_oid && stage == "0" && entry_path == path && fields.next().is_none())
        .then(|| oid.to_owned())
}

/// The first line of a Git LFS pointer, which stands in for the real file.
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs.github.com/spec/";

/// UTF-8 text without NUL bytes, as the editor would open it. An LFS pointer
/// is not the file's text, so comparing against it would mark every line.
fn text_content(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) || bytes.starts_with(LFS_POINTER_PREFIX) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use super::*;

    const OID: &str = "0123456789abcdef0123456789abcdef01234567";

    fn git(root: &Path, args: &[&str]) {
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    fn repository() -> (std::path::PathBuf, Workspace) {
        let root = std::env::temp_dir().join(format!("sage-git-base-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let root = workspace.root().to_path_buf();
        (root, workspace)
    }

    fn base(workspace: &Workspace, path: &str, known: Option<&str>) -> FileGitBase {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let resource = workspace.resource(path).unwrap();
        runtime
            .block_on(file_git_base(workspace, &resource, known))
            .unwrap()
    }

    fn oid_of(base: &FileGitBase) -> String {
        match base {
            FileGitBase::Ready { oid, .. } | FileGitBase::Unchanged { oid } => oid.clone(),
            FileGitBase::Unavailable => panic!("expected a base"),
        }
    }

    #[test]
    fn returns_the_staged_text_and_reports_an_unchanged_blob() {
        let (root, workspace) = repository();
        fs::write(root.join("a.txt"), "one\r\ntwo\n").unwrap();
        git(&root, &["add", "a.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("a.txt"), "edited\n").unwrap();

        let first = base(&workspace, "a.txt", None);
        let FileGitBase::Ready { content, .. } = &first else {
            panic!("expected text, got {first:?}");
        };
        assert_eq!(content, "one\r\ntwo\n");
        let oid = oid_of(&first);
        assert_eq!(
            base(&workspace, "a.txt", Some(&oid)),
            FileGitBase::Unchanged { oid: oid.clone() }
        );

        // Staging moves the base, as VS Code's gutter does.
        git(&root, &["add", "a.txt"]);
        let staged = base(&workspace, "a.txt", Some(&oid));
        assert_eq!(
            staged,
            FileGitBase::Ready {
                oid: oid_of(&staged),
                content: "edited\n".into()
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn untracked_ignored_and_binary_files_have_no_base() {
        let (root, workspace) = repository();
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("ignored.txt"), "x\n").unwrap();
        fs::write(root.join("new.txt"), "x\n").unwrap();
        fs::write(root.join("bin.dat"), b"a\0b").unwrap();
        fs::write(root.join("[weird].txt"), "literal\n").unwrap();
        git(&root, &["add", ".gitignore", "bin.dat", "[weird].txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);

        assert_eq!(base(&workspace, "new.txt", None), FileGitBase::Unavailable);
        assert_eq!(
            base(&workspace, "ignored.txt", None),
            FileGitBase::Unavailable
        );
        assert_eq!(base(&workspace, "bin.dat", None), FileGitBase::Unavailable);
        // Glob characters in a name are taken literally.
        assert!(matches!(
            base(&workspace, "[weird].txt", None),
            FileGitBase::Ready { content, .. } if content == "literal\n"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_plain_folder_has_no_base() {
        let root = std::env::temp_dir().join(format!("sage-git-base-plain-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), "x\n").unwrap();
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        assert_eq!(base(&workspace, "a.txt", None), FileGitBase::Unavailable);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_only_one_stage_zero_regular_file() {
        let record =
            |mode: &str, stage: &str, path: &str| format!("{mode} {OID} {stage}\t{path}\0");
        assert_eq!(
            staged_text_blob(record("100644", "0", "a").as_bytes(), "a"),
            Some(OID.into())
        );
        assert_eq!(
            staged_text_blob(record("100755", "0", "a").as_bytes(), "a"),
            Some(OID.into())
        );
        for (mode, stage, path) in [
            ("120000", "0", "a"),
            ("160000", "0", "a"),
            ("100644", "2", "a"),
            ("100644", "0", "b"),
        ] {
            assert_eq!(
                staged_text_blob(record(mode, stage, path).as_bytes(), "a"),
                None
            );
        }
        let conflicted = format!(
            "{}{}",
            record("100644", "1", "a"),
            record("100644", "2", "a")
        );
        assert_eq!(staged_text_blob(conflicted.as_bytes(), "a"), None);
        assert_eq!(staged_text_blob(b"", "a"), None);
    }

    #[test]
    fn text_content_refuses_binary_or_invalid_utf8() {
        assert_eq!(text_content(b"hi".to_vec()), Some("hi".into()));
        assert_eq!(text_content(vec![b'a', 0]), None);
        assert_eq!(text_content(vec![0xff, 0xfe]), None);
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 3\n";
        assert_eq!(text_content(pointer.to_vec()), None);
    }
}
