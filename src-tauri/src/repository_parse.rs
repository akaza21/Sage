use std::collections::HashMap;

use crate::{
    domain::{DiffHunk, DiffLine, FileChange, FileDiff, ResourceRef},
    error::{AppError, AppResult},
};

const MAX_CHANGES: usize = 300;
pub const MAX_DIFF_LINES: usize = 1_200;

pub fn parse_status(
    root_id: &str,
    bytes: &[u8],
    output_truncated: bool,
) -> AppResult<(Option<String>, Vec<FileChange>, bool)> {
    let bytes = complete_nul_records(bytes, output_truncated);
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut branch_head = None;
    let mut branch_oid = None;
    let mut changes = Vec::new();
    let mut truncated = output_truncated;
    let mut index = 0;

    while index < fields.len() {
        let record = std::str::from_utf8(fields[index])
            .map_err(|_| AppError::Git("Git returned a non-UTF-8 path".into()))?;
        index += 1;
        if record.is_empty() {
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                branch_oid = Some(value.to_owned());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.head ") {
            branch_head = Some(value.to_owned());
            continue;
        }
        if record.starts_with("# ") || record.starts_with("! ") {
            continue;
        }
        if changes.len() == MAX_CHANGES {
            truncated = true;
            break;
        }

        let change = if let Some(path) = record.strip_prefix("? ") {
            Some(FileChange {
                resource: resource(root_id, path),
                original_resource: None,
                index_status: "unchanged".into(),
                worktree_status: "untracked".into(),
                additions: None,
                deletions: None,
            })
        } else if record.starts_with("1 ") {
            parse_ordinary_change(root_id, record)?
        } else if record.starts_with("2 ") {
            let original_path = fields
                .get(index)
                .ok_or_else(|| AppError::Git("rename record is incomplete".into()))?;
            if original_path.is_empty() {
                if output_truncated {
                    truncated = true;
                    break;
                }
                return Err(AppError::Git("rename record is incomplete".into()));
            }
            index += 1;
            parse_renamed_change(
                root_id,
                record,
                std::str::from_utf8(original_path)
                    .map_err(|_| AppError::Git("Git returned a non-UTF-8 original path".into()))?,
            )?
        } else if record.starts_with("u ") {
            parse_unmerged_change(root_id, record)?
        } else {
            None
        };

        if let Some(change) = change {
            changes.push(change);
        }
    }

    let branch = match branch_head.as_deref() {
        Some("(detached)") => Some(match branch_oid {
            Some(oid) => format!("{} (detached)", &oid[..oid.len().min(8)]),
            None => "HEAD (detached)".into(),
        }),
        Some(head) => Some(head.into()),
        None => None,
    };
    Ok((branch, changes, truncated))
}

fn parse_ordinary_change(root_id: &str, record: &str) -> AppResult<Option<FileChange>> {
    let parts = record.splitn(9, ' ').collect::<Vec<_>>();
    if parts.len() != 9 {
        return Err(AppError::Git("ordinary status record is malformed".into()));
    }
    Ok(Some(change_from_xy(root_id, parts[8], None, parts[1])?))
}

fn parse_renamed_change(
    root_id: &str,
    record: &str,
    original_path: &str,
) -> AppResult<Option<FileChange>> {
    let parts = record.splitn(10, ' ').collect::<Vec<_>>();
    if parts.len() != 10 {
        return Err(AppError::Git("rename status record is malformed".into()));
    }
    Ok(Some(change_from_xy(
        root_id,
        parts[9],
        Some(original_path.into()),
        parts[1],
    )?))
}

fn parse_unmerged_change(root_id: &str, record: &str) -> AppResult<Option<FileChange>> {
    let parts = record.splitn(11, ' ').collect::<Vec<_>>();
    if parts.len() != 11 {
        return Err(AppError::Git("unmerged status record is malformed".into()));
    }
    Ok(Some(FileChange {
        resource: resource(root_id, parts[10]),
        original_resource: None,
        index_status: "conflicted".into(),
        worktree_status: "conflicted".into(),
        additions: None,
        deletions: None,
    }))
}

