use sage_core::error::{AppError, AppResult};

use sage_domain::repository::GitRecentCommit;
use sage_workspace::Workspace;

pub const MAX_RECENT_COMMITS: usize = 64;
const MAX_COMMIT_SUBJECT_BYTES: usize = 255;
const MAX_COMMIT_LIST_BYTES: usize = 32 * 1024;
const GIT_SHA_BYTES: usize = 40;
const LOG_COUNT: &str = "65";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecentCommitList {
    pub commits: Vec<GitRecentCommit>,
    pub truncated: bool,
}

pub(crate) fn parse_recent_commits(
    bytes: &[u8],
    output_truncated: bool,
) -> AppResult<RecentCommitList> {
    let records = commit_records(bytes, output_truncated)?;
    let mut commits = Vec::new();
    let mut truncated = output_truncated;
    let mut fields = records.into_iter();
    while let Some(sha) = fields.next() {
        let Some(subject) = fields.next() else {
            if output_truncated {
                truncated = true;
                break;
            }
            return Err(AppError::Git("Git commit list record is incomplete".into()));
        };
        if commits.len() == MAX_RECENT_COMMITS {
            truncated = true;
            break;
        }
        commits.push(GitRecentCommit {
            sha: admit_sha(sha)?,
            subject: admit_subject(subject),
        });
    }
    Ok(RecentCommitList { commits, truncated })
}

pub(crate) async fn list_recent_commits(workspace: &Workspace) -> AppResult<RecentCommitList> {
    let head = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        256,
    )
    .await?;
    if !head.succeeded() {
        return Ok(RecentCommitList {
            commits: Vec::new(),
            truncated: false,
        });
    }
    let output = crate::repository_workspace::run_git(
        workspace,
        &[
            "log",
            "-n",
            LOG_COUNT,
            "--format=%H%x00%s%x00",
            "--no-decorate",
            "--",
        ],
        MAX_COMMIT_LIST_BYTES,
    )
    .await?;
    crate::repository::ensure_success(&output)?;
    parse_recent_commits(&output.stdout, output.stdout_truncated)
}

fn admit_sha(value: &[u8]) -> AppResult<String> {
    let value = std::str::from_utf8(value)
        .map_err(|_| AppError::Git("Git returned an invalid commit identity".into()))?;
    let sha = value.trim().to_ascii_lowercase();
    if sha.len() == GIT_SHA_BYTES
        && sha
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Ok(sha);
    }
    Err(AppError::Git(
        "Git returned an invalid commit identity".into(),
    ))
}

fn admit_subject(value: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(value);
    let mut subject = String::new();
    for ch in decoded.chars() {
        let ch = if ch.is_control() && ch != '\t' {
            '\u{fffd}'
        } else {
            ch
        };
        if subject.len() + ch.len_utf8() > MAX_COMMIT_SUBJECT_BYTES {
            break;
        }
        subject.push(ch);
    }
    subject
}

