use super::*;

#[test]
fn non_native_browse_request_aliases_fail_closed() {
    for tool in ["_sage/browse", "sage.browse"] {
        let mut runtime = active_runtime("client-dynamic-browse-request-alias");
        let request = json!({"id":"browse-request-alias","method":"item/tool/call","params":{
            "threadId":"thread-1","turnId":"turn-1","callId":"browse-call-alias",
            "tool":tool,"arguments":{"url":"http://127.0.0.1:43124/proof"}
        }});
        assert!(matches!(
            runtime.ingest(&serde_json::to_vec(&request).unwrap(), &Mapper),
            Err(AdapterError::Ingress(
                IngressError::UnsupportedServerRequest
            ))
        ));
    }
}

#[test]
fn non_native_browse_item_aliases_fail_closed() {
    for tool in ["_sage/browse", "sage.browse"] {
        let mut runtime = active_runtime("client-dynamic-browse-item-alias");
        let item = json!({"method":"item/started","params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "id":"browse-item-alias","type":"dynamicToolCall","tool":tool,
                "arguments":{"url":"http://127.0.0.1:43124/proof"},"status":"inProgress"
            }
        }});
        assert!(matches!(
            runtime.ingest(&serde_json::to_vec(&item).unwrap(), &Mapper),
            Err(AdapterError::Ingress(
                IngressError::UnsupportedServerRequest
            ))
        ));
    }
}

fn active_runtime(client_id: &str) -> CodexAdapter {
    let mut runtime = adapter();
    ready(&mut runtime);
    arm_turn(&mut runtime, 2, client_id);
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    runtime
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
    runtime
}
