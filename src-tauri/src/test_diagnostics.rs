//! The one tracing capture for unit tests in this binary.
//!
//! tracing caches each callsite's `Interest` process-wide, decided on whichever
//! thread first reaches the callsite. A scoped `with_default` subscriber, or a
//! global filter that answers `always`/`never` there (as `filter_fn` does), lets
//! an unrelated test thread decide that this thread's events are never seen.
//! This subscriber is installed exactly once, answers `Interest::sometimes` for
//! every callsite, and routes each event to the capturing thread's buffer only.

use std::cell::RefCell;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::{Metadata, subscriber::Interest};
use tracing_subscriber::{
    Layer,
    fmt::MakeWriter,
    layer::{Context, Filter, SubscriberExt},
};

type Buffer = Arc<Mutex<Vec<u8>>>;

/// Bytes one capture keeps; later output is dropped.
const CAPTURE_BYTES: usize = 1024 * 1024;

struct Route {
    bytes: Buffer,
    admit: fn(&Metadata<'_>) -> bool,
}

thread_local! {
    static ROUTE: RefCell<Option<Route>> = const { RefCell::new(None) };
}

/// Admits an event only while its own thread is capturing.
struct CapturingThread;

impl<S> Filter<S> for CapturingThread {
    fn enabled(&self, metadata: &Metadata<'_>, _: &Context<'_, S>) -> bool {
        ROUTE
            .try_with(|route| {
                route
                    .borrow()
                    .as_ref()
                    .is_some_and(|route| (route.admit)(metadata))
            })
            .unwrap_or(false)
    }

    fn callsite_enabled(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }
}

struct RouteWriter(Option<Buffer>);

impl Write for RouteWriter {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if let Some(bytes) = &self.0 {
            let mut bytes = bytes.lock().unwrap();
            let room = CAPTURE_BYTES.saturating_sub(bytes.len());
            bytes.extend_from_slice(&input[..input.len().min(room)]);
        }
        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct CurrentRoute;

impl<'a> MakeWriter<'a> for CurrentRoute {
    type Writer = RouteWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RouteWriter(
            ROUTE
                .try_with(|route| route.borrow().as_ref().map(|r| Arc::clone(&r.bytes)))
                .ok()
                .flatten(),
        )
    }
}

fn install() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let layer = tracing_subscriber::fmt::layer()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_writer(CurrentRoute)
            .with_filter(CapturingThread);
        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
            .expect("test diagnostics capture must be the only tracing subscriber in this binary");
    });
}

struct RouteScope;

impl Drop for RouteScope {
    fn drop(&mut self) {
        ROUTE.with(|route| route.borrow_mut().take());
    }
}

/// Runs `body` and returns what it logged on this thread.
pub(crate) fn capture_diagnostics<T>(body: impl FnOnce() -> T) -> (T, String) {
    capture_matching(|_| true, body)
}

/// Runs `body` and returns what it logged on this thread that `admit` accepts.
pub(crate) fn capture_matching<T>(
    admit: fn(&Metadata<'_>) -> bool,
    body: impl FnOnce() -> T,
) -> (T, String) {
    install();
    let bytes = Buffer::default();
    ROUTE.with(|route| {
        assert!(route.borrow().is_none(), "diagnostics captures do not nest");
        *route.borrow_mut() = Some(Route {
            bytes: Arc::clone(&bytes),
            admit,
        });
    });
    let value = {
        let _scope = RouteScope;
        body()
    };
    let captured = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    (value, captured)
}

mod tests {
    use super::capture_diagnostics;

    fn warn_once() {
        tracing::warn!("callsite first reached off the capturing thread");
    }

    #[test]
    fn callsite_first_reached_on_another_thread_is_still_captured() {
        capture_diagnostics(|| ());
        std::thread::spawn(warn_once).join().unwrap();
        let ((), captured) = capture_diagnostics(warn_once);
        assert!(captured.contains("callsite first reached off the capturing thread"));
    }

    #[test]
    fn other_threads_do_not_reach_this_capture() {
        let ((), captured) = capture_diagnostics(|| {
            std::thread::spawn(|| tracing::warn!("logged by another thread"))
                .join()
                .unwrap();
        });
        assert!(!captured.contains("logged by another thread"));
    }
}
