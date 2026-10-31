use std::{io::Read, time::Instant};

use crate::workspace::Workspace;

use super::{
    contained_file::open_contained_file,
    engine::{RootCounters, SearchBudget, SearchEngine},
    file_scan::{BINARY_SNIFF_BYTES, LineMatches, ScanQuery, one_based},
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
        query: ScanQuery<'_>,
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
        // Only the cursor's own line can hold it.
        let Some(raw_line) = content
            .split('\n')
            .nth(cursor.line_number.saturating_sub(1) as usize)
        else {
            return false;
        };
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let mut columns = LineMatches::new(line, self.limits.max_preview_chars);
        let mut found = false;
        query
            .literal
            .for_each_match(line, query.whole_word, |(start, _)| {
                let column = columns.column(start);
                found = column == cursor.column_number;
                column < cursor.column_number
            });
        found
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_file_after_cursor(
        &self,
        workspace: &Workspace,
        relative_path: String,
        query: ScanQuery<'_>,
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
        // Binary files are skipped like symlinks: counted, but the search stays
        // complete, as in VS Code. Text that is not UTF-8 was not searched, so it
        // still marks the root partial.
        if bytes
            .iter()
            .take(BINARY_SNIFF_BYTES as usize)
            .any(|byte| *byte == 0)
        {
            counters.skipped_files += 1;
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
        // Lines before the cursor's hold nothing new.
        let skipped = cursor.line_number.saturating_sub(1) as usize;
        for (line_index, raw_line) in content.split('\n').enumerate().skip(skipped) {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line_number = one_based(line_index);
            let mut found = LineMatches::new(line, self.limits.max_preview_chars);
            query
                .literal
                .for_each_match(line, query.whole_word, |(start, _)| {
                    let found_match = found.at(&resource, line_number, start);
                    if line_number > cursor.line_number
                        || found_match.column_number > cursor.column_number
                    {
                        matches.push(found_match);
                    }
                    matches.len() < self.limits.max_results
                });
            if matches.len() == self.limits.max_results {
                push_reason(&mut budget.reasons, SearchLimitReason::Results);
                counters.partial = true;
                return;
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
