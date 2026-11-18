mod auth;
mod client;
mod connection;
mod handshake_io;
mod protocol;
mod transport;

use std::{
    io,
    sync::{Arc, Mutex},
};
use tauri::Manager;

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
    let access = Arc::new(transport::Access::new(
        address.to_string(),
        browser_url.origin().ascii_serialization(),
        Arc::new(Mutex::new(auth::Pairing::new(code))),
    ));
    let close_access = access.clone();
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| io::Error::other("main window unavailable"))?;
    main.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event
            && close_access.request_close()
        {
            api.prevent_close();
        }
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
            return;
        };
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { break; };
                    // Room for a reloading tab to connect while its old session winds down.
                    if clients.len() >= 2 { drop(stream); continue; }
                    let access = access.clone();
                    let app = app.clone();
                    clients.spawn(async move {
                        if let Ok((socket, session)) = transport::accept(stream, &access).await {
                            client::serve(socket, app, session).await;
                        }
                    });
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        }
    });
    let print_link = std::env::var("SAGE_BRIDGE_PRINT_LINK").as_deref() == Ok("1");
    match pairing_notice(&browser_url, address.port(), print_link) {
        PairingNotice::PrintLink(message) => println!("{message}"),
        PairingNotice::OpenBrowser { failure_notice } => {
            if crate::system_browser::open_dev_bridge(&browser_url).is_err() {
                println!("{failure_notice}");
            }
        }
    }
    Ok(())
}

enum PairingNotice {
    PrintLink(String),
    OpenBrowser { failure_notice: String },
}

fn pairing_notice(url: &url::Url, port: u16, print_link: bool) -> PairingNotice {
    if print_link {
        PairingNotice::PrintLink(format!("Open Sage in your browser: {url}"))
    } else {
        PairingNotice::OpenBrowser {
            failure_notice: format!(
                "Sage browser bridge is listening on port {port}. Restart with SAGE_BRIDGE_PRINT_LINK=1 to print the pairing link."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_pairing_notice_prints_link_without_launching_only_with_opt_in() {
        let url =
            url::Url::parse("http://127.0.0.1:23456/?bridge=34567#private-pairing-code").unwrap();
        let PairingNotice::OpenBrowser { failure_notice } = pairing_notice(&url, 34567, false)
        else {
            panic!("without opt-in, launch the browser");
        };
        assert_eq!(
            failure_notice,
            "Sage browser bridge is listening on port 34567. Restart with SAGE_BRIDGE_PRINT_LINK=1 to print the pairing link."
        );
        let PairingNotice::PrintLink(message) = pairing_notice(&url, 34567, true) else {
            panic!("with opt-in, print the link without launching a browser");
        };
        assert_eq!(
            message,
            "Open Sage in your browser: http://127.0.0.1:23456/?bridge=34567#private-pairing-code"
        );
    }
}
