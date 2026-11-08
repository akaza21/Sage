use crate::{
    error::{AppError, AppResult},
    workspace::Workspace,
};

pub const MAX_LOCAL_BRANCHES: usize = 64;
pub(crate) const MAX_BRANCH_NAME_BYTES: usize = 255;
const MAX_BRANCH_LIST_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalBranchList {
    pub names: Vec<String>,
    pub truncated: bool,
}

pub(crate) fn parse_local_branches(
    bytes: &[u8],
    output_truncated: bool,
) -> AppResult<LocalBranchList> {
    let bytes = trim_trailing_newlines(bytes);
    let mut names = Vec::new();
    let mut truncated = output_truncated;
    for record in branch_records(bytes, output_truncated)? {
        let name = record.trim_matches(['\r', '\n']);
        if name.is_empty() {
            continue;
        }
        if names.len() == MAX_LOCAL_BRANCHES {
            truncated = true;
            break;
        }
        if !valid_branch_name(name) {
            return Err(AppError::Git("Git returned an invalid branch name".into()));
        }
        names.push(name.to_owned());
    }
    Ok(LocalBranchList { names, truncated })
}

pub(crate) fn compose_current_named_branch(
    mut listed: LocalBranchList,
    current: Option<&str>,
) -> LocalBranchList {
    let Some(name) = named_branch(current) else {
        return listed;
    };
    if listed.names.iter().any(|existing| existing == name) {
        return listed;
    }
    if !valid_branch_name(name) {
        return listed;
    }
    if listed.names.len() >= MAX_LOCAL_BRANCHES {
        listed.truncated = true;
        return listed;
    }
    let insert_at = listed
        .names
        .iter()
        .position(|existing| existing.as_str() > name)
        .unwrap_or(listed.names.len());
    listed.names.insert(insert_at, name.to_owned());
    listed
}

pub(crate) fn named_branch(current: Option<&str>) -> Option<&str> {
    current.filter(|value| !value.ends_with(" (detached)"))
}

pub(crate) async fn list_local_branches(workspace: &Workspace) -> AppResult<LocalBranchList> {
    let output = crate::repository_workspace::run_git(
        workspace,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00",
            "--sort=refname",
            "refs/heads/",
        ],
        MAX_BRANCH_LIST_BYTES,
    )
    .await?;
    crate::repository::ensure_success(&output)?;
    parse_local_branches(&output.stdout, output.stdout_truncated)
}

pub(crate) fn valid_branch_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_BRANCH_NAME_BYTES {
        return false;
    }
    if name.starts_with(['-', '/', '.']) || name.ends_with(['/', '.']) {
        return false;
    }
    if name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name.contains('\\')
    {
        return false;
    }
    !name
        .chars()
        .any(|ch| ch.is_control() || ch.is_ascii_whitespace() || "~^:?*[".contains(ch))
}

fn branch_records(bytes: &[u8], output_truncated: bool) -> AppResult<Vec<&str>> {
    let fields = if bytes.contains(&0) {
        complete_nul_records(bytes, output_truncated)?
            .split(|byte| *byte == 0)
            .collect::<Vec<_>>()
    } else {
        bytes.split(|byte| *byte == b'\n').collect()
    };
    fields
        .into_iter()
        .map(|record| {
            std::str::from_utf8(record)
                .map_err(|_| AppError::Git("Git returned a non-UTF-8 branch name".into()))
        })
        .collect()
}

