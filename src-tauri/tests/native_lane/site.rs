use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::Value;

const HEAD_BYTES: usize = 8 * 1024;
const BODY_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 32;
const MAX_TARGETS: usize = 256;
const MAX_REPORTS: usize = 64;
pub const DOWNLOAD: &str = "/download.bin";
const DOWNLOAD_BODY: &[u8] = b"sage native lane download\n";
/// Held back long enough to press Stop.
pub const SLOW: &str = "/slow.html";
const SLOW_DELAY: Duration = Duration::from_secs(8);

const PAGES: &[(&str, &str)] = &[
    (
        "/device.html",
        include_str!("../../testdata/native-lane/device.html"),
    ),
    (
        "/annotate.html",
        include_str!("../../testdata/browser-site/annotate.html"),
    ),
    (
        "/hostile.html",
        include_str!("../../testdata/browser-site/hostile.html"),
    ),
    (
        "/plain.html",
        include_str!("../../testdata/native-lane/plain.html"),
    ),
    (SLOW, include_str!("../../testdata/native-lane/plain.html")),
    (
        "/s3-probe.html",
        include_str!("../../testdata/native-lane/s3-probe.html"),
    ),
    (
        "/s1-opener.html",
        include_str!("../../testdata/native-lane/s1-opener.html"),
    ),
    (
        "/s1-popup.html",
        include_str!("../../testdata/native-lane/s1-popup.html"),
    ),
];

#[derive(Default)]
struct Log {
    hits: BTreeMap<String, u32>,
    reports: Vec<(String, Value)>,
}

pub struct Site {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Log>>,
    worker: Option<JoinHandle<()>>,
}

impl Site {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Log::default()));
        let (worker_stop, worker_log) = (stop.clone(), log.clone());
        let worker = thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            for stream in listener.incoming() {
                if worker_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (active, log) = (active.clone(), worker_log.clone());
                thread::spawn(move || {
                    respond(stream, &log);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Self {
            address,
            stop,
            log,
            worker: Some(worker),
        })
    }

    pub fn url(&self, target: &str) -> String {
        format!("http://{}{target}", self.address)
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn hits(&self, target: &str) -> u32 {
        self.log
            .lock()
            .map_or(0, |log| log.hits.get(target).copied().unwrap_or(0))
    }

    pub fn reports(&self, name: &str) -> Vec<Value> {
        self.log.lock().map_or_else(
            |_| Vec::new(),
            |log| {
                log.reports
                    .iter()
                    .filter(|(report, _)| report == name)
                    .map(|(_, body)| body.clone())
                    .collect()
            },
        )
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn respond(mut stream: TcpStream, log: &Mutex<Log>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Some((head, mut body)) = read_head(&mut stream) else {
        return;
    };
    let mut lines = head.lines();
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let (method, target) = (request.next().unwrap_or(""), request.next().unwrap_or("/"));
    if let Ok(mut log) = log.lock()
        && (log.hits.len() < MAX_TARGETS || log.hits.contains_key(target))
    {
        *log.hits.entry(target.to_owned()).or_default() += 1;
    }
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if method == "GET" && target.split('?').next() == Some(SLOW) {
        thread::sleep(SLOW_DELAY);
    }
    let (status, page) = match (method, target.strip_prefix("/report/")) {
        ("POST", Some(name)) if length <= BODY_BYTES => {
            if read_body(&mut stream, &mut body, length) {
                let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                if let Ok(mut log) = log.lock()
                    && log.reports.len() < MAX_REPORTS
                {
                    log.reports.push((name.to_owned(), value));
                }
            }
            ("204 No Content", "")
        }
        ("GET", _) if target.split('?').next() == Some(DOWNLOAD) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"sage-lane.bin\"\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                DOWNLOAD_BODY.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(DOWNLOAD_BODY);
            return;
        }
        ("GET", _) => {
            let path = target.split('?').next().unwrap_or(target);
            PAGES
                .iter()
                .find(|(page, _)| *page == path)
                .map_or(("404 Not Found", "not found"), |(_, body)| {
                    ("200 OK", *body)
                })
        }
        _ => ("405 Method Not Allowed", ""),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        page.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(page.as_bytes());
}

fn read_head(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let body = buffer.split_off(end + 4);
            return Some((String::from_utf8_lossy(&buffer).into_owned(), body));
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(read) if buffer.len() + read > HEAD_BYTES + BODY_BYTES => return None,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
}

fn read_body(stream: &mut TcpStream, body: &mut Vec<u8>, length: usize) -> bool {
    let mut chunk = [0u8; 4096];
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    body.truncate(length);
    true
}
