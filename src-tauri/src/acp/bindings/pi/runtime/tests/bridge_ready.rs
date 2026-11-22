use super::*;

pub(super) fn announcement() -> Value {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(crate::acp::bindings::pi::bridge::install::SOURCE);
    let digest: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    json!({"type":"extension_ui_request","method":"setStatus","statusKey":"sage.pi.ready.v1",
        "statusText":json!({"version":1,"digest":digest}).to_string()})
}

#[test]
fn missing_or_mismatched_bridge_handshake_fails_closed_before_initialization() {
    for announcement in [
        None,
        Some(json!({"type":"extension_ui_request","method":"setStatus",
        "statusKey":"sage.pi.ready.v1","statusText":"{\"version\":1,\"digest\":\"wrong\"}"})),
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut runtime = fixture_runtime(99);
        runtime.enable_bridge(sage_workspace::Workspace::open("project", "root-pi", root).unwrap());
        if let Some(value) = announcement {
            let error = runtime
                .ingest(&frame(value), &Mapper)
                .err()
                .expect("reject digest");
            assert!(error.to_string().contains("approval bridge"));
        } else {
            let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
            let sent = deliver(&mut runtime, initialize);
            let error = runtime
                .ingest(&response(&sent, json!({})), &Mapper)
                .err()
                .expect("missing handshake");
            assert!(error.to_string().contains("approval bridge"));
        }
        assert!(
            runtime
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "hello").unwrap(),
                    client_message_id: RequiredText::new("client", "blocked").unwrap(),
                    access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
                })
                .is_err()
        );
    }
}
