use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    crypto::ring::default_provider,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
};

pub const SECRET: &str = "S10-TLS-PAGE-7c41";
const MAX_CONNECTIONS: usize = 16;

#[derive(Default)]
pub struct Counts {
    pub connections: AtomicU32,
    pub handshakes: AtomicU32,
    pub requests: AtomicU32,
}

pub struct TlsSite {
    address: SocketAddr,
    certificate: CertificateDer<'static>,
    stop: Arc<AtomicBool>,
    pub counts: Arc<Counts>,
    worker: Option<JoinHandle<()>>,
}

impl TlsSite {
    pub fn start() -> Result<Self, String> {
        let generated =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
                .map_err(|error| format!("certificate: {error}"))?;
        let certificate = generated.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            generated.signing_key.serialize_der(),
        ));
        let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], key)
            .map_err(|error| format!("server config: {error}"))?;
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Counts::default());
        let (worker_stop, worker_counts) = (stop.clone(), counts.clone());
        let worker = thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            for stream in listener.incoming() {
                if worker_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                worker_counts.connections.fetch_add(1, Ordering::SeqCst);
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (active, counts, config) =
                    (active.clone(), worker_counts.clone(), config.clone());
                thread::spawn(move || {
                    serve(stream, config, &counts);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Self {
            address,
            certificate,
            stop,
            counts,
            worker: Some(worker),
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("https://{}{path}", self.address)
    }

    /// Proves the fixture serves and counts, so a zero later means the browser
    /// never got the page.
    pub fn fetch_pinned(&self, path: &str) -> Result<String, String> {
        let mut roots = RootCertStore::empty();
        roots
            .add(self.certificate.clone())
            .map_err(|error| format!("pin: {error}"))?;
        let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = ServerName::try_from("127.0.0.1").map_err(|error| error.to_string())?;
        let connection =
            ClientConnection::new(Arc::new(config), name).map_err(|error| error.to_string())?;
        let socket = TcpStream::connect(self.address).map_err(|error| error.to_string())?;
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        let mut stream = StreamOwned::new(connection, socket);
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .map_err(|error| format!("write: {error}"))?;
        let mut response = Vec::new();
        let mut chunk = [0u8; 4096];
        while response.len() < 64 * 1024 {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&chunk[..read]),
                Err(_) if !response.is_empty() => break,
                Err(error) => return Err(format!("read: {error}")),
            }
        }
        Ok(String::from_utf8_lossy(&response).into_owned())
    }
}

impl Drop for TlsSite {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(socket: TcpStream, config: Arc<ServerConfig>, counts: &Counts) {
    let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(connection) = ServerConnection::new(config) else {
        return;
    };
    let mut stream = StreamOwned::new(connection, socket);
    while stream.conn.is_handshaking() {
        if stream.conn.complete_io(&mut stream.sock).is_err() {
            return;
        }
    }
    counts.handshakes.fetch_add(1, Ordering::SeqCst);
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(read) if read > 0 && head.len() + read <= 8192 => {
                head.extend_from_slice(&chunk[..read]);
            }
            _ => return,
        }
    }
    counts.requests.fetch_add(1, Ordering::SeqCst);
    let page = format!("<!doctype html><title>TLS fixture</title><p>{SECRET}</p>");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    stream.conn.send_close_notify();
    let _ = stream.conn.complete_io(&mut stream.sock);
}
