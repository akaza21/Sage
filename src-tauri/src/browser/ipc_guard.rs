use url::Url;

pub(crate) fn admits_main_invoke(label: &str, url: &Url, dev_url: Option<&Url>) -> bool {
    if label != "main" || url.username() != "" || url.password().is_some() {
        return false;
    }
    let packaged =
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost") && url.port().is_none();
    #[cfg(not(windows))]
    let packaged = packaged
        || (url.scheme() == "tauri" && url.host_str() == Some("localhost") && url.port().is_none());
    let development = cfg!(debug_assertions)
        && dev_url.is_some_and(|development| url.origin() == development.origin());
    packaged || development
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admits_main_invoke(label: &str, url: &Url) -> bool {
        super::admits_main_invoke(
            label,
            url,
            Some(&Url::parse("http://127.0.0.1:1420").unwrap()),
        )
    }

    #[test]
    fn only_main_webview_at_local_app_origin_can_invoke_sage_commands() {
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
    }
}
