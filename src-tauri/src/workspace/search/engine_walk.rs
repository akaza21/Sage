use std::{collections::VecDeque, fs};

use crate::workspace::{
    Workspace,
    file_walk::{
        MAX_WORKSPACE_QUEUED_DIRECTORIES, WalkBufferAdmit, admit_queued_directory,
        admit_walk_dirent, walk_buffer_remaining,
    },
};

use super::{
    engine::{RootCounters, SearchBudget, SearchEngine},
    engine_continue::{ContinuationSeek, stale_report},
    path_filter::path_is_admitted,
    types::{
        MAX_RESOURCE_PATH_BYTES, SearchLimitReason, SearchRootReport, SearchRootState,
        WorkspaceSearchMatch, contract_join, push_reason,
    },
};

impl SearchEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_root(
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
                    budget.started_at = std::time::Instant::now();
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
                },
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
}
