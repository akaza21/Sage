use std::{io::Read, time::Instant};

use crate::workspace::Workspace;

use super::{
    contained_file::open_contained_file,
    engine::{RootCounters, SearchBudget, SearchEngine},
    types::{
        ContinuationCursor, SearchLimitReason, SearchRootReport, SearchRootState,
        WorkspaceSearchMatch, push_reason,
    },
};

pub(super) struct ContinuationSeek {
    pub(super) cursor: ContinuationCursor,
    pub(super) started_at: Instant,
    pub(super) entries_visited: u64,
}

impl SearchEngine {
    pub(super) fn limit_reached_for_phase(
        &self,
        budget: &mut SearchBudget,
        result_count: usize,
        seek: Option<&ContinuationSeek>,
    ) -> bool {
        if let Some(seek) = seek {
            return seek.entries_visited >= self.limits.max_entries
                || seek.started_at.elapsed() >= self.limits.max_duration;
        }
        self.limit_reached(budget, result_count)
    }

    pub(super) fn file_has_cursor(
        &self,
        workspace: &Workspace,
        relative_path: &str,
        query: &str,
        case_sensitive: bool,
        cursor: &ContinuationCursor,
    ) -> bool {
        let resource = workspace.resource_unchecked(relative_path);
        let Ok(contained) = open_contained_file(workspace, &resource) else {
            return false;
        };
        if contained.byte_len > self.limits.max_file_bytes {
            return false;
        }
        let mut bytes = Vec::with_capacity(contained.byte_len as usize);
        if contained
            .file
            .take(contained.byte_len)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return false;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            return false;
        };
        content
            .split('\n')
            .enumerate()
            .any(|(line_index, raw_line)| {
                let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
                literal_positions(line, query, case_sensitive).any(|column| {
                    cursor.line_number == one_based(line_index) && cursor.column_number == column
                })
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_file_after_cursor(
        &self,
        workspace: &Workspace,
        relative_path: String,
        query: &str,
        case_sensitive: bool,
        cursor: &ContinuationCursor,
        matches: &mut Vec<WorkspaceSearchMatch>,
        budget: &mut SearchBudget,
        counters: &mut RootCounters,
    ) {
        let resource = workspace.resource_unchecked(&relative_path);
        let Ok(contained) = open_contained_file(workspace, &resource) else {
            counters.error_count += 1;
            counters.partial = true;
            return;
        };
        if contained.byte_len > self.limits.max_file_bytes {
            counters.skipped_files += 1;
            counters.partial = true;
            push_reason(&mut budget.reasons, SearchLimitReason::FileSize);
            return;
        }
        if budget.bytes_scanned.saturating_add(contained.byte_len) > self.limits.max_total_bytes {
            counters.partial = true;
            push_reason(&mut budget.reasons, SearchLimitReason::Bytes);
            return;
        }
        let mut bytes = Vec::with_capacity(contained.byte_len as usize);
        if contained
            .file
            .take(contained.byte_len)
            .read_to_end(&mut bytes)
            .is_err()
        {
            counters.error_count += 1;
            counters.partial = true;
            return;
        }
        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            counters.skipped_files += 1;
            counters.partial = true;
            return;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            counters.skipped_files += 1;
            counters.partial = true;
            return;
        };
        budget.files_seen += 1;
        budget.bytes_scanned += bytes.len() as u64;
        counters.files_scanned += 1;
        counters.bytes_scanned += bytes.len() as u64;
        for (line_index, raw_line) in content.split('\n').enumerate() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            for column in literal_positions(line, query, case_sensitive) {
                let line_number = one_based(line_index);
                if line_number < cursor.line_number
                    || (line_number == cursor.line_number && column <= cursor.column_number)
                {
                    continue;
                }
                matches.push(WorkspaceSearchMatch {
                    resource: resource.clone(),
                    line_number,
                    column_number: column,
                    preview: bounded_preview(line, self.limits.max_preview_chars),
                });
                if matches.len() == self.limits.max_results {
                    push_reason(&mut budget.reasons, SearchLimitReason::Results);
                    counters.partial = true;
                    return;
                }
            }
        }
    }
}

pub(super) fn stale_report(root_id: &str, root_name: String) -> SearchRootReport {
    SearchRootReport {
        root_id: root_id.into(),
        root_name,
        state: SearchRootState::Partial,
        files_scanned: 0,
        bytes_scanned: 0,
        entries_visited: 0,
        skipped_files: 0,
        error_count: 0,
        message: None,
    }
}

fn literal_positions<'a>(
    line: &'a str,
    query: &'a str,
    case_sensitive: bool,
) -> impl Iterator<Item = u32> + 'a {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let (start, end) = find_literal(&line[offset..], query, case_sensitive)?;
        let absolute = offset + start;
        offset += end.max(start + 1);
        Some(column_number(line, absolute))
    })
}

fn find_literal(line: &str, query: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if case_sensitive {
        return line.find(query).map(|start| (start, start + query.len()));
    }
    let needle = query.to_lowercase();
    let mut folded = String::with_capacity(line.len());
    let mut origin = Vec::with_capacity(line.len());
    for (start, character) in line.char_indices() {
        let end = start + character.len_utf8();
        for folded_character in character.to_lowercase() {
            let mut encoded = [0; 4];
            let encoded = folded_character.encode_utf8(&mut encoded);
            folded.push_str(encoded);
            origin.extend(std::iter::repeat_n((start, end), encoded.len()));
        }
    }
    let folded_start = folded.find(&needle)?;
    let folded_end = folded_start + needle.len();
    Some((
        origin.get(folded_start)?.0,
        origin.get(folded_end.checked_sub(1)?)?.1,
    ))
}

fn one_based(zero_based: usize) -> u32 {
    u32::try_from(zero_based.saturating_add(1)).unwrap_or(u32::MAX)
}

fn column_number(line: &str, byte_offset: usize) -> u32 {
    u32::try_from(line[..byte_offset].chars().count().saturating_add(1)).unwrap_or(u32::MAX)
}

fn bounded_preview(line: &str, max_chars: usize) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.into();
    }
    let mut preview = trimmed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    preview.push('…');
    preview
}
