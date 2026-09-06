use url::Url;

use super::error::PreviewError;
use crate::acp::limits::PREVIEW_LIMITS;

pub(crate) const MAX_PREVIEW_URL_BYTES: usize = PREVIEW_LIMITS.url_bytes as usize;

pub(crate) fn parse_allowed_preview_url(raw: &str) -> Result<Url, PreviewError> {
    if raw.is_empty()
        || raw.len() > MAX_PREVIEW_URL_BYTES
        || raw.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(PreviewError::invalid("Preview origin is invalid."));
    }
    let parsed =
        Url::parse(raw).map_err(|_| PreviewError::invalid("Preview origin is invalid."))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_none()
        || !matches!(
            parsed.host_str(),
            Some("127.0.0.1") | Some("::1") | Some("[::1]")
        )
    {
        return Err(PreviewError::invalid(
            "Preview origins must be loopback HTTP(S) URLs.",
        ));
    }
    Ok(parsed)
}

pub(crate) fn normalized_origin(url: &Url) -> String {
    let host = if matches!(url.host_str(), Some("::1") | Some("[::1]")) {
        "[::1]"
    } else {
        "127.0.0.1"
    };
    format!(
        "{}://{}:{}",
        url.scheme(),
        host,
        url.port().unwrap_or_default()
    )
}
