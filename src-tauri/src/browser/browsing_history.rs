//! Recording happens on WebView2's UI thread, so it must never touch the
//! disk there. A tab's commit only hands a small message to a bounded
//! queue; one worker thread, started on first use, owns the database and
//! writes queued visits in batched transactions. Reads, removals and
//! clears travel the same queue, so a clear can never be overtaken by a
//! visit recorded before it. When the queue is full a visit is dropped
//! rather than stalling navigation.

use std::{
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

pub(crate) mod commands;
mod store;
mod writer;

pub(crate) use store::{Cursor, MAX_PAGE, MAX_REMOVE, MAX_SEARCH_CHARS, VisitPage};
use writer::{Write, Writer};

pub(crate) const FILE_NAME: &str = "browsing-history.db";
const QUEUE: usize = 256;
const MAX_BATCH: usize = 64;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const UNAVAILABLE: &str = "browsing history is unavailable";

type Reply<T> = SyncSender<Result<T, String>>;

enum Request {
    Write(Write),
    Read(Read),
}

enum Read {
    Page {
        search: String,
        before: Option<Cursor>,
        limit: u32,
        reply: Reply<VisitPage>,
    },
    Remove {
        ids: Vec<i64>,
        reply: Reply<usize>,
    },
    RemoveRange {
        from: i64,
        to: i64,
        reply: Reply<usize>,
    },
}

struct Shared {
    path: PathBuf,
    queue: OnceLock<SyncSender<Request>>,
    dropped: AtomicU64,
    stopped: AtomicBool,
}

#[derive(Clone, Default)]
pub(crate) struct BrowsingHistory(Option<Arc<Shared>>);

impl BrowsingHistory {
    pub(crate) fn open(path: PathBuf) -> Self {
        Self(Some(Arc::new(Shared {
            path,
            queue: OnceLock::new(),
            dropped: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
        })))
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        Self::open(PathBuf::from(":memory:"))
    }

    /// Never blocks; false when the visit was not queued, so the tab's later
    /// titles and same-document changes must not name it.
    pub(super) fn record_visit(&self, tab: &str, url: &str, title: Option<&str>) -> bool {
        self.record(tab, url, title, false)
    }

    pub(super) fn record_same_document(&self, tab: &str, url: &str, title: Option<&str>) -> bool {
        self.record(tab, url, title, true)
    }

    fn record(&self, tab: &str, url: &str, title: Option<&str>, same_document: bool) -> bool {
        self.write(Write::Visit {
            tab: tab.to_owned(),
            url: url.to_owned(),
            title: title.map(str::to_owned),
            visited_at: now_ms(),
            same_document,
        })
    }

    pub(super) fn record_title(&self, tab: &str, title: &str) {
        self.write(Write::Title {
            tab: tab.to_owned(),
            title: title.to_owned(),
        });
    }

    fn write(&self, write: Write) -> bool {
        let Some(shared) = &self.0 else {
            return false;
        };
        match shared.queue().try_send(Request::Write(write)) {
            Ok(()) => return true,
            Err(TrySendError::Full(_)) => {
                let dropped = shared.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if dropped.is_power_of_two() {
                    eprintln!("browsing history queue full; {dropped} writes dropped");
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                if !shared.stopped.swap(true, Ordering::Relaxed) {
                    eprintln!("browsing history worker stopped; visits are not recorded");
                }
            }
        }
        false
    }

    /// Blocks until the worker answers; call it off the UI thread.
    pub(crate) fn page(
        &self,
        search: String,
        before: Option<Cursor>,
        limit: u32,
    ) -> Result<VisitPage, String> {
        self.ask(|reply| Read::Page {
            search,
            before,
            limit,
            reply,
        })
    }

    /// Blocks; call it off the UI thread.
    pub(crate) fn remove(&self, ids: Vec<i64>) -> Result<usize, String> {
        self.ask(|reply| Read::Remove { ids, reply })
    }

    /// Blocks; call it off the UI thread.
    pub(crate) fn remove_range(&self, from: i64, to: i64) -> Result<usize, String> {
        self.ask(|reply| Read::RemoveRange { from, to, reply })
    }

    pub(crate) fn clear(&self) -> Result<usize, String> {
        self.remove_range(i64::MIN, i64::MAX)
    }

    fn ask<T>(&self, request: impl FnOnce(Reply<T>) -> Read) -> Result<T, String> {
        let shared = self.0.as_ref().ok_or(UNAVAILABLE)?;
        let (reply, answer) = sync_channel(1);
        shared
            .queue()
            .send(Request::Read(request(reply)))
            .map_err(|_| UNAVAILABLE.to_owned())?;
        answer
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => "browsing history timed out".to_owned(),
                RecvTimeoutError::Disconnected => UNAVAILABLE.to_owned(),
            })?
    }
}

