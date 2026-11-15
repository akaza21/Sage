use url::Url;

pub(super) const MAX_ORIGIN_BYTES: usize = 4 * 1024;

/// `scheme://host[:port]`, default port left out. Only `http` and `https`
/// sites qualify: imported `android://` and similar entries have nowhere to
/// fill in Sage's browser.
pub(super) fn credential_origin(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_ORIGIN_BYTES {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_keep_scheme_host_and_a_non_default_port_only() {
        assert_eq!(
            credential_origin(" https://Example.com/login?next=1 ").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            credential_origin("https://example.com:443/").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            credential_origin("http://localhost:8080/admin").as_deref(),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn origins_refuse_other_schemes_bare_hosts_and_oversized_input() {
        for raw in [
            "",
            "example.com",
            "android://hash@com.example.app/",
            "ftp://example.com",
            "file:///C:/secrets.txt",
            "javascript:alert(1)",
        ] {
            assert_eq!(credential_origin(raw), None, "{raw}");
        }
        let long = format!("https://{}.com", "a".repeat(MAX_ORIGIN_BYTES));
        assert_eq!(credential_origin(&long), None);
    }
}
