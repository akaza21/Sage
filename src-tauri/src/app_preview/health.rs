use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use serde::Serialize;

use super::{
    discovery::PreviewTargetRef,
    url::{normalized_origin, parse_allowed_preview_url},
};

const HEALTH_TIMEOUT: Duration = Duration::from_millis(750);

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
    let _ = stream.set_read_timeout(Some(HEALTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HEALTH_TIMEOUT));
    let host = normalized_origin(&url);
    if stream
        .write_all(
            format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .is_err()
    {
        return false;
    }
    let mut response = [0_u8; 64];
    match stream.read(&mut response) {
        Ok(size) => response[..size].starts_with(b"HTTP/"),
        Err(_) => false,
    }
}
