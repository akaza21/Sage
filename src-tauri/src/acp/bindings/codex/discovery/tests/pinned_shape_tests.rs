//! Account and model fields of the pinned app-server schema.

use super::*;

fn ready() -> CodexDiscoveryProtocol {
    let mut protocol = protocol();
    let account_id = advance_to_account(&mut protocol);
    protocol
        .accept_account(&frame(json!({"id":account_id,"result":{
            "account":{"type":"chatgpt","email":null,"planType":"plus","workspaceName":null},
            "requiresOpenaiAuth":true,"workspaceRouting":null
        }})))
        .unwrap();
    protocol
}

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
fn account_read_still_refuses_unknown_keys() {
    for result in [
        json!({"account":null,"requiresOpenaiAuth":true,"workspace":null}),
        json!({
            "account":{"type":"chatgpt","email":null,"planType":"plus","workspace":null},
            "requiresOpenaiAuth":true
        }),
        json!({"account":null,"requiresOpenaiAuth":true,"workspaceRouting":{"extra":true}}),
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

#[test]
fn model_catalog_validates_the_pinned_model_metadata() {
    let mut annotated = model("gpt-annotated", "Annotated", true);
    annotated["modelSpecialty"] = json!("coding");
    annotated["multiAgentVersion"] = json!("v2");
    annotated["availableAccessPrograms"] = json!({"cyber":["standard"]});
    annotated["upgradeInfo"] = json!({
        "model":"gpt-next","upgradeCopy":null,"modelLink":null,
        "migrationMarkdown":null,"retirementAt":1_800_000_000
    });
    let mut protocol = ready();
    let request = protocol.model_request().unwrap();
    assert!(
        protocol
            .accept_model_page(&frame(json!({"id":request.id(),"result":{
                "data":[annotated],"nextCursor":null
            }})))
            .is_ok()
    );

    for (field, invalid) in [
        ("multiAgentVersion", json!("v3")),
        ("availableAccessPrograms", json!({"cyber":"standard"})),
        (
            "upgradeInfo",
            json!({"model":"gpt-next","upgradeCopy":null,"modelLink":null,"migrationMarkdown":null}),
        ),
    ] {
        let mut entry = model("gpt-invalid", "Invalid", true);
        entry[field] = invalid;
        let mut protocol = ready();
        let request = protocol.model_request().unwrap();
        assert_eq!(
            protocol
                .accept_model_page(&frame(json!({"id":request.id(),"result":{
                    "data":[entry],"nextCursor":null
                }})))
                .unwrap_err(),
            CodexDiscoveryError::ModelCatalog,
            "{field}"
        );
    }
}