fn change_from_xy(
    root_id: &str,
    path: &str,
    original_path: Option<String>,
    xy: &str,
) -> AppResult<FileChange> {
    let mut codes = xy.chars();
    let index = codes
        .next()
        .ok_or_else(|| AppError::Git("status record has no index state".into()))?;
    let worktree = codes
        .next()
        .ok_or_else(|| AppError::Git("status record has no worktree state".into()))?;
    Ok(FileChange {
        resource: resource(root_id, path),
        original_resource: original_path.map(|path| resource(root_id, &path)),
        index_status: status_name(index).into(),
        worktree_status: status_name(worktree).into(),
        additions: None,
        deletions: None,
    })
}

fn status_name(code: char) -> &'static str {
    match code {
        '.' | ' ' => "unchanged",
        'M' => "modified",
        'T' => "typeChanged",
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'U' => "conflicted",
        '?' => "untracked",
        _ => "unknown",
    }
}

pub fn merge_numstat(
    stats: &mut HashMap<String, (Option<u64>, Option<u64>)>,
    bytes: &[u8],
    output_truncated: bool,
) -> AppResult<bool> {
    let bytes = complete_nul_records(bytes, output_truncated);
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut index = 0;
    while index < fields.len() {
        let record = std::str::from_utf8(fields[index])
            .map_err(|_| AppError::Git("Git returned non-UTF-8 numstat data".into()))?;
        index += 1;
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, '\t');
        let additions = parse_count(parts.next());
        let deletions = parse_count(parts.next());
        let mut path = parts.next().unwrap_or_default();
        if path.is_empty() {
            let new_path = fields.get(index + 1).copied().unwrap_or_default();
            if new_path.is_empty() {
                if output_truncated {
                    return Ok(true);
                }
                return Err(AppError::Git("rename numstat record is incomplete".into()));
            }
            path = std::str::from_utf8(new_path)
                .map_err(|_| AppError::Git("Git returned a non-UTF-8 rename path".into()))?;
            index += 2;
        }
        if path.is_empty() {
            continue;
        }
        let current = stats.entry(path.into()).or_insert((Some(0), Some(0)));
        current.0 = add_optional(current.0, additions);
        current.1 = add_optional(current.1, deletions);
    }
    Ok(output_truncated)
}

fn complete_nul_records(bytes: &[u8], output_truncated: bool) -> &[u8] {
    if !output_truncated {
        return bytes;
    }
    bytes
        .iter()
        .rposition(|byte| *byte == 0)
        .map_or(&[], |last| &bytes[..=last])
}

fn parse_count(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse().ok())
}

fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    left.zip(right).map(|(left, right)| left + right)
}

pub fn parse_unified_diff(
    resource: &ResourceRef,
    raw: &str,
    output_truncated: bool,
) -> AppResult<FileDiff> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0;
    let mut new_line = 0;
    let mut line_count = 0;
    let mut truncated = output_truncated;
    let binary = raw.lines().any(|line| line.starts_with("Binary files "));

    for line in raw.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            let (old_start, new_start) = parse_hunk_starts(header)?;
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                header: format!("@@ {header}"),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = &mut current else {
            continue;
        };
        if line_count == MAX_DIFF_LINES {
            truncated = true;
            break;
        }

        let (kind, old_number, new_number, text) = if let Some(text) = line.strip_prefix('+') {
            let number = new_line;
            new_line += 1;
            ("added", None, Some(number), text)
        } else if let Some(text) = line.strip_prefix('-') {
            let number = old_line;
            old_line += 1;
            ("removed", Some(number), None, text)
        } else if let Some(text) = line.strip_prefix(' ') {
            let old_number = old_line;
            let new_number = new_line;
            old_line += 1;
            new_line += 1;
            ("context", Some(old_number), Some(new_number), text)
        } else {
            ("meta", None, None, line)
        };
        hunk.lines.push(DiffLine {
            kind: kind.into(),
            old_line: old_number,
            new_line: new_number,
            text: text.into(),
        });
        line_count += 1;
    }
    if let Some(hunk) = current {
        hunks.push(hunk);
    }

    Ok(FileDiff {
        resource: resource.clone(),
        hunks,
        binary,
        truncated,
    })
}

