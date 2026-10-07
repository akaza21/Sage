use super::*;
use crate::acp::bindings::claude::runtime::tests::initialize;

#[test]
fn browser_mcp_descriptor_is_session_scoped_for_new_and_resume() {
    let admission = McpAdmission::browser_stdio(
        r"C:\Sage\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe-name".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "1234".into()),
            (
                "SAGE_BROWSER_MCP_SESSION".into(),
                "session-claude-runtime".into(),
            ),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "one-use-token".into()),
        ],
    )
    .expect("browser admission");
    let expected = json!([{
        "name":"sage-browser",
        "command":r"C:\Sage\sage.exe",
        "args":["--sage-browser-mcp-stdio"],
        "env":[
            {"name":"SAGE_BROWSER_MCP_PIPE","value":"pipe-name"},
            {"name":"SAGE_BROWSER_MCP_HOST_PID","value":"1234"},
            {"name":"SAGE_BROWSER_MCP_SESSION","value":"session-claude-runtime"},
            {"name":"SAGE_BROWSER_MCP_TOKEN","value":"one-use-token"},
        ],
    }]);
    for resume in [false, true] {
        let mut runtime = ClaudeRuntime::fixture();
        runtime.mcp = admission.clone();
        initialize(&mut runtime);
        let egress = if resume {
            runtime.command(AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("external", "claude-session-1").unwrap(),
                native_root: r"C:\fixture\root".into(),
                model: None,
            })
        } else {
            runtime.command(AdapterCommand::NewSession {
                native_root: r"C:\fixture\root".into(),
                model: None,
            })
        }
        .expect("session command");
        let value: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        assert_eq!(
            value["method"],
            if resume {
                "session/resume"
            } else {
                "session/new"
            }
        );
        assert_eq!(value["params"]["mcpServers"], expected);
        assert!(
            !value["params"]["mcpServers"][0]["args"]
                .to_string()
                .contains("one-use-token")
        );
    }
}
