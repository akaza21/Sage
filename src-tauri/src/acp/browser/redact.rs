use super::BrowserResult;
use regex::Regex;
use std::sync::LazyLock;

static URLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[a-z][a-z0-9+.-]*:\S+").expect("valid URL regex"));
static AUTH_HEADERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)\b(?:proxy-authorization|authorization|x-api-key|api-key|cookie|set-cookie)[ \t]*:[ \t]*[^\r\n]*")
        .expect("valid authorization header regex")
});
static ASSIGNMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(access_token|id_token|token|key|secret|password|auth|session|code|sig|signature|authorization|cookie)\b(\s*[:=]\s*)([^\s&;,#]+)")
        .expect("valid secret assignment regex")
});
static BARE_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}|(?:ghp_|github_pat_|sk-ant-|sk-|xox[bap]-)[A-Za-z0-9_-]{16,}|AKIA[A-Z0-9]{16})\b")
        .expect("valid bare token regex")
});

pub(super) fn redact_result(result: &mut BrowserResult) {
    match result {
        BrowserResult::Navigate { url, .. } => {
            *url = redact_url(url);
        }
        BrowserResult::SnapshotPage { text, nodes, .. } => {
            *text = redact_console(text);
            for node in nodes {
                node.name = redact_console(&node.name);
            }
        }
        BrowserResult::ReadNetwork { entries, .. } => {
            for entry in entries {
                entry.url = redact_url(&entry.url);
            }
        }
        BrowserResult::ReadConsole { entries, .. } => {
            for entry in entries {
                entry.text = redact_console(&entry.text);
            }
        }
        _ => {}
    }
}

fn sensitive(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "token"
            | "key"
            | "secret"
            | "password"
            | "auth"
            | "session"
            | "code"
            | "sig"
            | "signature"
            | "access_token"
            | "id_token"
            | "api_key"
            | "authorization"
            | "cookie"
    )
}

pub(crate) fn redact_url(input: &str) -> String {
    let Ok(mut url) = url::Url::parse(input) else {
        return "[redacted]".into();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return "[redacted]".into();
    }
    if url.set_username("").is_err() || url.set_password(None).is_err() {
        return "[redacted]".into();
    }
    if let Some(query) = url.query() {
        let Some(safe) = redact_parameters(query) else {
            return "[redacted]".into();
        };
        url.set_query(Some(&safe));
    }
    if let Some(fragment) = url.fragment() {
        let Some(parameters) = redact_parameters(fragment) else {
            return "[redacted]".into();
        };
        let safe = redact_assignments(&parameters);
        url.set_fragment(Some(&safe));
    }
    url.into()
}

pub(crate) fn redact_console(input: &str) -> String {
    let headers = AUTH_HEADERS.replace_all(input, |capture: &regex::Captures<'_>| {
        let name = capture[0].split(':').next().unwrap_or("authorization");
        format!("{name}: [redacted]")
    });
    let urls = URLS.replace_all(&headers, |capture: &regex::Captures<'_>| {
        redact_url(&capture[0])
    });
    let assignments = redact_assignments(&urls);
    BARE_TOKENS
        .replace_all(&assignments, "[redacted]")
        .into_owned()
}

fn redact_parameters(input: &str) -> Option<String> {
    let mut safe = Vec::new();
    for segment in input.split(['&', ';']) {
        let Some((key, value)) = segment.split_once('=') else {
            safe.push(segment.to_owned());
            continue;
        };
        if invalid_percent_encoding(key) {
            return None;
        }
        let decoded = url::form_urlencoded::parse(format!("{key}=").as_bytes())
            .next()?
            .0
            .into_owned();
        if decoded.contains('\u{fffd}') {
            return None;
        }
        safe.push(format!(
            "{key}={}",
            if sensitive(&decoded) {
                "[redacted]"
            } else {
                value
            }
        ));
    }
    Some(safe.join("&"))
}

fn invalid_percent_encoding(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == b'%'
            && (index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit())
    })
}

fn redact_assignments(input: &str) -> String {
    ASSIGNMENTS
        .replace_all(input, |capture: &regex::Captures<'_>| {
            format!("{}{}[redacted]", &capture[1], &capture[2])
        })
        .into_owned()
}
