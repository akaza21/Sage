use std::{
    cell::Cell,
    cmp::Ordering as NameOrder,
    collections::{HashMap, VecDeque},
    fs::DirEntry,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::Scope,
    time::Instant,
};

use crate::workspace::{
    Workspace,
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
    path_glob::PathGlobs,
};

use super::{
    engine::SearchEngine,
    file_scan::{FileScan, ScanQuery},
    path_filter::path_is_admitted,
    types::{MAX_RESOURCE_PATH_BYTES, contract_join},
};

/// Folders handed out ahead of the walk for each worker. Each worker takes a
/// whole folder, so handing work over costs once per folder, not once per
/// File; a few folders each keep the workers busy while the walk catches up.
/// Measured on 200k small Files: 8 folders in all ran 1.9x, 16 ran 2.2x.
const FOLDERS_AHEAD_PER_WORKER: u64 = 5;
/// A larger folder is left to the walk, which bounds what one listing holds.
const MAX_LISTED_ENTRIES: usize = 4_096;

pub(super) struct QueuedFolder {
    pub(super) path: PathBuf,
    pub(super) relative: String,
    pub(super) depth: usize,
    pub(super) parent_rules: FolderChain,
}

pub(super) struct Listing {
    pub(super) entries: Arc<[io::Result<DirEntry>]>,
    pub(super) folder_rules: FolderChain,
    pub(super) scans: FolderScans,
}

/// Scans of one folder's Files, arriving in the walk's order (by name).
pub(super) struct FolderScans {
    arriving: Option<mpsc::Receiver<(String, FileScan)>>,
    next: Option<(String, FileScan)>,
}

impl FolderScans {
    pub(super) fn none() -> Self {
        Self {
            arriving: None,
            next: None,
        }
    }

    /// The walk asks in name order, the order the worker reads in.
    pub(super) fn take(&mut self, name: &str) -> Option<FileScan> {
        loop {
            let (scanned, scan) = match self.next.take() {
                Some(next) => next,
                None => self.arriving.as_ref()?.recv().ok()?,
            };
            match scanned.as_str().cmp(name) {
                NameOrder::Less => continue,
                NameOrder::Equal => return Some(scan),
                NameOrder::Greater => {
                    self.next = Some((scanned, scan));
                    return None;
                }
            }
        }
    }
}

struct Job {
    id: u64,
    directory: PathBuf,
    relative: String,
    parent_rules: FolderChain,
    phase_deadline: Option<Instant>,
    reads: Option<ReadBudget>,
}

#[derive(Clone, Copy)]
pub(super) struct ReadBudget {
    pub(super) bytes_left: u64,
}

#[derive(Clone, Copy)]
pub(super) struct WalkContext<'a> {
    pub(super) engine: &'a SearchEngine,
    pub(super) workspace: &'a Workspace,
    pub(super) rules: &'a IgnoreRules,
    pub(super) excluded: &'a PathGlobs,
    pub(super) include_patterns: &'a [String],
    pub(super) exclude_patterns: &'a [String],
    pub(super) query: ScanQuery<'a>,
}

/// The walk still charges every entry and File to the budget in its own
/// order, so results, counters, limits and continuations are the same as
/// without read-ahead; only the waiting for the disk overlaps.
pub(super) struct Prefetch {
    jobs: Option<mpsc::Sender<Job>>,
    results: mpsc::Receiver<(u64, Option<Listing>)>,
    ready: HashMap<u64, Option<Listing>>,
    submitted: u64,
    ahead: u64,
    stop: Arc<AtomicBool>,
}

pub(super) fn search_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(4)
}

impl Prefetch {
    pub(super) fn start<'scope>(
        scope: &'scope Scope<'scope, '_>,
        context: WalkContext<'scope>,
    ) -> Option<Self> {
        let workers = context.engine.threads.saturating_sub(1);
        if workers == 0 {
            return None;
        }
        let (jobs, job_queue) = mpsc::channel::<Job>();
        let (sender, results) = mpsc::channel();
        let job_queue = Arc::new(Mutex::new(job_queue));
        let stop = Arc::new(AtomicBool::new(false));
        for _ in 0..workers {
            let sender = sender.clone();
            let job_queue = Arc::clone(&job_queue);
            let stop = Arc::clone(&stop);
            scope.spawn(move || {
                loop {
                    let job = job_queue
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    let Ok(job) = job else {
                        break;
                    };
                    let id = job.id;
                    let delivered = Cell::new(false);
                    let deliver = |listing| {
                        delivered.set(true);
                        sender.send((id, Some(listing))).is_ok()
                    };
                    // A panic leaves the rest of the folder to the walk, which
                    // then fails the same way, instead of waiting forever.
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        read_folder(context, job, &stop, deliver);
                    }));
                    if !delivered.get() && sender.send((id, None)).is_err() {
                        break;
                    }
                }
            });
        }
        Some(Self {
            jobs: Some(jobs),
            results,
            ready: HashMap::new(),
            submitted: 0,
            ahead: workers as u64 * FOLDERS_AHEAD_PER_WORKER,
            stop,
        })
    }

    pub(super) fn submit_ahead(
        &mut self,
        queue: &VecDeque<QueuedFolder>,
        front: u64,
        phase_deadline: Option<Instant>,
        reads: Option<ReadBudget>,
    ) {
        let Some(jobs) = &self.jobs else {
            return;
        };
        self.submitted = self.submitted.max(front);
        while self.submitted < front + self.ahead {
            let Some(folder) = queue.get((self.submitted - front) as usize) else {
                break;
            };
            let job = Job {
                id: self.submitted,
                directory: folder.path.clone(),
                relative: folder.relative.clone(),
                parent_rules: folder.parent_rules.clone(),
                phase_deadline,
                reads,
            };
            if jobs.send(job).is_err() {
                break;
            }
            self.submitted += 1;
        }
    }

    pub(super) fn take(&mut self, id: u64) -> Option<Listing> {
        if id >= self.submitted {
            return None;
        }
        loop {
            if let Some(listing) = self.ready.remove(&id) {
                return listing;
            }
            let (done, listing) = self.results.recv().ok()?;
            self.ready.insert(done, listing);
        }
    }
}