fn parse_hunk_starts(header: &str) -> AppResult<(u32, u32)> {
    let mut parts = header.split_whitespace();
    let old = parts
        .next()
        .and_then(|value| value.strip_prefix('-'))
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.parse().ok());
    let new = parts
        .next()
        .and_then(|value| value.strip_prefix('+'))
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.parse().ok());
    old.zip(new)
        .ok_or_else(|| AppError::Git("diff hunk header is malformed".into()))
}

pub fn added_file_diff(resource: ResourceRef, content: &str, content_truncated: bool) -> FileDiff {
    let lines = content
        .lines()
        .take(MAX_DIFF_LINES)
        .enumerate()
        .map(|(index, text)| DiffLine {
            kind: "added".into(),
            old_line: None,
            new_line: Some(index as u32 + 1),
            text: text.into(),
        })
        .collect();
    FileDiff {
        resource,
        hunks: vec![DiffHunk {
            header: format!("@@ -0,0 +1,{} @@", content.lines().count()),
            lines,
        }],
        binary: false,
        truncated: content_truncated,
    }
}

fn resource(root_id: &str, relative_path: &str) -> ResourceRef {
    ResourceRef {
        root_id: root_id.into(),
        relative_path: relative_path.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2_without_losing_spaces_in_paths() {
        let input = b"# branch.head main\0? notes/hello world.md\0\
              1 .M N... 100644 100644 100644 abcdef abcdef src/main.rs\0";
        let (branch, changes, truncated) = parse_status("root-test", input, false).unwrap();

        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].resource.relative_path, "notes/hello world.md");
        assert_eq!(changes[0].worktree_status, "untracked");
        assert_eq!(changes[1].resource.relative_path, "src/main.rs");
        assert_eq!(changes[1].worktree_status, "modified");
        assert!(!truncated);
    }

    #[test]
    fn truncated_status_drops_the_partial_final_record() {
        let input = b"# branch.head main\0? complete.txt\0? partial";
        let (branch, changes, truncated) = parse_status("root-test", input, true).unwrap();

        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].resource.relative_path, "complete.txt");
        assert!(truncated);
    }

    #[test]
    fn truncated_numstat_keeps_only_complete_records() {
        let input = b"1\t2\tcomplete.txt\0" as &[u8];
        let mut bytes = input.to_vec();
        bytes.extend_from_slice(b"9\t8\tpartial");
        let mut stats = HashMap::new();
        let truncated = merge_numstat(&mut stats, &bytes, true).unwrap();

        assert_eq!(stats.get("complete.txt"), Some(&(Some(1), Some(2))));
        assert!(!stats.contains_key("partial"));
        assert!(truncated);
    }

    #[test]
    fn detached_head_uses_the_short_object_id() {
        let input = b"# branch.oid 0123456789abcdef\0# branch.head (detached)\0";
        let (branch, changes, truncated) = parse_status("root-test", input, false).unwrap();

        assert_eq!(branch.as_deref(), Some("01234567 (detached)"));
        assert!(changes.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn parses_unified_diff_into_numbered_lines() {
        let raw = concat!(
            "diff --git a/src/main.rs b/src/main.rs\n",
            "--- a/src/main.rs\n",
            "+++ b/src/main.rs\n",
            "@@ -1,2 +1,2 @@\n",
            "-fn old() {}\n",
            "+fn new() {}\n",
            " context\n",
        );
        let diff = parse_unified_diff(&resource("root-test", "src/main.rs"), raw, false).unwrap();

        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines[0].old_line, Some(1));
        assert_eq!(diff.hunks[0].lines[1].new_line, Some(1));
        assert_eq!(diff.hunks[0].lines[2].new_line, Some(2));
    }
}