fn commit_records(bytes: &[u8], output_truncated: bool) -> AppResult<Vec<&[u8]>> {
    let bytes = trim_trailing_newlines(bytes);
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let complete = complete_nul_records(bytes, output_truncated)?;
    let mut records = complete.split(|byte| *byte == 0).collect::<Vec<_>>();
    if records.last().is_some_and(|record| record.is_empty()) {
        records.pop();
    }
    Ok(records)
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
        return Err(AppError::Git("Git commit list record is incomplete".into()));
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

    const FIRST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn pair(sha: &str, subject: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(sha.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(subject.as_bytes());
        bytes.push(0);
        bytes
    }

    #[test]
    fn parses_nul_delimited_sha_and_subject_pairs() {
        let mut bytes = pair(FIRST, "first");
        bytes.extend_from_slice(&pair(SECOND, "second"));
        let listed = parse_recent_commits(&bytes, false).unwrap();
        assert_eq!(
            listed.commits,
            [
                GitRecentCommit {
                    sha: FIRST.into(),
                    subject: "first".into(),
                },
                GitRecentCommit {
                    sha: SECOND.into(),
                    subject: "second".into(),
                },
            ]
        );
        assert!(!listed.truncated);
    }

    #[test]
    fn empty_log_is_an_empty_list() {
        let listed = parse_recent_commits(b"", false).unwrap();
        assert!(listed.commits.is_empty());
        assert!(!listed.truncated);
    }

    #[test]
    fn truncated_output_drops_the_partial_final_record() {
        let mut bytes = pair(FIRST, "first");
        bytes.extend_from_slice(b"bbbbbbbbbbbbbbbb");
        let listed = parse_recent_commits(&bytes, true).unwrap();
        assert_eq!(listed.commits[0].sha, FIRST);
        assert_eq!(listed.commits.len(), 1);
        assert!(listed.truncated);
    }

    #[test]
    fn incomplete_untruncated_nul_output_fails_closed() {
        let mut bytes = pair(FIRST, "first");
        bytes.extend_from_slice(b"bbbbbbbbbbbbbbbb");
        assert!(parse_recent_commits(&bytes, false).is_err());
    }

    #[test]
    fn rejects_a_non_hex_or_short_sha() {
        assert!(parse_recent_commits(&pair("deadbeef", "subject"), false).is_err());
        assert!(parse_recent_commits(&pair(&"g".repeat(40), "subject"), false).is_err());
    }

    #[test]
    fn normalizes_uppercase_sha_to_lowercase() {
        let listed =
            parse_recent_commits(&pair(&FIRST.to_ascii_uppercase(), "first"), false).unwrap();
        assert_eq!(listed.commits[0].sha, FIRST);
    }

    #[test]
    fn caps_the_list_at_sixty_four_commits() {
        let mut bytes = Vec::new();
        for index in 0..65 {
            let sha = format!("{index:040x}");
            bytes.extend_from_slice(&pair(&sha, &format!("c{index}")));
        }
        let listed = parse_recent_commits(&bytes, false).unwrap();
        assert_eq!(listed.commits.len(), 64);
        assert_eq!(listed.commits[0].sha, "0".repeat(40));
        assert_eq!(listed.commits[63].subject, "c63");
        assert!(listed.truncated);
    }

    #[test]
    fn bounds_an_oversize_subject_without_failing_the_list() {
        let listed = parse_recent_commits(&pair(FIRST, &"s".repeat(300)), false).unwrap();
        assert_eq!(listed.commits[0].subject.len(), 255);
        assert!(!listed.truncated);
    }

    #[test]
    fn empty_subject_stays_paired_with_its_sha() {
        let listed = parse_recent_commits(&pair(FIRST, ""), false).unwrap();
        assert_eq!(listed.commits[0].sha, FIRST);
        assert_eq!(listed.commits[0].subject, "");
    }

    #[test]
    fn unusual_subjects_stay_paired_with_their_sha() {
        for (subject, expected) in [
            (&b"before\rafter"[..], "before\u{fffd}after"),
            (&b"\x1b[31mred"[..], "\u{fffd}[31mred"),
            (&b"caf\xe9"[..], "caf\u{fffd}"),
            (
                "tab\tdelete\u{7f}next\u{85}".as_bytes(),
                "tab\tdelete\u{fffd}next\u{fffd}",
            ),
        ] {
            let mut bytes = pair(FIRST, "");
            bytes.pop();
            bytes.extend_from_slice(subject);
            bytes.push(0);
            bytes.extend_from_slice(&pair(SECOND, "normal"));
            let listed = parse_recent_commits(&bytes, false).unwrap();
            assert_eq!(listed.commits[0].sha, FIRST);
            assert_eq!(listed.commits[0].subject, expected);
            assert_eq!(listed.commits[1].sha, SECOND);
            assert_eq!(listed.commits[1].subject, "normal");
            assert!(!listed.truncated);
        }
    }

    #[test]
    fn replacement_expansion_is_bounded_on_a_character_boundary() {
        for suffix in ["\r", "é", "\u{fffd}"] {
            let subject = format!("{}{}tail", "a".repeat(254), suffix);
            let listed = parse_recent_commits(&pair(FIRST, &subject), false).unwrap();
            assert_eq!(listed.commits[0].subject, "a".repeat(254));
        }
        let listed = parse_recent_commits(&pair(FIRST, &"\r".repeat(100)), false).unwrap();
        assert_eq!(listed.commits[0].subject, "\u{fffd}".repeat(85));
        let mut bytes = pair(FIRST, &"a".repeat(253));
        bytes.pop();
        bytes.extend_from_slice(b"\xe9\0");
        let listed = parse_recent_commits(&bytes, false).unwrap();
        assert_eq!(listed.commits[0].subject, "a".repeat(253));
    }

    #[test]
    fn non_utf8_sha_still_fails_closed() {
        let mut bytes = pair(FIRST, "subject");
        bytes[0] = 0xe9;
        assert!(parse_recent_commits(&bytes, false).is_err());
    }

    #[test]
    fn lists_unusual_subjects_from_real_commit_objects() {
        use std::{io::Write, process::Stdio};

        let (root, workspace) = fixture_repository();
        let git = |args: &[&str]| {
            let output = crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        let tree = git(&["rev-parse", "HEAD^{tree}"]);
        let author = git(&["var", "GIT_AUTHOR_IDENT"]);
        let committer = git(&["var", "GIT_COMMITTER_IDENT"]);
        let mut parent = git(&["rev-parse", "HEAD"]);
        let mut expected = vec![
            GitRecentCommit {
                sha: git(&["rev-parse", "HEAD^"]),
                subject: "first".into(),
            },
            GitRecentCommit {
                sha: parent.clone(),
                subject: "second".into(),
            },
        ];
        // Byte 255 falls inside é, so the bound must stop at byte 254.
        let long_prefix = "a".repeat(254);
        let long_message = format!("{long_prefix}é{}\n", "z".repeat(4096));
        for (message, subject) in [
            (&b"\x1b[31mred\n"[..], "\u{fffd}[31mred"),
            (&b"caf\xe9\n"[..], "caf\u{fffd}"),
            (&b""[..], ""),
            (long_message.as_bytes(), long_prefix.as_str()),
            (
                &b"first line\nsecond line\n\nbody is not the subject\n"[..],
                "first line second line",
            ),
            (&b"--option\n"[..], "--option"),
            (&b"%H %s\n"[..], "%H %s"),
        ] {
            // Write the object verbatim: commit-tree may recode invalid UTF-8.
            let mut object =
                format!("tree {tree}\nparent {parent}\nauthor {author}\ncommitter {committer}\n\n")
                    .into_bytes();
            object.extend_from_slice(message);
            let mut child = crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["hash-object", "-t", "commit", "-w", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&object).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            parent = String::from_utf8(output.stdout).unwrap().trim().to_owned();
            let stored = crate::git_process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(["cat-file", "commit", &parent])
                .output()
                .unwrap();
            assert!(stored.status.success());
            assert_eq!(stored.stdout, object);
            expected.push(GitRecentCommit {
                sha: parent.clone(),
                subject: subject.into(),
            });
        }
        git(&["update-ref", "HEAD", &parent]);
        let listed = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_recent_commits(&workspace))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        expected.reverse();
        assert_eq!(listed.commits, expected);
        assert_eq!(listed.commits.len(), 9);
        assert!(
            listed
                .commits
                .iter()
                .all(|commit| commit.subject.len() <= MAX_COMMIT_SUBJECT_BYTES)
        );
        assert!(!listed.truncated);
    }

    fn fixture_repository() -> (std::path::PathBuf, Workspace) {
        use std::fs;
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-history-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "sage@example.invalid"],
            vec!["config", "user.name", "Sage Test"],
            vec!["config", "core.autocrlf", "false"],
        ] {
            assert!(
                crate::git_process::fixture_git()
                    .arg("-C")
                    .arg(&root)
                    .args(&args)
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
                .args(["commit", "-q", "-m", "first"])
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("readme.txt"), "second\n").unwrap();
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
                .args(["commit", "-q", "-m", "second"])
                .status()
                .unwrap()
                .success()
        );
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    #[test]
    fn lists_newest_first_from_a_live_repository() {
        let (root, workspace) = fixture_repository();
        let listed = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_recent_commits(&workspace))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(listed.commits.len(), 2);
        assert_eq!(listed.commits[0].subject, "second");
        assert_eq!(listed.commits[1].subject, "first");
        assert_eq!(listed.commits[0].sha.len(), 40);
        assert!(!listed.truncated);
    }

    #[test]
    fn unborn_head_is_an_empty_list() {
        use std::fs;
        use uuid::Uuid;

        let root = std::env::temp_dir().join(format!("sage-git-history-empty-{}", Uuid::new_v4()));
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
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let listed = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_recent_commits(&workspace))
            .unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(listed.commits.is_empty());
        assert!(!listed.truncated);
    }
}
