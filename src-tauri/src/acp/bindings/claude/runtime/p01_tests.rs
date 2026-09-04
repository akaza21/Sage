use super::*;

#[test]
fn nonconformant_browse_method_aliases_fail_closed() {
    for method in ["sage.browse", "sage_browse"] {
        let mut runtime = ClaudeRuntime::fixture();
        ready(&mut runtime);
        let _ = prompt(&mut runtime);
        assert!(matches!(
            runtime.ingest(
                &frame(json!({
                    "jsonrpc":"2.0","id":"legacy-browse","method":method,
                    "params":{"sessionId":"claude-session-1","url":"http://127.0.0.1:5173/"}
                })),
                &Mapper,
            ),
            Err(AdapterRuntimeError::MandatoryDenyTeardown)
        ));
    }
}
