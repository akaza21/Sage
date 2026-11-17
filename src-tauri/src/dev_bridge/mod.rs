mod auth;
mod client;
mod protocol;
mod transport;

use std::{
    io,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

pub(crate) fn host_url(app: &tauri::AppHandle) -> io::Result<url::Url> {
    let mut url = app
        .config()
        .build
        .dev_url
        .clone()
        .ok_or_else(|| io::Error::other("browser bridge requires devUrl"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(io::Error::other(
            "browser bridge requires a loopback HTTP devUrl",
        ));
    }
    url.set_fragment(None);
    url.query_pairs_mut().append_pair("bridge-host", "");
    Ok(url)
}

pub(crate) fn start(app: &tauri::AppHandle) -> io::Result<()> {
    let mut browser_url = host_url(app)?;
    browser_url.set_query(None);
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let code = auth::random_secret()?;
    browser_url
        .query_pairs_mut()
        .append_pair("bridge", &address.port().to_string());
    browser_url.set_fragment(Some(&code));
    let access = Arc::new(transport::Access {
        host: address.to_string(),
        origin: browser_url.origin().ascii_serialization(),
        pairing: Arc::new(Mutex::new(auth::Pairing::new(code))),
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
            return;
        };
        let permits = Arc::new(Semaphore::new(protocol::MAX_PENDING));
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { break; };
                    // One paired client plus one bounded handshake to reject concurrent arrivals.
                    if clients.len() >= 2 { drop(stream); continue; }
                    let access = access.clone();
                    let app = app.clone();
                    let permits = permits.clone();
                    clients.spawn(async move {
                        if let Ok((socket, _lease)) = transport::accept(stream, &access).await {
                            client::serve(socket, app, permits).await;
                        }
                    });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    });
    // The link carries the pairing code, so it goes to the terminal only when the
    // browser cannot be opened, and never to tracing.
    if crate::system_browser::open_dev_bridge(&browser_url).is_err() {
        println!("Open Sage in your browser: {browser_url}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn invokes_follow_the_configured_development_origin_without_admitting_children() {
        use crate::browser::ipc_guard::admits_main_invoke;
        let configured = url::Url::parse("http://127.0.0.1:23456").unwrap();
        let host = configured.join("/?bridge-host").unwrap();
        assert!(admits_main_invoke("main", &host, Some(&configured)));
        assert!(!admits_main_invoke("browser-1", &host, Some(&configured)));
        assert!(!admits_main_invoke(
            "main",
            &url::Url::parse("http://127.0.0.1:23457").unwrap(),
            Some(&configured)
        ));
    }
}
