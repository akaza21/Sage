use std::{
    collections::VecDeque,
    fs::{self, DirEntry},
    ops::Deref,
};

use crate::workspace::{
    Workspace,
    file_walk::{
        MAX_WORKSPACE_QUEUED_DIRECTORIES, WalkBufferAdmit, admit_queued_directory,
        admit_walk_dirent, walk_buffer_remaining,
    },
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
    path_glob::PathGlobs,
};

use super::{
    engine::{RootCounters, SearchBudget, SearchEngine},
    engine_continue::{ContinuationSeek, stale_report},
    file_prefetch::{FolderScans, Prefetch, QueuedFolder, ReadBudget, WalkContext},
    file_scan::ScanQuery,
    path_filter::path_is_admitted,
    types::{
        MAX_RESOURCE_PATH_BYTES, SearchLimitReason, SearchRootReport, SearchRootState,
        WorkspaceSearchMatch, contract_join, push_reason,
    },
};

/// An entry of the folder being walked: listed by the walk itself, or
/// borrowed from a listing read ahead.
enum FolderEntry<'a> {
    Listed(DirEntry),
    ReadAhead(&'a DirEntry),
}

impl Deref for FolderEntry<'_> {
    type Target = DirEntry;

    fn deref(&self) -> &DirEntry {
        match self {
            Self::Listed(entry) => entry,
            Self::ReadAhead(entry) => entry,
        }
    }
}

impl SearchEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_root(
        &self,
        root_id: &str,
        root_name: String,
        workspace: &Workspace,
        query: ScanQuery<'_>,
        include_patterns: &[String],
        exclude_patterns: &[String],
        use_ignore_files: bool,
        excluded: &PathGlobs,
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
        let rules = IgnoreRules::load(
            workspace.root(),
            use_ignore_files,
            self.global_excludes.as_deref(),
        );
        let context = WalkContext {
            engine: self,
            workspace,
            rules: &rules,
            excluded,
            include_patterns,
            exclude_patterns,
            query,
        };
        let stopped = std::thread::scope(|scope| {
            let mut prefetch = Prefetch::start(scope, context);
            let mut directories = VecDeque::from([QueuedFolder {
                path: workspace.root().to_path_buf(),
                relative: String::new(),
                depth: 0,
                parent_rules: FolderChain::default(),
            }]);
            // Folders are numbered in the order they were queued.
            let mut front = 0_u64;

            loop {
                if let Some(prefetch) = prefetch.as_mut() {
                    let reads = seek.is_none().then(|| ReadBudget {
                        deadline: budget.started_at.checked_add(self.limits.max_duration),
                        bytes_left: self
                            .limits
                            .max_total_bytes
                            .saturating_sub(budget.bytes_scanned),
                    });
                    prefetch.submit_ahead(&directories, front, reads);
                }
                let Some(QueuedFolder {
                    path: directory,
                    relative,
                    depth,
                    parent_rules,
                }) = directories.pop_front()
                else {
                    break;
                };
                if self.limit_reached_for_phase(budget, matches.len(), seek.as_ref()) {
                    counters.partial = true;
                    break;
                }
                let listing = prefetch.as_mut().and_then(|prefetch| prefetch.take(front));
                front += 1;
                let (listed_ahead, mut ahead) = match listing {
                    Some(listing) => (
                        Some(listing.entries),
                        Some((listing.folder_rules, listing.scans)),
                    ),
                    None => (None, None),
                };
                // Only whether an entry failed to list matters, not why.
                let read_directory: Box<dyn Iterator<Item = Result<FolderEntry<'_>, ()>>> =
                    match &listed_ahead {
                        Some(entries) => {
                            Box::new(entries.iter().map(|entry| {
                                entry.as_ref().map(FolderEntry::ReadAhead).map_err(drop)
                            }))
                        }
                        None => match fs::read_dir(&directory) {
                            Ok(entries) => Box::new(
                                entries.map(|entry| entry.map(FolderEntry::Listed).map_err(drop)),
                            ),
                            Err(_) => {
                                counters.error_count += 1;
                                counters.partial = true;
                                continue;
                            }
                        },
                    };
                let mut entries = Vec::new();
                let mut rule_files = RuleFiles::default();
                let mut cut_short = false;
                for entry in read_directory {
                    if self.limit_reached_for_phase(budget, matches.len(), seek.as_ref()) {
                        counters.partial = true;
                        cut_short = true;
                        break;
                    }
                    if let Some(active_seek) = seek.as_mut() {
                        if active_seek.entries_visited == self.limits.max_entries {
                            return Some(stale_report(root_id, root_name.clone()));
                        }
                        active_seek.entries_visited += 1;
                    } else {
                        if budget.entries_visited == self.limits.max_entries {
                            push_reason(&mut budget.reasons, SearchLimitReason::Entries);
                            counters.partial = true;
                            cut_short = true;
                            break;
                        }
                        budget.entries_visited += 1;
                        counters.entries_visited += 1;
                    }
                    if let Ok(entry) = &entry {
                        rule_files.note(&entry.file_name());
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
                                cut_short = true;
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
                // A listing cut short may have missed a rule file, so look for
                // them by name; the same rules apply when a continuation replays.
                // What was read ahead assumed the whole listing.
                if cut_short {
                    rule_files = RuleFiles::probe(&directory);
                    ahead = None;
                }
                let (folder_rules, mut scans) = match ahead {
                    Some((folder_rules, scans)) => (folder_rules, scans),
                    None => (
                        rules.enter(&parent_rules, &relative, rule_files),
                        FolderScans::none(),
                    ),
                };

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
                    let file_type = match self.entry_file_type(&entry) {
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
                    // Excluded and ignored folders are not entered, so their
                    // contents stay out as in VS Code and Git.
                    if excluded.matches(&child_relative)
                        || rules.is_ignored(&folder_rules, &child_relative, file_type.is_dir())
                    {
                        if !file_type.is_dir() {
                            counters.skipped_files += 1;
                        }
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
                            directories.push_back(QueuedFolder {
                                path: entry.path(),
                                relative: child_relative,
                                depth: depth + 1,
                                parent_rules: folder_rules.clone(),
                            });
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
                            &active_seek.cursor,
                        ) {
                            continue;
                        }
                        let cursor = active_seek.cursor.clone();
                        *seek = None;
                        charge_remaining_to_page = true;
                        budget.started_at = self.now();
                        self.search_file_after_cursor(
                            workspace,
                            child_relative,
                            query,
                            &cursor,
                            matches,
                            budget,
                            &mut counters,
                        );
                    } else {
                        budget.files_seen += 1;
                        let scan = scans.take(&name).unwrap_or_else(|| {
                            let bytes_left = self
                                .limits
                                .max_total_bytes
                                .saturating_sub(budget.bytes_scanned);
                            self.scan_file(workspace, &child_relative, query, bytes_left)
                        });
                        self.apply_scan(scan, matches, budget, &mut counters);
                    }
                }
            }
            None
        });
        if let Some(report) = stopped {
            return report;
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
