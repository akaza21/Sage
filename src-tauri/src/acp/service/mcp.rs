use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    mcp::inventory::{McpAuthUrl, McpInventory},
};
use sage_acp_protocol::{
    contract::SafeText, scheduler::RuntimeOwner, session_control::SessionControl,
};

impl AcpService {
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

    pub(crate) fn reload_mcp_servers(
        &self,
        owner: RuntimeOwner,
    ) -> Result<McpInventory, SessionControlError> {
        self.change_then_read(
            owner,
            SessionControl::ReloadMcpServers,
            "The MCP configuration was reloaded, but the server list did not arrive in time.",
        )
    }

    pub(crate) fn reconnect_mcp_server(
        &self,
        owner: RuntimeOwner,
        name: sage_acp_protocol::contract::RequiredText<128>,
    ) -> Result<McpInventory, SessionControlError> {
        self.change_then_read(
            owner,
            SessionControl::ReconnectMcpServer { name },
            "The server was reconnected, but the server list did not arrive in time.",
        )
    }

    /// A list that misses its deadline is reported as late, not as the change
    /// failing; a change that misses its deadline is late itself.
    fn change_then_read(
        &self,
        owner: RuntimeOwner,
        control: SessionControl,
        late: &str,
    ) -> Result<McpInventory, SessionControlError> {
        match self.session_control_reply(owner.clone(), control)? {
            NativeRpcReply::Controlled(_) => {
                self.read_mcp_servers(owner).map_err(|error| match error {
                    SessionControlError::Late => {
                        SessionControlError::Refused(SafeText::new("controlSummary", late).ok())
                    }
                    other => other,
                })
            }
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }

    /// Starts the named server's sign-in; the harness answers with where
    /// the user signs in, which only the native side opens.
    pub(crate) fn sign_in_mcp_server(
        &self,
        owner: RuntimeOwner,
        name: sage_acp_protocol::contract::RequiredText<128>,
    ) -> Result<McpAuthUrl, SessionControlError> {
        match self.session_control_reply(owner, SessionControl::SignInMcpServer { name })? {
            NativeRpcReply::McpSignIn(url) => Ok(url),
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }
}
