use sage_core::error::{AppError, AppResult};

use sage_workspace::Workspace;

pub const MAX_REMOTES: usize = 16;
const MAX_REMOTE_NAME_BYTES: usize = 255;
const MAX_REMOTE_LIST_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteList {
    pub names: Vec<String>,
    pub truncated: bool,
}

pub(crate) fn parse_remote_names(bytes: &[u8], output_truncated: bool) -> AppResult<RemoteList> {
    let bytes = trim_trailing_newlines(bytes);
    let mut names = Vec::new();
    let mut truncated = output_truncated;
    for record in remote_records(bytes)? {
        let name = record.trim_matches(['\r', '\n']);
        if name.is_empty() {
            continue;
        }
        if names.len() == MAX_REMOTES {
            truncated = true;
            break;
        }
        if !valid_remote_name(name) {
            return Err(AppError::Git("Git returned an invalid remote name".into()));
        }
        names.push(name.to_owned());
    }
    Ok(RemoteList { names, truncated })
}

pub(crate) async fn list_remotes(workspace: &Workspace) -> AppResult<RemoteList> {
    let output =
        crate::repository_workspace::run_git(workspace, &["remote"], MAX_REMOTE_LIST_BYTES).await?;
    crate::repository::ensure_success(&output)?;
    parse_remote_names(&output.stdout, output.stdout_truncated)
}

pub(crate) fn admit_listed_remote<'a>(
    listed: &'a RemoteList,
    requested: &str,
) -> AppResult<&'a str> {
    if !valid_remote_name(requested) {
        return Err(AppError::InvalidRequest(
            "Git fetch and push require an exact listed remote name".into(),
        ));
    }
    listed
        .names
        .iter()
        .find(|name| name.as_str() == requested)
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Git fetch and push require an exact listed remote name".into(),
            )
        })
}

fn valid_remote_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_REMOTE_NAME_BYTES {
        return false;
    }
    if name.starts_with(['-', '.', '/']) || name.ends_with(['.', '/']) {
        return false;
    }
    if name.contains("..") || name.contains('\\') || name.contains('@') {
        return false;
    }
    !name
        .chars()
        .any(|ch| ch.is_control() || ch.is_ascii_whitespace() || "~^:?*[".contains(ch))
}

fn remote_records(bytes: &[u8]) -> AppResult<Vec<&str>> {
    bytes
        .split(|byte| *byte == b'\n')
        .map(|record| {
            std::str::from_utf8(record)
                .map_err(|_| AppError::Git("Git returned a non-UTF-8 remote name".into()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_newline_delimited_remote_names() {
        let listed = parse_remote_names(b"origin\nupstream\n", false).unwrap();
        assert_eq!(listed.names, ["origin", "upstream"]);
        assert!(!listed.truncated);
    }

    #[test]
    fn empty_remote_list_is_empty() {
        let listed = parse_remote_names(b"", false).unwrap();
        assert!(listed.names.is_empty());
        assert!(!listed.truncated);
    }

    #[test]
    fn rejects_an_invalid_remote_name() {
        assert!(parse_remote_names(b"ori gin\n", false).is_err());
        assert!(parse_remote_names(b"-origin\n", false).is_err());
        assert!(parse_remote_names(b"origin@{push}\n", false).is_err());
    }

    #[test]
    fn caps_the_list_at_sixteen_remotes() {
        let mut bytes = Vec::new();
        for index in 0..20 {
            bytes.extend_from_slice(format!("r{index}\n").as_bytes());
        }
        let listed = parse_remote_names(&bytes, false).unwrap();
        assert_eq!(listed.names.len(), 16);
        assert_eq!(listed.names[0], "r0");
        assert_eq!(listed.names[15], "r15");
        assert!(listed.truncated);
    }

    #[test]
    fn truncated_output_marks_the_list() {
        let listed = parse_remote_names(b"origin\n", true).unwrap();
        assert_eq!(listed.names, ["origin"]);
        assert!(listed.truncated);
    }

    #[test]
    fn admits_only_an_exact_listed_remote_name() {
        let listed = parse_remote_names(b"origin\nupstream\n", false).unwrap();
        assert_eq!(admit_listed_remote(&listed, "origin").unwrap(), "origin");
        assert!(admit_listed_remote(&listed, "missing").is_err());
        assert!(admit_listed_remote(&listed, "https://example.invalid/repo.git").is_err());
        assert!(admit_listed_remote(&listed, "origin@origin").is_err());
    }
}
