use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    fs::{self, DirEntry},
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

/// A folder in the walk's queue.
pub(super) struct QueuedFolder {
    pub(super) path: PathBuf,
    pub(super) relative: String,
    pub(super) depth: usize,
    /// The rules of the folder holding it.
    pub(super) parent_rules: FolderChain,
}

/// One folder, listed and read ahead.
pub(super) struct Listing {
    /// Every entry, in the order the platform listed them.
    pub(super) entries: Vec<io::Result<DirEntry>>,
    /// The rules in force inside the folder, from the complete listing.
    pub(super) folder_rules: FolderChain,
    /// The folder's Files that were read, by root-relative path. Any other
    /// File the walk reads itself.
    pub(super) scans: HashMap<String, FileScan>,
}

struct Job {
    id: u64,
    directory: PathBuf,
    relative: String,
    parent_rules: FolderChain,
    /// What is left of the page when the job was handed out. Both only
    /// shrink, so reads past them are never needed.
    reads: Option<ReadBudget>,
}

/// What the page has left, for the reads ahead; None while a continuation
/// replays, which reads no Files.
#[derive(Clone, Copy)]
pub(super) struct ReadBudget {
    /// None when the time limit is too far off to represent.
    pub(super) deadline: Option<Instant>,
    pub(super) bytes_left: u64,
}

/// What the workers share with the walk, borrowed for the walk's length.
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

/// Lists and reads the next few folders of the walk's queue on worker
/// threads. The walk still goes through each listing in its own order and
/// charges every entry and File to the budget then, so results, counters,
/// limits and continuations are the same as without it; only the waiting
/// for the disk overlaps.
pub(super) struct Prefetch {
    jobs: Option<mpsc::Sender<Job>>,
    results: mpsc::Receiver<(u64, Option<Listing>)>,
    ready: HashMap<u64, Option<Listing>>,
    submitted: u64,
    ahead: u64,
    stop: Arc<AtomicBool>,
}

/// Search's threads in all, the walk's included: at most four, and never
/// more than the machine has cores.
pub(super) fn search_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(4)
}

impl Prefetch {
    /// None on a single core, where the walk does everything itself.
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
                    // A panic leaves the folder to the walk, which then fails
                    // the same way, instead of waiting forever for it.
                    let listing =
                        catch_unwind(AssertUnwindSafe(|| list_folder(context, job, &stop)))
                            .unwrap_or(None);
                    if sender.send((id, listing)).is_err() {
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

    /// Hands out the queued folders not yet handed out, up to a few ahead
    /// of `front`, the number of the folder at the head of `queue`. Folders
    /// are numbered in the order they were queued.
    pub(super) fn submit_ahead(
        &mut self,
        queue: &VecDeque<QueuedFolder>,
        front: u64,
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
                reads,
            };
            if jobs.send(job).is_err() {
                break;
            }
            self.submitted += 1;
        }
    }

    /// Folder `id`'s listing, waiting for it if a worker still has it; None
    /// if it was never handed out or the worker left it to the walk.
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

fn list_folder(context: WalkContext<'_>, job: Job, stop: &AtomicBool) -> Option<Listing> {
    let mut entries = Vec::new();
    let mut rule_files = RuleFiles::default();
    for entry in fs::read_dir(&job.directory).ok()? {
        if entries.len() == MAX_LISTED_ENTRIES || stop.load(Ordering::Relaxed) {
            return None;
        }
        if let Ok(entry) = &entry {
            rule_files.note(&entry.file_name());
        }
        entries.push(entry);
    }
    let folder_rules = context
        .rules
        .enter(&job.parent_rules, &job.relative, rule_files);
    let mut scans = HashMap::new();
    if let Some(reads) = job.reads {
        let mut files = entries
            .iter()
            .filter_map(|entry| entry.as_ref().ok())
            .filter_map(|entry| {
                let relative = searchable_file(entry, &job.relative, &context, &folder_rules)?;
                Some((entry.file_name(), relative))
            })
            .collect::<Vec<(OsString, String)>>();
        // In walk order, so the reads stop where a page would.
        files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let max_results = context.engine.limits.max_results;
        let (mut found, mut bytes) = (0_usize, 0_u64);
        for (_, relative) in files {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            if found >= max_results
                || bytes > reads.bytes_left
                || reads
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                break;
            }
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
            scans.insert(relative, scan);
        }
    }
    Some(Listing {
        entries,
        folder_rules,
        scans,
    })
}

/// The root-relative path of an entry the walk will read as a File, by the
/// walk's own tests; None for anything else. Getting this wrong costs only
/// speed: the walk reads any File that was not read ahead itself.
fn searchable_file(
    entry: &DirEntry,
    folder: &str,
    context: &WalkContext<'_>,
    folder_rules: &FolderChain,
) -> Option<String> {
    let name = entry.file_name();
    let name = name.to_str()?;
    if name.contains('\\') || name.contains('/') {
        return None;
    }
    let relative = contract_join(folder, name);
    let file_type = entry.file_type().ok()?;
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
