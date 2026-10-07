use super::{BrowserMcpError, BrowserMcpServer, BrowserMcpToken, MAX_SESSION_CONNECTIONS};
use crate::acp::browser::BrowserCapability;
use sha2::{Digest, Sha256};

impl std::fmt::Debug for BrowserMcpToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrowserMcpToken([redacted])")
    }
}

impl BrowserMcpToken {
    pub fn into_env_value(self) -> String {
        self.0
    }

    pub fn from_env_value(value: String) -> Result<Self, BrowserMcpError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(BrowserMcpError::Unauthorized);
        }
        Ok(Self(value))
    }
}

impl<P: BrowserCapability> BrowserMcpServer<P> {
    pub fn connect(
        &mut self,
        token: &BrowserMcpToken,
        session_id: &str,
        now_ms: u64,
    ) -> Result<(), BrowserMcpError> {
        if self.connected || session_id != self.session_id {
            return Err(BrowserMcpError::Unauthorized);
        }
        if self
            .expires_at
            .is_some_and(|expires_at| now_ms > expires_at)
        {
            return Err(BrowserMcpError::Expired);
        }
        let candidate: [u8; 32] = Sha256::digest(token.0.as_bytes()).into();
        if self
            .token_digest
            .iter()
            .zip(candidate)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            != 0
        {
            return Err(BrowserMcpError::Unauthorized);
        }
        self.connected = true;
        self.accepted_connections = 1;
        self.token_digest.fill(0);
        Ok(())
    }

    pub(crate) fn reconnect(
        &mut self,
        token: &BrowserMcpToken,
        session_id: &str,
    ) -> Result<(), BrowserMcpError> {
        if !self.connected || session_id != self.session_id {
            return Err(BrowserMcpError::Unauthorized);
        }
        if self.accepted_connections >= MAX_SESSION_CONNECTIONS {
            return Err(BrowserMcpError::Unavailable);
        }
        let Some(digest) = self.reconnect_digest else {
            return Err(BrowserMcpError::Unauthorized);
        };
        let candidate: [u8; 32] = Sha256::digest(token.0.as_bytes()).into();
        if digest
            .iter()
            .zip(candidate)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            != 0
        {
            return Err(BrowserMcpError::Unauthorized);
        }
        self.accepted_connections += 1;
        Ok(())
    }

    pub(crate) fn supports_reconnect(&self) -> bool {
        self.reconnect_digest.is_some()
    }
}
