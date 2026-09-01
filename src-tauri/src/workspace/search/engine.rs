use std::{collections::VecDeque, fs, time::Instant};

use crate::{
    error::{AppError, AppResult},
    workspace::Workspace,
};

use super::engine_continue::{ContinuationSeek, stale_report};
use super::path_filter::path_is_admitted;
use super::types::{
    ContinuationCursor, MAX_RESOURCE_PATH_BYTES, SearchLimitReason, SearchLimits, SearchRoot,
    SearchRootReport, SearchRootSource, SearchRootState, WorkspaceSearchMatch,
    WorkspaceSearchRequest, WorkspaceSearchResponse, contract_join, encode_continuation_token,
    push_reason, stale_continuation, unavailable_report, validate_continuation_stamp,
    validate_search_request,
};
use crate::workspace::file_walk::{
    MAX_WORKSPACE_QUEUED_DIRECTORIES, WalkBufferAdmit, admit_queued_directory, admit_walk_dirent,
    walk_buffer_remaining, walk_over_cap,
};

pub struct SearchEngine {
    pub(super) limits: SearchLimits,
}

pub(super) struct SearchBudget {
    pub(super) started_at: Instant,
    pub(super) files_seen: u64,
    pub(super) bytes_scanned: u64,
    pub(super) entries_visited: u64,
    pub(super) reasons: Vec<SearchLimitReason>,
}

pub(super) struct RootCounters {
    pub(super) files_scanned: u64,
    pub(super) bytes_scanned: u64,
    pub(super) entries_visited: u64,
    pub(super) skipped_files: u64,
    pub(super) error_count: u64,
    pub(super) partial: bool,
}

impl SearchEngine {
    pub fn new(limits: SearchLimits) -> Self {
        Self { limits }
    }

