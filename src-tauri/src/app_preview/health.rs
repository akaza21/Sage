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
    // Bound the complete status line, sharing one deadline across fragmented reads.
    let deadline = Instant::now() + HEALTH_TIMEOUT;
    let mut response = [0_u8; 64];
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
        if let Some(end) = response[..received]
            .windows(2)
            .position(|pair| pair == b"\r\n")
        {
            return valid_status_line(&response[..end]);
        }
    }
    false
}

fn valid_status_line(line: &[u8]) -> bool {
    line.len() >= 13
        && line.starts_with(b"HTTP/1.")
        && line[7].is_ascii_digit()
        && line[8] == b' '
        && line[9..12].iter().all(u8::is_ascii_digit)
        && line[12] == b' '
        && line[13..]
            .iter()
            .all(|&byte| matches!(byte, b'\t' | b' '..=b'~' | 0x80..=0xff))
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
        let listener = TcpListener::bind("[::1]:0")
            .expect("IPv6 loopback is required for the IPv6 Host-header test");
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

    #[test]
    fn malformed_status_lines_are_unhealthy() {
        for response in [
            b"HTTP/".as_slice(),
            b"HTTP/garbage",
            b"HTTP/1.1 20 OK\r\n",
            b"HTTP/1.1 2000 OK\r\n",
            b"HTTP/1.1 abc OK\r\n",
            b"HTTP/2.0 200 OK\r\n",
            b"HTTP/1.x 200 OK\r\n",
            b"HTTP/1.1 200\r\n",
            b"HTTP/1.1 200 O\0K\r\n",
            b"HTTP/1.1 200 OK\n",
        ] {
            assert!(
                !probe(TcpListener::bind("127.0.0.1:0").unwrap(), move |stream| {
                    stream.write_all(response).unwrap();
                }),
                "accepted {response:?}"
            );
        }
    }

    #[test]
    fn oversized_unterminated_status_line_is_unhealthy() {
        assert!(!probe(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            |stream| {
                let mut response = b"HTTP/1.1 200 ".to_vec();
                response.extend([b'x'; 128]);
                stream.write_all(&response).unwrap();
            }
        ));
    }

    #[test]
    fn status_line_byte_cap_includes_crlf() {
        for (size, healthy) in [(64, true), (65, false)] {
            assert_eq!(
                probe(TcpListener::bind("127.0.0.1:0").unwrap(), move |stream| {
                    let mut response = b"HTTP/1.1 200 ".to_vec();
                    response.resize(size - 2, b'x');
                    response.extend(b"\r\n");
                    stream.write_all(&response).unwrap();
                }),
                healthy,
                "status line size {size}"
            );
        }
    }

    #[test]
    fn every_status_line_split_is_healthy() {
        const STATUS: &[u8] = b"HTTP/1.1 200 OK\r\n";
        for split in 1..STATUS.len() {
            assert!(
                probe(TcpListener::bind("127.0.0.1:0").unwrap(), move |stream| {
                    stream.set_nodelay(true).unwrap();
                    stream.write_all(&STATUS[..split]).unwrap();
                    stream.flush().unwrap();
                    thread::sleep(Duration::from_millis(30));
                    stream.write_all(&STATUS[split..]).unwrap();
                }),
                "status line split at {split}"
            );
        }
    }

    #[test]
    fn valid_status_lines_do_not_require_a_success_code_or_reason() {
        for response in [
            b"HTTP/1.0 200 \r\n".as_slice(),
            b"HTTP/1.1 503 Unavailable\r\n",
        ] {
            assert!(probe(
                TcpListener::bind("127.0.0.1:0").unwrap(),
                move |stream| {
                    stream.write_all(response).unwrap();
                }
            ));
        }
    }
}
