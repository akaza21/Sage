use url::Url;

pub(super) const MAX_BROWSER_URL_BYTES: usize = 2048;

pub(super) fn admit_url(input: &str) -> Result<Url, &'static str> {
    if input.is_empty()
        || input.len() > MAX_BROWSER_URL_BYTES
        || input.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("invalid browser URL");
    }
    let url = Url::parse(input).map_err(|_| "invalid browser URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("browser URL is not admitted");
    }
    Ok(url)
}