impl Shared {
    fn queue(&self) -> &SyncSender<Request> {
        self.queue.get_or_init(|| {
            let (sender, receiver) = sync_channel(QUEUE);
            let path = self.path.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("sage-browsing-history".into())
                .spawn(move || run(&path, &receiver))
            {
                // The receiver is gone with the closure: writes are dropped
                // and reads report the store unavailable.
                eprintln!("browsing history worker failed to start: {error}");
            }
            sender
        })
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

fn run(path: &std::path::Path, receiver: &Receiver<Request>) {
    let mut connection: Option<Connection> = None;
    let mut writer = Writer::default();
    let mut writes = Vec::with_capacity(MAX_BATCH);
    loop {
        let received = if connection.is_some() {
            receiver.recv_timeout(IDLE_TIMEOUT)
        } else {
            receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        let first = match received {
            Ok(first) => first,
            Err(RecvTimeoutError::Timeout) => {
                connection = None;
                writes = Vec::new();
                // Keep only the bounded visit attribution: late titles must
                // still name their own committed rows when the database wakes.
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut next = Some(first);
        while let Some(request) = next.take() {
            match request {
                Request::Write(write) => {
                    writes.push(write);
                    if writes.len() < MAX_BATCH {
                        next = receiver.try_recv().ok();
                    }
                }
                Request::Read(read) => {
                    flush(path, &mut connection, &mut writer, &mut writes);
                    answer(path, &mut connection, read);
                }
            }
        }
        flush(path, &mut connection, &mut writer, &mut writes);
    }
}

/// Opens the database on first use, and again after a failed open, so a
/// transient failure does not disable history for the session.
fn connect<'a>(
    path: &std::path::Path,
    connection: &'a mut Option<Connection>,
) -> Result<&'a mut Connection, String> {
    if connection.is_none() {
        let opened = path
            .to_str()
            .ok_or_else(|| "browsing history path is not UTF-8".to_owned())
            .and_then(|path| store::open(path).map_err(|error| error.to_string()));
        match opened {
            Ok(opened) => *connection = Some(opened),
            Err(error) => {
                eprintln!("browsing history could not open: {error}");
                return Err(UNAVAILABLE.into());
            }
        }
    }
    connection.as_mut().ok_or_else(|| UNAVAILABLE.into())
}

fn flush(
    path: &std::path::Path,
    connection: &mut Option<Connection>,
    writer: &mut Writer,
    writes: &mut Vec<Write>,
) {
    if writes.is_empty() {
        return;
    }
    let result = connect(path, connection).and_then(|connection| {
        writer
            .apply(connection, writes, now_ms())
            .map_err(|error| error.to_string())
    });
    if let Err(error) = result {
        writer.invalidate_visits(writes);
        eprintln!("browsing history write failed: {error}");
    }
    writes.clear();
}

fn answer(path: &std::path::Path, connection: &mut Option<Connection>, read: Read) {
    let opened = connect(path, connection);
    let failed = |error: rusqlite::Error| error.to_string();
    // A reader that gave up has dropped its receiver; nothing to report.
    match read {
        Read::Page {
            search,
            before,
            limit,
            reply,
        } => {
            let _ = reply.send(opened.and_then(|connection| {
                store::page(connection, &search, before, limit).map_err(failed)
            }));
        }
        Read::Remove { ids, reply } => {
            let _ = reply.send(
                opened.and_then(|connection| store::remove(connection, &ids).map_err(failed)),
            );
        }
        Read::RemoveRange { from, to, reply } => {
            let _ =
                reply.send(opened.and_then(|connection| {
                    store::remove_range(connection, from, to).map_err(failed)
                }));
        }
    }
}

#[cfg(test)]
#[path = "browsing_history_tests.rs"]
mod tests;
