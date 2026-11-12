//! `/mcp`: the MCP servers of a chat's running session, as its harness
//! reports them, and the two things the user can ask of them.

use super::*;
use crate::acp::{
    contract::SafeText,
    goal::NativeRpcReply,
    mcp::inventory::{McpAuthUrl, McpInventory},
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

impl AcpService {
    /// The session's MCP servers as its harness reports them now.
    pub(crate) fn read_mcp_servers(
        &self,
        owner: RuntimeOwner,
    ) -> Result<McpInventory, SessionControlError> {
        match self.session_control_reply(owner, SessionControl::ReadMcpServers)? {
            NativeRpcReply::McpServers(inventory) => Ok(inventory),
            NativeRpcReply::TooLarge => Err(SessionControlError::Refused(
                SafeText::new(
                    "controlSummary",
                    "The harness reported more MCP server detail than Sage can show.",
                )
                .ok(),
            )),
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }

    /// Has the harness read its MCP configuration again, then lists the
    /// servers as they stand after it.
    pub(crate) fn reload_mcp_servers(
        &self,
        owner: RuntimeOwner,
    ) -> Result<McpInventory, SessionControlError> {
        match self.session_control_reply(owner.clone(), SessionControl::ReloadMcpServers)? {
            NativeRpcReply::Controlled(_) => self.read_mcp_servers(owner),
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }

    /// Starts the named server's sign-in; the harness answers with where
    /// the user signs in, which only the native side opens.
    pub(crate) fn sign_in_mcp_server(
        &self,
        owner: RuntimeOwner,
        name: crate::acp::contract::RequiredText<128>,
    ) -> Result<McpAuthUrl, SessionControlError> {
        match self.session_control_reply(owner, SessionControl::SignInMcpServer { name })? {
            NativeRpcReply::McpSignIn(url) => Ok(url),
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }
}
