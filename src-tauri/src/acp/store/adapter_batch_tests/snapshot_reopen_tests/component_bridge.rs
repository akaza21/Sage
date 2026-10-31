//! Test-only, bounded transport for a headless component -> native command -> SQLite read.
use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_REQUESTS: usize = 4;
const BRIDGE_LIFETIME: Duration = Duration::from_secs(180);

fn read_before(stream: &mut TcpStream, bytes: &mut [u8], deadline: Instant) {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("snapshot request deadline");
        stream.set_read_timeout(Some(remaining)).unwrap();
        let count = stream.read(&mut bytes[offset..]).unwrap();
        assert!(count > 0, "snapshot request ended early");
        offset += count;
    }
}

fn request(stream: &mut TcpStream, bridge_deadline: Instant) -> (bool, Vec<u8>) {
    let deadline = bridge_deadline.min(Instant::now() + Duration::from_secs(2));
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        assert!(headers.len() < MAX_REQUEST_BYTES);
        let mut byte = [0];
        read_before(stream, &mut byte, deadline);
        headers.push(byte[0]);
    }
    let headers = String::from_utf8(headers).unwrap();
    let finished = headers.starts_with("POST /finish ");
    assert!(finished || headers.starts_with("POST /snapshot "));
    let length: usize = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse().unwrap())
        })
        .unwrap_or(0);
    assert!(length <= MAX_REQUEST_BYTES);
    let mut body = vec![0; length];
    read_before(stream, &mut body, deadline);
    (finished, body)
}

fn response(stream: &mut TcpStream, status: &str, body: &[u8]) {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
}

#[test]
fn component_snapshot_bridge_reads_historical_sqlite_bytes() {
    let (state, directory, owner, reference) = historical_snapshot_state();
    let page = state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let external = std::env::var_os("SAGE_GAP_C_SNAPSHOT_BOOTSTRAP");
    let self_client = if let Some(path) = external {
        let path = PathBuf::from(path);
        let lane = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        assert!(
            path.parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(lane)
        );
        fs::write(path, serde_json::to_vec(&serde_json::json!({
            "url": format!("http://{address}"), "page": page, "reference": reference,
            "project": {"id": owner.project_id.as_str(), "name": "Snapshot bridge", "roots": [{
                "id": reference.root_id, "name": "Root", "ordinal": 0, "availability": "available", "message": null
            }]}
        })).unwrap()).unwrap();
        None
    } else {
        let mut wire = serde_json::to_value(&reference).unwrap();
        wire["projectGeneration"] = serde_json::json!("0");
        wire["expectedProjectionRevision"] =
            serde_json::json!(page.projection_revision.get().to_string());
        Some(std::thread::spawn(move || {
            let body = serde_json::to_vec(&wire).unwrap();
            let mut stream = TcpStream::connect(address).unwrap();
            write!(
                stream,
                "POST /snapshot HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            let mut result = String::new();
            stream.read_to_string(&mut result).unwrap();
            assert!(result.contains("original old\\n"));
            assert!(result.contains("original new\\n"));
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .write_all(b"POST /finish HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            let mut result = String::new();
            stream.read_to_string(&mut result).unwrap();
        }))
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let deadline = Instant::now() + BRIDGE_LIFETIME;
    let mut served = 0;
    let mut finished = false;
    while Instant::now() < deadline && served < MAX_REQUESTS {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => panic!("snapshot bridge accept: {error}"),
        };
        stream.set_nonblocking(false).unwrap();
        let (finish, body) = request(&mut stream, deadline);
        if finish {
            response(&mut stream, "200 OK", b"{}");
            finished = true;
            break;
        }
        served += 1;
        let request = serde_json::from_slice(&body).unwrap();
        let result = runtime.block_on(
            crate::commands::acp::file_snapshot::get_acp_file_snapshot_inner(&state, request),
        );
        match result {
            Ok(value) => response(&mut stream, "200 OK", &serde_json::to_vec(&value).unwrap()),
            Err(error) => response(
                &mut stream,
                "409 Conflict",
                &serde_json::to_vec(&error).unwrap(),
            ),
        }
    }
    if let Some(client) = self_client {
        client.join().unwrap();
    }
    assert!(finished, "component snapshot bridge did not finish");
    assert!(served > 0, "component never read a native snapshot");
    drop(state);
    fs::remove_dir_all(directory).unwrap();
}
