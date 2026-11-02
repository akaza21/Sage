use std::{
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::acp::limits::PREVIEW_LIMITS;

use super::{discovery::PreviewTargetRef, url::parse_allowed_preview_url};

const HEALTH_TIMEOUT: Duration = Duration::from_millis(PREVIEW_LIMITS.health_probe_timeout_ms);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewHealthResponse {
    #[serde(rename = "ref")]
    pub ref_: PreviewTargetRef,
    pub origin: Option<String>,
    pub healthy: bool,
}

pub(crate) fn get(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    let Ok(url) = parse_allowed_preview_url(origin) else {
        return false;
    };
    if url.scheme() == "https" {
        return false;
    }
    let Some(address) = url
        .socket_addrs(|| None)
        .ok()
        .and_then(|mut values| values.pop())
    else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, HEALTH_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(HEALTH_TIMEOUT));
    let host = address;
    if stream
        .write_all(
            format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .is_err()
    {
        return false;
    }
    // Only the HTTP prefix is needed. Share one deadline across fragmented reads.
    let deadline = Instant::now() + HEALTH_TIMEOUT;
    let mut response = [0_u8; 5];
    let mut received = 0;
    while received < response.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return false;
        }
        match stream.read(&mut response[received..]) {
            Ok(0) => return false,
            Ok(size) => received += size,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    &response == b"HTTP/"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    fn probe(listener: TcpListener, respond: impl FnOnce(&mut TcpStream) + Send + 'static) -> bool {
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 1024);
            }
            let request = String::from_utf8(request).unwrap();
            let host = request
                .lines()
                .find(|line| line.starts_with("Host:"))
                .unwrap();
            assert!(!host.contains("://"));
            assert!(request.contains(&format!("Host: {address}\r\n")));
            respond(&mut stream);
        });
        let healthy = get(Some(&format!("http://{address}")));
        server.join().unwrap();
        healthy
    }

    #[test]
    fn ipv4_host_header() {
        assert!(probe(TcpListener::bind("127.0.0.1:0").unwrap(), |stream| {
            stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
        }));
    }

    #[test]
    fn ipv6_host_header() {
        let Ok(listener) = TcpListener::bind("[::1]:0") else {
            return;
        };
        assert!(probe(listener, |stream| {
            stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
        }));
    }

    #[test]
    fn fragmented_http_prefix() {
        assert!(probe(TcpListener::bind("127.0.0.1:0").unwrap(), |stream| {
            stream.write_all(b"HT").unwrap();
            stream.flush().unwrap();
            thread::sleep(Duration::from_millis(30));
            stream.write_all(b"TP/1.1 200 OK\r\n\r\n").unwrap();
        }));
    }

    #[test]
    fn non_http_response_is_unhealthy() {
        assert!(!probe(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            |stream| {
                stream.write_all(b"not HTTP").unwrap();
            }
        ));
    }

    #[test]
    fn closed_connection_is_unhealthy() {
        assert!(!probe(TcpListener::bind("127.0.0.1:0").unwrap(), |_| {}));
    }

    #[test]
    fn fragmented_response_shares_one_read_deadline() {
        assert!(!probe(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            |stream| {
                thread::sleep(HEALTH_TIMEOUT * 2 / 3);
                let _ = stream.write_all(b"HT");
                thread::sleep(HEALTH_TIMEOUT * 2 / 3);
                let _ = stream.write_all(b"TP/1.1 200 OK\r\n\r\n");
            }
        ));
    }

    #[test]
    fn incomplete_http_prefix_is_unhealthy() {
        assert!(!probe(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            |stream| {
                stream.write_all(b"HT").unwrap();
            }
        ));
    }
}