impl Drop for Prefetch {
    fn drop(&mut self) {
        // Workers drop what they are doing, and closing the queue lets them
        // finish, so the scope can end.
        self.stop.store(true, Ordering::Relaxed);
        self.jobs = None;
    }
}

/// Nothing that may touch the disk per entry runs before delivery, so the
/// walk is never kept waiting past where it would have read a File.
fn read_folder(
    context: WalkContext<'_>,
    job: Job,
    stop: &AtomicBool,
    deliver: impl FnOnce(Listing) -> bool,
) {
    let stopped = || {
        stop.load(Ordering::Relaxed)
            || job
                .phase_deadline
                .is_some_and(|deadline| context.engine.now() >= deadline)
    };
    if stopped() {
        return;
    }
    let Ok(mut listed) = context.engine.read_directory(&job.directory) else {
        return;
    };
    let mut entries = Vec::new();
    let mut rule_files = RuleFiles::default();
    loop {
        if stopped() {
            return;
        }
        let Some(entry) = listed.next() else {
            break;
        };
        if entries.len() == MAX_LISTED_ENTRIES {
            return;
        }
        if let Ok(entry) = &entry {
            rule_files.note(&entry.file_name());
        }
        entries.push(entry);
    }
    if stopped() {
        return;
    }
    let entries = Arc::<[_]>::from(entries);
    let folder_rules = context
        .rules
        .enter(&job.parent_rules, &job.relative, rule_files);
    let Some(reads) = job.reads else {
        deliver(Listing {
            entries,
            folder_rules,
            scans: FolderScans::none(),
        });
        return;
    };
    // In walk order: the walk takes them in turn, and the reads stop where
    // a page would.
    let mut named = entries
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry)))
        .collect::<Vec<_>>();
    named.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let (scanned, arriving) = mpsc::channel();
    let listing = Listing {
        entries: Arc::clone(&entries),
        folder_rules: folder_rules.clone(),
        scans: FolderScans {
            arriving: Some(arriving),
            next: None,
        },
    };
    if !deliver(listing) {
        return;
    }
    let max_results = context.engine.limits.max_results;
    let (mut found, mut bytes) = (0_usize, 0_u64);
    for (name, entry) in named {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if found >= max_results
            || bytes > reads.bytes_left
            || job
                .phase_deadline
                .is_some_and(|deadline| context.engine.now() >= deadline)
        {
            break;
        }
        let Some(relative) = searchable_file(entry, &name, &job.relative, &context, &folder_rules)
        else {
            continue;
        };
        // Only the whole budget left is safe to pass: the walk may not
        // charge every byte counted here (an oversized File is skipped).
        let scan = context.engine.scan_file(
            context.workspace,
            &relative,
            context.query,
            reads.bytes_left,
        );
        if let FileScan::Opened { byte_len, body } = &scan {
            bytes = bytes.saturating_add(*byte_len);
            found += body.match_count();
        }
        if scanned.send((name, scan)).is_err() {
            return;
        }
    }
}

/// Getting this wrong costs only speed: the walk reads any File that was not
/// read ahead itself.
fn searchable_file(
    entry: &DirEntry,
    name: &str,
    folder: &str,
    context: &WalkContext<'_>,
    folder_rules: &FolderChain,
) -> Option<String> {
    if name.contains('\\') || name.contains('/') {
        return None;
    }
    let relative = contract_join(folder, name);
    let file_type = context.engine.entry_file_type(entry).ok()?;
    let admitted = relative.len() <= MAX_RESOURCE_PATH_BYTES
        && file_type.is_file()
        && !context.excluded.matches(&relative)
        && !context.rules.is_ignored(folder_rules, &relative, false)
        && path_is_admitted(
            &relative,
            context.include_patterns,
            context.exclude_patterns,
        );
    admitted.then_some(relative)
}
