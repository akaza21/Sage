use super::*;

#[test]
fn account_read_admits_workspace_fields_present_or_absent() {
    let routed = json!({
        "account":{"type":"chatgpt","email":null,"planType":"promax","workspaceName":"Research"},
        "requiresOpenaiAuth":true,
        "workspaceRouting":{
            "chatgptAccountId":"account-1",
            "backendOrigin":"https://chatgpt.com",
            "accountRoutingOverride":"us"
        }
    });
    let absent = json!({
        "account":{"type":"chatgpt","email":null,"planType":"plus"},
        "requiresOpenaiAuth":true
    });
    for result in [routed, absent] {
        let mut protocol = protocol();
        let account_id = advance_to_account(&mut protocol);
        assert_eq!(
            protocol
                .accept_account(&frame(json!({"id":account_id,"result":result})))
                .unwrap(),
            CodexAccountSnapshot::ManagedChatgpt
        );
    }
}

#[test]
fn account_read_refuses_a_result_without_the_members_sage_reads() {
    for result in [
        json!({"account":null}),
        json!({"account":null,"requiresOpenaiAuth":"yes"}),
        json!({"requiresOpenaiAuth":true}),
        json!({"account":"chatgpt","requiresOpenaiAuth":true}),
    ] {
        let mut candidate = protocol();
        let account_id = advance_to_account(&mut candidate);
        assert_eq!(
            candidate
                .accept_account(&frame(json!({"id":account_id,"result":result})))
                .unwrap_err(),
            CodexDiscoveryError::Protocol
        );
    }
}
