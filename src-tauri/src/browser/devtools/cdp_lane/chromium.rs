//! A CDP transport over a headless Chromium launched with
//! `--remote-debugging-pipe`: NUL-delimited JSON on fds 3 (in) and 4 (out),
//! with one flattened session attached to one page target.

use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::acp::browser::BrowserFailure;
use crate::browser::devtools::{
    snapshot::DocumentStamp,
    transport::{CdpTransport, Settled},
};

const DEFAULT_CHROMIUM: &str = "/opt/pw-browsers/chromium";
const FRAME_BYTES: usize = 16 * 1024 * 1024;
const PENDING_RESPONSES: usize = 16;
const DOCUMENTS: usize = 64;
/// Overlay events kept for the Annotate checks; the oldest go first.
const OVERLAY_EVENTS: usize = 64;
const SETUP_TIMEOUT: Duration = Duration::from_secs(20);

struct Waiting(Arc<AtomicBool>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub(super) struct Chromium {
    child: Child,
    link: Mutex<Link>,
    session: String,
    documents: Mutex<Vec<String>>,
    issued: AtomicBool,
    /// This lane pins native density to 1; overrides belong to its page session.
    capture_scale: Mutex<f64>,
    /// How many times each command method was sent to the page, with or
    /// without the dispatch permit.
    calls: Mutex<HashMap<String, usize>>,
    /// A stamp, or `Unavailable`, reported in place of the real one: WebView2
    /// reports a new document only after its navigation events arrive, and
    /// no stamp at all while a navigation loads.
    held_stamp: Mutex<Option<Result<DocumentStamp, BrowserFailure>>>,
    overlay_events: Arc<Mutex<VecDeque<Value>>>,
    profile: PathBuf,
    /// Shared with the frame reader, which releases developer mode's object
    /// group when an evaluation completes, as the WebView2 completion
    /// callback does, even after the caller stopped waiting.
    wire: Arc<Wire>,
}

#[path = "wire.rs"]
mod wire;
use wire::Wire;

struct Link {
    responses: Receiver<Value>,
}

impl Chromium {
    /// Launches Chromium from `SAGE_TEST_CHROMIUM` (default
    /// `/opt/pw-browsers/chromium`). A missing browser fails the test.
    pub(super) fn launch() -> Self {
        let binary = std::env::var_os("SAGE_TEST_CHROMIUM")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CHROMIUM));
        assert!(
            Path::new(&binary).is_file(),
            "the cdp-lane feature needs Chromium at {}; set SAGE_TEST_CHROMIUM",
            binary.display()
        );
        let profile =
            std::env::temp_dir().join(format!("sage-cdp-lane-{}", uuid::Uuid::new_v4().simple()));
        // std cannot place pipes on fds 3 and 4, so a shell moves the child's
        // stdin and stdout there before it becomes Chromium.
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"exec "$0" "$@" 3<&0 4>&1 </dev/null >/dev/null"#)
            .arg(&binary)
            .args([
                "--headless=new",
                "--force-device-scale-factor=1",
                "--remote-debugging-pipe",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--mute-audio",
            ])
            // Chromium refuses to sandbox as root, as in cloud containers.
            .args(running_as_root().then_some("--no-sandbox"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("launch Chromium");
        let wire = Arc::new(Wire::default());
        *wire.commands.lock().expect("command pipe") =
            Some(child.stdin.take().expect("Chromium command pipe"));
        let events = child.stdout.take().expect("Chromium response pipe");
        let (sender, responses) = mpsc::sync_channel(PENDING_RESPONSES);
        let overlay_events = Arc::new(Mutex::new(VecDeque::new()));
        let kept = overlay_events.clone();
        let reader_wire = wire.clone();
        thread::spawn(move || read_frames(events, sender, kept, reader_wire));
        let mut chromium = Self {
            child,
            link: Mutex::new(Link { responses }),
            session: String::new(),
            documents: Mutex::new(Vec::new()),
            issued: AtomicBool::new(false),
            capture_scale: Mutex::new(1.0),
            calls: Mutex::new(HashMap::new()),
            held_stamp: Mutex::new(None),
            overlay_events,
            profile,
            wire,
        };
        let target = chromium.setup(None, "Target.createTarget", json!({"url": "about:blank"}));
        let attached = chromium.setup(
            None,
            "Target.attachToTarget",
            json!({"targetId": target["targetId"], "flatten": true}),
        );
        chromium.session = attached["sessionId"]
            .as_str()
            .expect("flattened session id")
            .to_owned();
        // The same domains the WebView2 transport enables when it attaches.
        for domain in ["Runtime.enable", "Log.enable", "Network.enable"] {
            let session = chromium.session.clone();
            chromium.setup(Some(&session), domain, json!({}));
        }
        chromium
    }

    /// Starts a new dispatch, so `issued` reports only this command's calls,
    /// as the WebView2 transport's per-dispatch flag does.
    pub(super) fn begin_dispatch(&self) {
        self.issued.store(false, Ordering::Release);
    }

    /// How many commands named `method` this transport has sent to the page,
    /// through `call` or `observe`.
    /// How many times the frame reader released developer mode's object
    /// group when an evaluation's answer arrived.
    pub(super) fn completion_releases(&self) -> usize {
        self.wire.late_releases.load(Ordering::SeqCst)
    }

    pub(super) fn calls(&self, method: &str) -> usize {
        self.calls
            .lock()
            .expect("call counts")
            .get(method)
            .copied()
            .unwrap_or(0)
    }

    fn count(&self, method: &str) -> Result<(), BrowserFailure> {
        *self
            .calls
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .entry(method.to_owned())
            .or_default() += 1;
        Ok(())
    }

    /// Takes the `Overlay.*` events Chromium sent since the last take.
    pub(super) fn take_overlay_events(&self) -> Vec<Value> {
        self.overlay_events
            .lock()
            .expect("overlay events")
            .drain(..)
            .collect()
    }

    /// Keeps reporting `stamp` until released with `None`.
    pub(super) fn hold_stamp(&self, stamp: Option<Result<DocumentStamp, BrowserFailure>>) {
        *self.held_stamp.lock().expect("held stamp") = stamp;
    }

    fn setup(&self, session: Option<&str>, method: &str, params: Value) -> Value {
        let result = self
            .send(
                session,
                method,
                &params.to_string(),
                FRAME_BYTES,
                SETUP_TIMEOUT,
            )
            .unwrap_or_else(|error| panic!("Chromium {method} failed: {error:?}"));
        serde_json::from_str(&result).expect("Chromium setup result")
    }

    fn send(
        &self,
        session: Option<&str>,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        let params: Value =
            serde_json::from_str(params).map_err(|_| BrowserFailure::InvalidRequest)?;
        let capture_scale = match method {
            "Emulation.clearDeviceMetricsOverride" => Some(1.0),
            "Emulation.setDeviceMetricsOverride" => params["deviceScaleFactor"]
                .as_f64()
                .map(|scale| if scale == 0.0 { 1.0 } else { scale }),
            _ => None,
        };
        let link = self.link.lock().map_err(|_| BrowserFailure::Unavailable)?;
        let release =
            crate::acp::browser::developer::releases_on_completion(method, &params.to_string());
        let waiting = (release && method == "Runtime.evaluate" && params["returnByValue"] == false)
            .then(|| Waiting(Arc::new(AtomicBool::new(true))));
        let id = self.wire.write_waiting(
            session,
            method,
            params,
            release,
            waiting.as_ref().map(|waiting| waiting.0.clone()),
        )?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = match link.responses.recv_timeout(remaining) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => return Err(BrowserFailure::OutcomeUnknown),
                Err(RecvTimeoutError::Disconnected) => return Err(BrowserFailure::Unavailable),
            };
            // A late answer to an earlier, timed-out command is dropped.
            if message["id"].as_u64() != Some(id) {
                continue;
            }
            if message.get("error").is_some() {
                return Err(BrowserFailure::Unavailable);
            }
            let result = message
                .get("result")
                .ok_or(BrowserFailure::Unavailable)?
                .to_string();
            if result.len() > max_result_bytes {
                return Err(BrowserFailure::Capacity);
            }
            if let Some(scale) = capture_scale {
                *self
                    .capture_scale
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)? = scale;
            }
            return Ok(result);
        }
    }

    /// The main frame's loader id and URL.
    fn frame(&self) -> Result<(String, String), BrowserFailure> {
        // A page can grow its frame tree past any small cap; the test
        // transport reads it whole, as WebView2 keeps the stamp natively.
        let tree = self.observe(
            "Page.getFrameTree",
            "{}",
            FRAME_BYTES,
            Duration::from_secs(5),
        )?;
        let tree: Value = serde_json::from_str(&tree).map_err(|_| BrowserFailure::Unavailable)?;
        let frame = &tree["frameTree"]["frame"];
        match (frame["loaderId"].as_str(), frame["url"].as_str()) {
            (Some(loader), Some(url)) => Ok((loader.to_owned(), url.to_owned())),
            _ => Err(BrowserFailure::Unavailable),
        }
    }

    /// Numbers each distinct main-frame document in the order first seen.
    fn document_stamp(&self, loader: String) -> Result<DocumentStamp, BrowserFailure> {
        let mut documents = self
            .documents
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        let index = match documents.iter().position(|seen| *seen == loader) {
            Some(index) => index,
            None if documents.len() == DOCUMENTS => return Err(BrowserFailure::Capacity),
            None => {
                documents.push(loader);
                documents.len() - 1
            }
        };
        let id = index as u64 + 1;
        Ok(DocumentStamp {
            navigation_id: id,
            generation: id,
        })
    }
}