    pub fn search(
        &self,
        request: WorkspaceSearchRequest,
        roots: Vec<SearchRoot>,
    ) -> AppResult<WorkspaceSearchResponse> {
        validate_search_request(&request)?;
        let query = request.query.trim();
        let mut seek = request
            .continuation_token
            .as_deref()
            .map(|token| validate_continuation_stamp(token, &request, query))
            .transpose()?
            .map(|cursor| ContinuationSeek {
                cursor,
                started_at: Instant::now(),
                entries_visited: 0,
            });
        if self.limits.max_results == 0
            || self.limits.max_files == 0
            || self.limits.max_total_bytes == 0
            || self.limits.max_file_bytes == 0
            || self.limits.max_entries == 0
            || self.limits.max_preview_chars == 0
        {
            return Err(AppError::InvalidRequest(
                "workspace search limits must be positive".into(),
            ));
        }

        let mut matches = Vec::new();
        let mut reports = Vec::with_capacity(roots.len());
        let mut budget = SearchBudget {
            started_at: Instant::now(),
            files_seen: 0,
            bytes_scanned: 0,
            entries_visited: 0,
            reasons: Vec::new(),
        };

        for root in roots {
            let SearchRoot {
                root_id,
                root_name,
                source,
            } = root;
            let report = match source {
                SearchRootSource::Unavailable(message) => {
                    unavailable_report(root_id, root_name, message)
                }
                SearchRootSource::Available(workspace) => self.search_root(
                    &root_id,
                    root_name,
                    &workspace,
                    query,
                    request.case_sensitive,
                    request.whole_word,
                    &request.include_patterns,
                    &request.exclude_patterns,
                    &mut matches,
                    &mut budget,
                    &mut seek,
                ),
            };
            reports.push(report);
        }

        if seek.is_some() {
            return Err(stale_continuation());
        }

        let truncated = !budget.reasons.is_empty()
            || reports
                .iter()
                .any(|report| report.state == SearchRootState::Partial);
        let continuation_token = if truncated {
            matches.last().and_then(|last| {
                encode_continuation_token(
                    &request,
                    query,
                    &ContinuationCursor {
                        root_id: last.resource.root_id.clone(),
                        relative_path: last.resource.relative_path.clone(),
                        line_number: last.line_number,
                        column_number: last.column_number,
                    },
                )
            })
        } else {
            None
        };

        Ok(WorkspaceSearchResponse {
            request_id: request.request_id,
            project_id: request.project_id,
            transition_epoch: request.expected_transition_epoch,
            index_generation: request.index_generation,
            query: query.into(),
            truncated,
            continuation_token,
            limit_reasons: budget.reasons,
            matches,
            roots: reports,
            elapsed_ms: elapsed_millis(budget.started_at),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn search_root(
        &self,
        root_id: &str,
        root_name: String,
        workspace: &Workspace,
        query: &str,
        case_sensitive: bool,
        whole_word: bool,
        include_patterns: &[String],
        exclude_patterns: &[String],
        matches: &mut Vec<WorkspaceSearchMatch>,
        budget: &mut SearchBudget,
        seek: &mut Option<ContinuationSeek>,
    ) -> SearchRootReport {
        if workspace.root_id() != root_id {
            return SearchRootReport {
                root_id: root_id.into(),
                root_name,
                state: SearchRootState::Unavailable,
                files_scanned: 0,
                bytes_scanned: 0,
                entries_visited: 0,
                skipped_files: 0,
                error_count: 1,
                message: Some("search root identity does not match its Workspace".into()),
            };
        }

        let mut counters = RootCounters {
            files_scanned: 0,
            bytes_scanned: 0,
            entries_visited: 0,
            skipped_files: 0,
            error_count: 0,
            partial: false,
        };
        let mut directories = VecDeque::from([(workspace.root().to_path_buf(), String::new(), 0)]);

        while let Some((directory, relative, depth)) = directories.pop_front() {
            if self.limit_reached_for_phase(budget, matches.len(), seek.as_ref()) {
                counters.partial = true;
                break;
            }
            let read_directory = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => {
                    counters.error_count += 1;
                    counters.partial = true;
                    continue;
                }
            };
            let mut entries = Vec::new();
            for entry in read_directory {
                if self.limit_reached_for_phase(budget, matches.len(), seek.as_ref()) {
                    counters.partial = true;
                    break;
                }
                if let Some(active_seek) = seek.as_mut() {
                    if active_seek.entries_visited == self.limits.max_entries {
                        return stale_report(root_id, root_name);
                    }
                    active_seek.entries_visited += 1;
                } else {
                    if budget.entries_visited == self.limits.max_entries {
                        push_reason(&mut budget.reasons, SearchLimitReason::Entries);
                        counters.partial = true;
                        break;
                    }
                    budget.entries_visited += 1;
                    counters.entries_visited += 1;
                }
                match entry {
                    Ok(entry) => match admit_walk_dirent(
                        &entry.file_name(),
                        entries.len(),
                        walk_buffer_remaining(budget.entries_visited, self.limits.max_entries),
                    ) {
                        WalkBufferAdmit::Skip => {}
                        WalkBufferAdmit::Keep => entries.push(entry),
                        WalkBufferAdmit::Cap => {
                            push_reason(&mut budget.reasons, SearchLimitReason::Entries);
                            counters.partial = true;
                            break;
                        }
                    },
                    Err(_) => {
                        counters.error_count += 1;
                        counters.partial = true;
                    }
                }
            }
            entries.sort_unstable_by_key(|entry| entry.file_name());

            let mut charge_remaining_to_page = false;
            for entry in entries {
                if charge_remaining_to_page {
                    if budget.entries_visited == self.limits.max_entries {
                        push_reason(&mut budget.reasons, SearchLimitReason::Entries);
                        counters.partial = true;
                        break;
                    }
                    budget.entries_visited += 1;
                    counters.entries_visited += 1;
                }
                if self.limit_reached_for_phase(budget, matches.len(), seek.as_ref()) {
                    counters.partial = true;
                    break;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    counters.error_count += 1;
                    counters.partial = true;
                    continue;
                };
                if name.contains('\\') || name.contains('/') {
                    counters.error_count += 1;
                    counters.partial = true;
                    continue;
                }
                let child_relative = contract_join(&relative, &name);
                if child_relative.len() > MAX_RESOURCE_PATH_BYTES {
                    counters.error_count += 1;
                    counters.partial = true;
                    continue;
                }
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => {
                        counters.error_count += 1;
                        counters.partial = true;
                        continue;
                    }
                };
                if file_type.is_symlink() {
                    counters.skipped_files += 1;
                    continue;
                }
                if file_type.is_dir() {
                    if depth == self.limits.max_depth {
                        push_reason(&mut budget.reasons, SearchLimitReason::Depth);
                        counters.partial = true;
                    } else if !admit_queued_directory(
                        directories.len(),
                        MAX_WORKSPACE_QUEUED_DIRECTORIES,
                    ) {
                        push_reason(&mut budget.reasons, SearchLimitReason::Entries);
                        counters.partial = true;
                    } else {
                        directories.push_back((entry.path(), child_relative, depth + 1));
                    }
                    continue;
                }
                if !file_type.is_file() {
                    counters.skipped_files += 1;
                    continue;
                }
                if !path_is_admitted(&child_relative, include_patterns, exclude_patterns) {
                    counters.skipped_files += 1;
                    continue;
                }
                if seek.is_none() && budget.files_seen == self.limits.max_files {
                    push_reason(&mut budget.reasons, SearchLimitReason::FileCount);
                    counters.partial = true;
                    break;
                }
                if let Some(active_seek) = seek.as_ref() {
                    if active_seek.cursor.root_id != root_id
                        || active_seek.cursor.relative_path != child_relative
                    {
                        continue;
                    }
                    if !self.file_has_cursor(
                        workspace,
                        &child_relative,
                        query,
                        case_sensitive,
                        whole_word,
                        &active_seek.cursor,
                    ) {
                        continue;
                    }
                    let cursor = active_seek.cursor.clone();
                    *seek = None;
                    charge_remaining_to_page = true;
                    budget.started_at = Instant::now();
                    self.search_file_after_cursor(
                        workspace,
                        child_relative,
                        query,
                        case_sensitive,
                        whole_word,
                        &cursor,
                        matches,
                        budget,
                        &mut counters,
                    );
                } else {
                    budget.files_seen += 1;
                    self.search_file(
                        workspace,
                        child_relative,
                        query,
                        case_sensitive,
                        whole_word,
                        matches,
                        budget,
                        &mut counters,
                    );
                }
            }
        }

        let state = if counters.partial || counters.error_count > 0 {
            SearchRootState::Partial
        } else {
            SearchRootState::Complete
        };
        let message = if counters.error_count > 0 {
            Some(format!(
                "{} filesystem entr{} could not be searched",
                counters.error_count,
                if counters.error_count == 1 {
                    "y"
                } else {
                    "ies"
                }
            ))
        } else {
            None
        };
        SearchRootReport {
            root_id: root_id.into(),
            root_name,
            state,
            files_scanned: counters.files_scanned,
            bytes_scanned: counters.bytes_scanned,
            entries_visited: counters.entries_visited,
            skipped_files: counters.skipped_files,
            error_count: counters.error_count,
            message,
        }
    }

    pub(super) fn limit_reached(&self, budget: &mut SearchBudget, result_count: usize) -> bool {
        if result_count >= self.limits.max_results {
            push_reason(&mut budget.reasons, SearchLimitReason::Results);
            return true;
        }
        if walk_over_cap(budget.files_seen, self.limits.max_files) {
            push_reason(&mut budget.reasons, SearchLimitReason::FileCount);
            return true;
        }
        if walk_over_cap(budget.entries_visited, self.limits.max_entries) {
            push_reason(&mut budget.reasons, SearchLimitReason::Entries);
            return true;
        }
        if budget.started_at.elapsed() >= self.limits.max_duration {
            push_reason(&mut budget.reasons, SearchLimitReason::Time);
            return true;
        }
        false
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}
