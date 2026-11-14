pub fn run_workspace_search_helper_if_requested() {
    sage_workspace::search::helper_protocol::run_stdio_helper_if_requested();
}

pub fn run_browser_mcp_helper_if_requested() {
    #[cfg(windows)]
    crate::acp::browser::mcp::transport::run_stdio_helper_if_requested();
}
