use super::*;
use crate::{acp::runtime::permissions::PermissionDecision, settings_store::SessionAccessMode};
use std::path::PathBuf;
struct Journey {
    runtime: PiRuntime,
    root: PathBuf,
}
impl Drop for Journey {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct FileMapper(PathBuf);
impl ResourceMapper for FileMapper {
    fn map_existing(
        &self,
        path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        let path = std::path::Path::new(path)
            .strip_prefix(&self.0)
            .map_err(|_| crate::acp::intent::UnavailableResourceReason::Unsupported)?;
        SafeResourceRef::new(
            SageRootId::parse("root-pi").unwrap(),
            path.to_str().unwrap(),
        )
        .map_err(|_| crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}
impl Journey {
    fn new(mode: SessionAccessMode) -> Self {
        let root = std::env::temp_dir().join(format!("sage-pi-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut runtime = fixture_runtime(99);
        runtime
            .enable_bridge(crate::workspace::Workspace::open("project", "root-pi", &root).unwrap());
        ready(&mut runtime);
        let egress = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "write").unwrap(),
                client_message_id: RequiredText::new("client", "bridge-turn").unwrap(),
                access_mode: mode,
            })
            .unwrap();
        let sent = deliver(&mut runtime, egress);
        runtime
            .ingest(&response(&sent, Value::Null), &Mapper)
            .unwrap();
        runtime
            .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
            .unwrap();
        Self { runtime, root }
    }
    fn prepare(&mut self, tool: &str, args: Value) -> AdapterBatch {
        self.runtime.ingest(&frame(json!({"type":"tool_execution_start","toolCallId":"tool-1","toolName":tool,"args":args})), &Mapper).unwrap();
        self.dialog(
            "prepare-1",
            "sage.pi.prepare.v1",
            &json!({"version":1,"toolCallId":"tool-1","tool":tool,"args":args}).to_string(),
        )
    }
    fn dialog(&mut self, id: &str, title: &str, placeholder: &str) -> AdapterBatch {
        self.runtime.ingest(&frame(json!({"type":"extension_ui_request", "method":"input","id":id,"title":title,"placeholder":placeholder})), &FileMapper(self.root.clone())).unwrap()
    }
    fn allow(&mut self) {
        let egress = self
            .runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse("prepare-1").unwrap(),
                decision: PermissionDecision::AllowOnce,
            })
            .unwrap();
        assert_eq!(deliver(&mut self.runtime, egress)["value"], "approved");
    }
}
#[test]
fn pi_bridge_journey_asked_allowed_and_single_use_write_emits_saved_pair() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    let batch = j.prepare("write", json!({"path":"a.txt","content":"new content"}));
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    assert!(!j.root.join("a.txt").exists());
    j.allow();
    assert!(!j.root.join("a.txt").exists());
    let batch = j.dialog("commit-1", "sage.pi.commit.v1", "tool-1");
    assert!(
        matches!(batch.events(), [AdapterEvent::Item { .. }, AdapterEvent::FilePair { old_text, new_text, .. }] if old_text.is_empty() && new_text == "new content")
    );
    assert_eq!(request(&batch.egress()[0])["value"], "written");
    assert_eq!(
        std::fs::read_to_string(j.root.join("a.txt")).unwrap(),
        "new content"
    );
    let replay = j.dialog("commit-2", "sage.pi.commit.v1", "tool-1");
    assert_eq!(request(&replay.egress()[0])["value"], "denied");
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}
#[test]
fn pi_bridge_journey_read_only_and_shell_are_denied_without_side_effects() {
    for (mode, tool, args) in [
        (
            SessionAccessMode::ReadOnly,
            "write",
            json!({"path":"a.txt","content":"bad"}),
        ),
        (
            SessionAccessMode::WorkspaceWrite,
            "bash",
            json!({"command":"touch a.txt"}),
        ),
    ] {
        let mut j = Journey::new(mode);
        let batch = j.prepare(tool, args);
        assert!(batch.events().is_empty());
        assert_eq!(request(&batch.egress()[0])["value"], "denied");
        assert!(!j.root.join("a.txt").exists());
    }
}
#[test]
fn pi_bridge_journey_conflict_and_unwritten_approval_deny_commit() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    let early = j.dialog("commit-early", "sage.pi.commit.v1", "tool-1");
    assert_eq!(request(&early.egress()[0])["value"], "denied");
    assert!(!j.root.join("a.txt").exists());
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.allow();
    std::fs::write(j.root.join("a.txt"), "user").unwrap();
    let conflict = j.dialog("commit-conflict", "sage.pi.commit.v1", "tool-1");
    assert_eq!(request(&conflict.egress()[0])["value"], "denied");
    assert_eq!(
        std::fs::read_to_string(j.root.join("a.txt")).unwrap(),
        "user"
    );
}
#[test]
fn pi_bridge_journey_denied_permission_and_cancel_clear_pending_bytes() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    let deny = j
        .runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("prepare-1").unwrap(),
            decision: PermissionDecision::Deny,
        })
        .unwrap();
    j.runtime.commit_mandatory_egress(deny.token()).unwrap();
    assert_eq!(deliver(&mut j.runtime, deny)["value"], "denied");
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
    assert!(!j.root.join("a.txt").exists());
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.runtime.command(AdapterCommand::Cancel).unwrap();
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}

#[test]
fn pi_bridge_rejects_sensitive_and_oversized_dialog_ids() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    for id in [
        "".to_owned(),
        "x".repeat(513),
        "/private/identity".to_owned(),
    ] {
        let result = j.runtime.ingest(&frame(json!({"type":"extension_ui_request", "method":"input","id":id,"title":"sage.pi.prepare.v1","placeholder":"{}"})), &Mapper);
        assert!(result.is_err());
    }
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}
