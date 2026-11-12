use super::NativeAuthUrl;
use crate::acp::bindings::codex::{AuthBrowser, CodexDiscoveryRunError};

pub(crate) struct NativeBrowser;

impl AuthBrowser for NativeBrowser {
    fn open(&self, url: &NativeAuthUrl) -> Result<(), CodexDiscoveryRunError> {
        crate::system_browser::open_https(url.as_str()).map_err(|error| match error {
            crate::system_browser::BrowserError::Unavailable => CodexDiscoveryRunError::Unavailable,
            crate::system_browser::BrowserError::Failed => CodexDiscoveryRunError::Transport,
        })
    }
}