impl CdpTransport for Chromium {
    fn screenshot_scale(&self, _: Duration) -> Result<f64, BrowserFailure> {
        self.capture_scale
            .lock()
            .map(|scale| *scale)
            .map_err(|_| BrowserFailure::Unavailable)
    }

    fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        self.issued.store(true, Ordering::Release);
        self.count(method)?;
        self.send(
            Some(&self.session),
            method,
            params,
            max_result_bytes,
            timeout,
        )
    }

    fn observe(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        self.count(method)?;
        self.send(
            Some(&self.session),
            method,
            params,
            max_result_bytes,
            timeout,
        )
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        if let Some(held) = *self
            .held_stamp
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
        {
            return held;
        }
        let (loader, _) = self.frame()?;
        self.document_stamp(loader)
    }

    fn issued(&self) -> bool {
        self.issued.load(Ordering::Acquire)
    }

    fn settled(&self, before: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        let (loader, url) = self.frame()?;
        let stamp = self.document_stamp(loader)?;
        if stamp.navigation_id <= before.navigation_id {
            return Ok(None);
        }
        let ready = self.observe(
            "Runtime.evaluate",
            r#"{"expression":"document.readyState","returnByValue":true}"#,
            4096,
            Duration::from_secs(5),
        )?;
        let ready: Value = serde_json::from_str(&ready).map_err(|_| BrowserFailure::Unavailable)?;
        if ready["result"]["value"] != "complete" {
            return Ok(None);
        }
        Ok(Some(Settled {
            url,
            navigation_id: stamp.navigation_id,
        }))
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        let _ = self.send(None, "Browser.close", "{}", 4096, Duration::from_secs(2));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Splits Chromium's output into frames and forwards command responses;
/// `Overlay.*` events are kept, others dropped. A frame over `FRAME_BYTES`
/// ends the link.
fn read_frames(
    mut events: ChildStdout,
    responses: SyncSender<Value>,
    overlay: Arc<Mutex<VecDeque<Value>>>,
    wire: Arc<Wire>,
) {
    let mut pending = Vec::new();
    let mut chunk = vec![0u8; 65_536];
    loop {
        let read = match events.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let mut scan = pending.len();
        pending.extend_from_slice(&chunk[..read]);
        while let Some(offset) = pending[scan..].iter().position(|byte| *byte == 0) {
            let end = scan + offset;
            let message = serde_json::from_slice::<Value>(&pending[..end]);
            pending.drain(..=end);
            scan = 0;
            let Ok(message) = message else {
                return;
            };
            if message["method"]
                .as_str()
                .is_some_and(|method| method.starts_with("Overlay."))
                && let Ok(mut kept) = overlay.lock()
            {
                if kept.len() == OVERLAY_EVENTS {
                    kept.pop_front();
                }
                kept.push_back(message);
                continue;
            }
            if let Some(id) = message["id"].as_u64() {
                wire.answered(id);
            }
            if message.get("id").is_some() && responses.send(message).is_err() {
                return;
            }
        }
        if pending.len() > FRAME_BYTES {
            return;
        }
    }
}

fn running_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|process| process.uid() == 0)
}
