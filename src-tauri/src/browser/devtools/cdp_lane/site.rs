//! A loopback HTTP server for the CDP lane's fixture pages. One request per
//! connection, served in order; requests and pages are bounded.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const REQUEST_BYTES: usize = 8192;
/// Rows on the large page; each row is four DOM nodes.
const BIG_PAGE_ROWS: usize = 2_500;

pub(super) struct Site {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Site {
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture site");
        let address = listener.local_addr().expect("fixture site address");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            for stream in listener.incoming() {
                if worker_stop.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    respond(stream);
                }
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }

    pub(super) fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
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

fn respond(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) if request.len() + read > REQUEST_BYTES => return,
            Ok(read) => request.extend_from_slice(&chunk[..read]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    if let Some(delay) = delay(path) {
        thread::sleep(delay);
    }
    let (status, body) = match page(path) {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", String::from("not found")),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn page(path: &str) -> Option<String> {
    let body = match path {
        "/form.html" => include_str!("../../../../testdata/browser-site/form.html"),
        "/injection.html" => include_str!("../../../../testdata/browser-site/injection.html"),
        "/shared-old.html" => include_str!("../../../../testdata/browser-site/shared-old.html"),
        "/shared-new.html" => include_str!("../../../../testdata/browser-site/shared-new.html"),
        "/hostile.html" => include_str!("../../../../testdata/browser-site/hostile.html"),
        "/includes.html" => include_str!("../../../../testdata/browser-site/includes.html"),
        "/navigates.html" => include_str!("../../../../testdata/browser-site/navigates.html"),
        "/landing.html" => include_str!("../../../../testdata/browser-site/landing.html"),
        "/big.html" => return Some(big_page()),
        _ => return None,
    };
    Some(body.to_owned())
}

/// A slow page, and a subresource that keeps the slow page loading.
fn delay(path: &str) -> Option<Duration> {
    match path {
        "/shared-new.html" => Some(Duration::from_millis(2_500)),
        "/hang.png" => Some(Duration::from_millis(4_000)),
        _ => None,
    }
}

/// About 10,000 DOM nodes: every row is an item, a link, and two text nodes.
fn big_page() -> String {
    let mut body = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Big list</title></head><body><ul>",
    );
    for row in 0..BIG_PAGE_ROWS {
        body.push_str(&format!(
            "<li><a href=\"#row-{row}\">Row {row} link</a> detail text for row {row}</li>"
        ));
    }
    body.push_str("</ul></body></html>");
    body
}
