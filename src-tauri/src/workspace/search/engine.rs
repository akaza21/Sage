use std::{
    fs::{DirEntry, FileType},
    io,
    path::PathBuf,
    time::Instant,
};

use crate::{
    error::{AppError, AppResult},
    workspace::{file_walk::walk_over_cap, path_glob::PathGlobs},
};

use super::{
    engine_continue::ContinuationSeek,
    file_scan::ScanQuery,
    literal_match::LiteralQuery,
    types::{
        ContinuationCursor, SearchLimitReason, SearchLimits, SearchRoot, SearchRootSource,
        SearchRootState, WorkspaceSearchRequest, WorkspaceSearchResponse,
        encode_continuation_token, push_reason, stale_continuation, unavailable_report,
        validate_continuation_stamp, validate_search_request,
    },
};

pub struct SearchEngine {
    pub(super) limits: SearchLimits,
    /// Git's global excludes file, which the helper cannot resolve itself.
    pub(super) global_excludes: Option<PathBuf>,
    /// Threads a root's search may use, the walk's included.
    pub(super) threads: usize,
    /// Tests make time run out after this many checks, so a time limit
    /// lands at the same place on every run.
    #[cfg(test)]
    pub(super) time_checks_left: Option<std::sync::atomic::AtomicU64>,
    /// Tests read time from this clock instead, so a time limit lands at the
    /// same place on every run.
    #[cfg(test)]
    pub(super) test_clock: Option<TestClock>,
}

/// Time that moves only when an entry's file type is read, by the same step
/// each time: a filesystem whose listings leave file types out, where each
/// costs a metadata read.
#[cfg(test)]
pub(super) struct TestClock {
    started_at: Instant,
    elapsed_nanos: std::sync::atomic::AtomicU64,
    per_file_type: std::time::Duration,
}

#[cfg(test)]
impl TestClock {
    pub(super) fn new(per_file_type: std::time::Duration) -> Self {
        Self {
            started_at: Instant::now(),
            elapsed_nanos: std::sync::atomic::AtomicU64::new(0),
            per_file_type,
        }
    }

    fn now(&self) -> Instant {
        use std::sync::atomic::Ordering;
        self.started_at
            + std::time::Duration::from_nanos(self.elapsed_nanos.load(Ordering::Relaxed))
    }

    fn read_file_type(&self) {
        use std::sync::atomic::Ordering;
        let step = u64::try_from(self.per_file_type.as_nanos()).unwrap_or(u64::MAX);
        self.elapsed_nanos.fetch_add(step, Ordering::Relaxed);
    }
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
        Self {
            limits,
            global_excludes: None,
            threads: super::file_prefetch::search_threads(),
            #[cfg(test)]
            time_checks_left: None,
            #[cfg(test)]
            test_clock: None,
        }
    }

    pub(super) fn with_global_excludes(mut self, path: Option<PathBuf>) -> Self {
        self.global_excludes = path;
        self
    }

    pub fn search(
        &self,
        request: WorkspaceSearchRequest,
        roots: Vec<SearchRoot>,
    ) -> AppResult<WorkspaceSearchResponse> {
        validate_search_request(&request)?;
        let query = request.query.trim();
        let literal = LiteralQuery::new(query, request.case_sensitive);
        let mut seek = request
            .continuation_token
            .as_deref()
            .map(|token| validate_continuation_stamp(token, &request, query))
            .transpose()?
            .map(|cursor| ContinuationSeek {
                cursor,
                started_at: self.now(),
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
            started_at: self.now(),
            files_seen: 0,
            bytes_scanned: 0,
            entries_visited: 0,
            reasons: Vec::new(),
        };

        let excluded = PathGlobs::new(&request.files_exclude());
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
                    ScanQuery {
                        literal: &literal,
                        whole_word: request.whole_word,
                    },
                    &request.include_patterns,
                    &request.exclude_patterns,
                    request.use_ignore_files,
                    &excluded,
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
        if self.out_of_time(budget.started_at) {
            push_reason(&mut budget.reasons, SearchLimitReason::Time);
            return true;
        }
        false
    }

    /// The entry's file type, which costs a metadata read where the listing
    /// left it out.
    pub(super) fn entry_file_type(&self, entry: &DirEntry) -> io::Result<FileType> {
        #[cfg(test)]
        if let Some(clock) = &self.test_clock {
            clock.read_file_type();
        }
        entry.file_type()
    }

    pub(super) fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(clock) = &self.test_clock {
            return clock.now();
        }
        Instant::now()
    }

    pub(super) fn time_since(&self, started_at: Instant) -> std::time::Duration {
        self.now().saturating_duration_since(started_at)
    }

    fn out_of_time(&self, started_at: Instant) -> bool {
        #[cfg(test)]
        if let Some(left) = &self.time_checks_left {
            use std::sync::atomic::Ordering;
            return left
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    left.checked_sub(1)
                })
                .is_err();
        }
        self.time_since(started_at) >= self.limits.max_duration
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}
