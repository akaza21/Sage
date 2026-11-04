use super::*;
use crate::acp::browser::{BrowserCommand, BrowserFailure, BrowserRequest, BrowserScope, attachment::BrowserAttachments};
use crate::acp::contract::{SageChatId, SageProjectId};
use rusqlite::Connection;
use serde_json::json;

fn fixture() -> (BrowserAttachments, AttachmentLease, SettingsStore, std::path::PathBuf) {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../work");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("sage-developer-audit-{}.db", uuid::Uuid::new_v4()));
    let settings = SettingsStore::open(&path).unwrap();
    settings.set_browser_developer_mode(true).unwrap();
    let registry = BrowserAttachments::default();
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    registry.attach(project.clone(), chat.clone(), "browser-7", "https://example.com/", 1).unwrap();
    let lease = registry.resolve(&project, &chat).unwrap();
    lease.authority_for(&BrowserScope { session_id: "session-1".into(), tab_id: 7 }).unwrap();
    (registry, lease, settings, path)
}

fn outcomes(path: &std::path::Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection.prepare("SELECT outcome FROM browser_cdp_audit ORDER BY id").unwrap();
    statement.query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn every_grant_end_is_audited_once_and_denials_are_preserved() {
    for end in [
        |registry: &BrowserAttachments, _: &AttachmentLease| { registry.take_over("browser-7"); },
        |registry: &BrowserAttachments, _: &AttachmentLease| { registry.revoke_tab("browser-7"); },
        |registry: &BrowserAttachments, _: &AttachmentLease| { registry.end_developer_mode(); },
        |registry: &BrowserAttachments, _: &AttachmentLease| { registry.navigation_settled("browser-7", "https://other.example/", 2); },
        |registry: &BrowserAttachments, _: &AttachmentLease| { registry.detach(&SageProjectId::parse("project-one").unwrap(), &SageChatId::parse("chat-one").unwrap()); },
        |_: &BrowserAttachments, lease: &AttachmentLease| { lease.authority_for(&BrowserScope { session_id: "session-2".into(), tab_id: 7 }); },
    ] {
        let (registry, lease, settings, path) = fixture();
        let policy = LeaseDeveloperPolicy::new(lease.clone(), settings.clone());
        assert!(policy.grant("session-1", "https://example.com"));
        audit_refused(&settings, &BrowserRequest { scope: BrowserScope { session_id: "session-1".into(), tab_id: 7 }, command: BrowserCommand::Cdp { method: "DOM.getDocument".into(), params: Default::default() } }, BrowserFailure::Denied);
        end(&registry, &lease);
        end(&registry, &lease);
        assert!(!policy.granted("session-1", "https://example.com"));
        assert_eq!(outcomes(&path), ["granted", "denied", "revoked"]);
        drop((policy, registry, lease, settings));
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn an_unwritable_grant_audit_never_creates_authority() {
    let (registry, lease, settings, path) = fixture();
    Connection::open(&path).unwrap().execute_batch("DROP TABLE browser_cdp_audit").unwrap();
    let policy = LeaseDeveloperPolicy::new(lease.clone(), settings.clone());
    assert!(!policy.grant("session-1", "https://example.com"));
    assert!(!lease.developer_granted("session-1", "https://example.com"));
    drop((policy, registry, lease, settings));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn approving_the_same_site_again_cannot_revive_a_revoked_call() {
    let (registry, lease, settings, path) = fixture();
    let old = LeaseDeveloperPolicy::new(lease.clone(), settings.clone());
    assert!(old.grant("session-1", "https://example.com"));
    registry.end_developer_mode();
    let next = LeaseDeveloperPolicy::new(lease.clone(), settings.clone());
    assert!(next.grant("session-1", "https://example.com"));
    assert!(next.enabled());
    assert!(!old.enabled());
    assert!(!old.grant("session-1", "https://example.com"));
    drop((old, next, registry, lease, settings));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn mcp_validation_and_preflight_denials_reach_durable_audit() {
    use crate::acp::browser::{BrowserCapability, BrowserResult, mcp::BrowserMcpServer};
    struct AuditPort(SettingsStore);
    impl BrowserCapability for AuditPort {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> { Err(BrowserFailure::Denied) }
        fn refused(&mut self, request: &BrowserRequest, failure: BrowserFailure) { audit_refused(&self.0, request, failure); }
    }
    let (registry, lease, settings, path) = fixture();
    let (mut server, token) = BrowserMcpServer::issue(AuditPort(settings.clone()), BrowserScope { session_id: "session-1".into(), tab_id: 7 }, 1000);
    server.connect(&token, "session-1", 1001).unwrap();
    for arguments in [
        json!({"method":"malformed-secret", "params":{}}),
        json!({"method":"Runtime.evaluate", "params":{"expression":"x".repeat(13 * 1024)}}),
        json!({"method":"Runtime.evaluate", "params":[]}),
        json!({"method":"Runtime.evaluate", "scope":{"sessionId":"foreign-secret"}}),
    ] {
        let request = serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":"cdp", "arguments":arguments}})).unwrap();
        let result = server.handle(&request).unwrap();
        let result: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert!(result.get("error").is_some() || result["result"]["isError"] == true);
    }
    assert_eq!(outcomes(&path).len(), 4);
    let connection = Connection::open(&path).unwrap();
    let leaked: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM browser_cdp_audit WHERE method LIKE '%secret%' OR session_id LIKE '%secret%' OR params_preview != '[omitted]')", [], |row| row.get(0)).unwrap();
    assert!(!leaked);
    drop((connection, server, registry, lease, settings));
    std::fs::remove_file(path).unwrap();
}
