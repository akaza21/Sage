use super::{
    error::PreviewError,
    url::{normalized_origin, parse_allowed_preview_url},
};

pub(crate) fn loopback_origin(raw: &str) -> Result<String, PreviewError> {
    Ok(normalized_origin(&parse_allowed_preview_url(raw)?))
}
