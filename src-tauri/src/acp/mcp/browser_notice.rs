use super::McpAdmission;

impl Default for McpAdmission {
    fn default() -> Self {
        Self::from_trusted_descriptors(Vec::new()).expect("empty MCP admission is valid")
    }
}

impl McpAdmission {
    /// Retain a bounded notice while discarding every browser descriptor and
    /// environment entry before runtime construction.
    #[cfg(windows)]
    pub(crate) fn without_browser_tools(notice: &'static str) -> Self {
        Self {
            servers: Vec::new(),
            browser_unavailable_notice: Some(notice),
        }
    }

    pub(crate) fn take_browser_unavailable_notice(&mut self) -> Option<&'static str> {
        self.browser_unavailable_notice.take()
    }
}
