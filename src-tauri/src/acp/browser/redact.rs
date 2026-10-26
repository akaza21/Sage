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
static SECRET_SHAPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*|[0-9A-Fa-f]{32,}|[A-Za-z0-9+/]{32,}={0,2})$")
        .expect("valid secret shape regex")
});
static MIXED_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{24,}$").expect("valid mixed token regex"));
/// What a redacted path segment becomes, already percent-encoded so that a
/// second pass leaves it as it is.
const REDACTED_SEGMENT: &str = "%5Bredacted%5D";

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
            | "refresh_token"
            | "client_secret"
            | "client_assertion"
            | "apikey"
            | "api-key"
            | "x-amz-signature"
            | "x-amz-credential"
            | "x-amz-security-token"
            | "x-goog-signature"
            | "state"
    )
}

/// Whether a bare value looks like a credential: a known token prefix, a
/// JWT, a hex or base64 run of 32 or more characters, or 24 or more
/// `[A-Za-z0-9_-]` characters mixing upper case, lower case and digits.
fn secret_like(value: &str) -> bool {
    BARE_TOKENS.is_match(value)
        || SECRET_SHAPES.is_match(value)
        || (MIXED_TOKEN.is_match(value)
            && value.bytes().any(|byte| byte.is_ascii_uppercase())
            && value.bytes().any(|byte| byte.is_ascii_lowercase())
            && value.bytes().any(|byte| byte.is_ascii_digit()))
}

/// A path segment is redacted when it looks like a credential, except a git
/// object id (exactly 40 or 64 hex characters), which forge URLs carry.
fn secret_segment(segment: &str) -> bool {
    let hash =
        matches!(segment.len(), 40 | 64) && segment.bytes().all(|byte| byte.is_ascii_hexdigit());
    !hash && secret_like(segment)
}

/// Percent-decodes `input`, keeping malformed escapes as they are.
fn percent_decoded(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| input.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escape {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
    let path = url.path().to_owned();
    let safe_path = path
        .split('/')
        .map(|segment| {
            if secret_segment(&percent_decoded(segment)) {
                REDACTED_SEGMENT
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    if safe_path != path {
        url.set_path(&safe_path);
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
            safe.push(if secret_like(&percent_decoded(segment)) {
                "[redacted]".to_owned()
            } else {
                segment.to_owned()
            });
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
            if sensitive(&decoded) || secret_like(&percent_decoded(value)) {
                "[redacted]"
            } else {
                value
            }
        ));
    }
    Some(safe.join("&"))
}

/// Drops control characters (keeping `\n` when `multiline`) and bidi
/// overrides, then cuts the text to at most `limit` bytes.
pub(crate) fn safe_page_text(value: &str, limit: usize, multiline: bool) -> String {
    let filtered: String = value
        .chars()
        .filter(|ch| {
            (!ch.is_control() || (multiline && *ch == '\n'))
                && !matches!(*ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let mut end = filtered.len().min(limit);
    while !filtered.is_char_boundary(end) {
        end -= 1;
    }
    filtered[..end].to_owned()
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

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