fn trim_trailing_newlines(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

fn complete_nul_records(bytes: &[u8], output_truncated: bool) -> AppResult<&[u8]> {
    if bytes.is_empty() {
        return Ok(bytes);
    }
    if !output_truncated && !bytes.ends_with(&[0]) {
        return Err(AppError::Git("Git branch list record is incomplete".into()));
    }
    if !output_truncated {
        return Ok(bytes);
    }
    Ok(bytes
        .iter()
        .rposition(|byte| *byte == 0)
        .map_or(&[][..], |last| &bytes[..=last]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_delimited_local_branch_names() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        assert_eq!(listed.names, ["main", "topic"]);
        assert!(!listed.truncated);
    }

    #[test]
    fn trailing_newlines_after_the_final_nul_are_ignored() {
        let listed = parse_local_branches(b"main\0topic\0\r\n", false).unwrap();
        assert_eq!(listed.names, ["main", "topic"]);
    }

    #[test]
    fn leftover_newline_between_nul_records_is_not_a_name() {
        let listed = parse_local_branches(b"main\0\ntopic\0\n", false).unwrap();
        assert_eq!(listed.names, ["main", "topic"]);
    }

    #[test]
    fn newline_delimited_names_are_accepted() {
        let listed = parse_local_branches(b"main\ntopic\n", false).unwrap();
        assert_eq!(listed.names, ["main", "topic"]);
    }

    #[test]
    fn empty_for_each_ref_is_an_empty_list() {
        let listed = parse_local_branches(b"", false).unwrap();
        assert!(listed.names.is_empty());
        assert!(!listed.truncated);
    }

    #[test]
    fn truncated_output_drops_the_partial_final_name() {
        let listed = parse_local_branches(b"main\0top", true).unwrap();
        assert_eq!(listed.names, ["main"]);
        assert!(listed.truncated);
    }

    #[test]
    fn incomplete_untruncated_nul_output_fails_closed() {
        assert!(parse_local_branches(b"main\0top", false).is_err());
    }

    #[test]
    fn rejects_dotdot_and_control_names() {
        assert!(parse_local_branches(b"feat..ure\0", false).is_err());
        assert!(parse_local_branches(b"bad\nname\0", false).is_err());
        assert!(parse_local_branches(b"-hidden\0", false).is_err());
    }

    #[test]
    fn accepts_nested_feature_names() {
        let listed = parse_local_branches(b"feature/foo\0", false).unwrap();
        assert_eq!(listed.names, ["feature/foo"]);
    }

    #[test]
    fn caps_the_list_at_sixty_four_names() {
        let mut bytes = Vec::new();
        for index in 0..65 {
            bytes.extend_from_slice(format!("b{index:02}").as_bytes());
            bytes.push(0);
        }
        let listed = parse_local_branches(&bytes, false).unwrap();
        assert_eq!(listed.names.len(), 64);
        assert_eq!(listed.names[0], "b00");
        assert_eq!(listed.names[63], "b63");
        assert!(listed.truncated);
    }

    #[test]
    fn compose_inserts_an_unborn_current_name() {
        let listed = parse_local_branches(b"", false).unwrap();
        let composed = compose_current_named_branch(listed, Some("main"));
        assert_eq!(composed.names, ["main"]);
        assert!(!composed.truncated);
    }

    #[test]
    fn compose_does_not_forge_a_detached_name() {
        let listed = parse_local_branches(b"main\0", false).unwrap();
        let composed = compose_current_named_branch(listed, Some("01234567 (detached)"));
        assert_eq!(composed.names, ["main"]);
        assert_eq!(named_branch(Some("HEAD (detached)")), None);
    }

    #[test]
    fn compose_keeps_sorted_order_when_inserting_current() {
        let listed = parse_local_branches(b"main\0topic\0", false).unwrap();
        let composed = compose_current_named_branch(listed, Some("hotfix"));
        assert_eq!(composed.names, ["hotfix", "main", "topic"]);
    }

    #[test]
    fn lists_local_heads_from_a_live_repository() {
        use std::fs;
        use uuid::Uuid;

        use crate::workspace::Workspace;

        let root = std::env::temp_dir().join(format!("sage-git-branches-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        for (key, value) in [
            ("user.email", "sage@example.invalid"),
            ("user.name", "Sage Test"),
            ("core.autocrlf", "false"),
        ] {
            assert!(
                crate::git_process::fixture_git()
                    .arg("-C")
                    .arg(&root)
                    .args(["config", key, value])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(root.join("readme.txt"), "first\n").unwrap();
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["add", "readme.txt"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["commit", "-q", "-m", "fixture"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["branch", "-m", "main"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["checkout", "-q", "-b", "topic"])
                .status()
                .unwrap()
                .success()
        );

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let listed = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_local_branches(&workspace))
            .unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(listed.names, ["main", "topic"]);
        assert!(!listed.truncated);
    }
}
