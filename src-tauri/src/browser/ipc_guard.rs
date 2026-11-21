use url::Url;

pub(crate) fn admits_main_invoke(label: &str, url: &Url, _dev_url: Option<&Url>) -> bool {
    if label != "main" || url.username() != "" || url.password().is_some() {
        return false;
    }
    let packaged =
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost") && url.port().is_none();
    #[cfg(not(windows))]
    let packaged = packaged
        || (url.scheme() == "tauri" && url.host_str() == Some("localhost") && url.port().is_none());
    #[cfg(debug_assertions)]
    let development = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && _dev_url
            .and_then(Url::port_or_known_default)
            .is_some_and(|port| url.port_or_known_default() == Some(port));
    #[cfg(not(debug_assertions))]
    let development = false;
    packaged || development
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_main_webview_at_local_app_origin_can_invoke_sage_commands() {
        let dev_url = Url::parse("http://127.0.0.1:1420/").unwrap();
        let admits_main_invoke =
            |label: &str, url: &Url| super::admits_main_invoke(label, url, Some(&dev_url));
        let app = Url::parse("http://tauri.localhost/index.html").unwrap();
        assert!(admits_main_invoke("main", &app));
        assert!(!admits_main_invoke("browser-1", &app));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("https://example.com/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "browser-1",
            &Url::parse("https://example.com/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("http://tauri.localhost.evil.test/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("http://tauri.localhost:1420/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("https://tauri.localhost/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("tauri://localhost:9/").unwrap()
        ));
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("http://127.0.0.1:1421/").unwrap()
        ));
        #[cfg(debug_assertions)]
        assert!(admits_main_invoke(
            "main",
            &Url::parse("http://127.0.0.1:1420/").unwrap()
        ));
        #[cfg(not(debug_assertions))]
        assert!(!admits_main_invoke(
            "main",
            &Url::parse("http://127.0.0.1:1420/").unwrap()
        ));
    }

    #[test]
    fn development_origin_uses_only_the_configured_port_in_debug_builds() {
        let dev_url = Url::parse("http://127.0.0.1:15432/").unwrap();
        let configured = Url::parse("http://127.0.0.1:15432/index.html").unwrap();
        #[cfg(debug_assertions)]
        assert!(admits_main_invoke("main", &configured, Some(&dev_url)));
        #[cfg(not(debug_assertions))]
        assert!(!admits_main_invoke("main", &configured, Some(&dev_url)));
        for refused in [
            "http://127.0.0.1:1420/",
            "http://127.0.0.1:15433/",
            "http://localhost:15432/",
            "http://127.0.0.2:15432/",
            "https://127.0.0.1:15432/",
            "http://user@127.0.0.1:15432/",
            "http://:password@127.0.0.1:15432/",
        ] {
            assert!(
                !admits_main_invoke("main", &Url::parse(refused).unwrap(), Some(&dev_url)),
                "admitted {refused}"
            );
        }
        assert!(!admits_main_invoke(
            "browser-1",
            &configured,
            Some(&dev_url)
        ));
        assert!(!admits_main_invoke("main", &configured, None));
    }
}
