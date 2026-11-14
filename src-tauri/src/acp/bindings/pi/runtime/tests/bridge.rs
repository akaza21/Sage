use super::*;
use crate::acp::bindings::pi::bridge::Refusal;
use crate::acp_protocol::{access_mode::SessionAccessMode, permissions::PermissionDecision};
use std::path::PathBuf;
#[cfg(feature = "acp-fixture")]
#[path = "bridge_fixture.rs"]
mod fixture_cases;
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
        Self::with_bridge(mode, true)
    }
    fn with_bridge(mode: SessionAccessMode, bridge: bool) -> Self {
        let root = std::env::temp_dir().join(format!("sage-pi-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let mut runtime = fixture_runtime(99);
        if bridge {
            runtime.enable_bridge(
                crate::workspace::Workspace::open("project", "root-pi", &root).unwrap(),
            );
        }
        ready(&mut runtime);
        let mut journey = Self { runtime, root };
        journey.turn("bridge-turn", mode);
        journey
    }
    fn turn(&mut self, client: &str, mode: SessionAccessMode) {
        let egress = self
            .runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "write").unwrap(),
                client_message_id: RequiredText::new("client", client).unwrap(),
                access_mode: mode,
            })
            .unwrap();
        let sent = deliver(&mut self.runtime, egress);
        self.runtime
            .ingest(&response(&sent, Value::Null), &Mapper)
            .unwrap();
        self.runtime
            .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
            .unwrap();
    }
    fn settle(&mut self) {
        self.runtime
            .ingest(&frame(json!({"type":"agent_end","messages":[{"role":"assistant","content":[],"stopReason":"stop","usage":{"input":2,"output":1}}],"willRetry":false})), &Mapper)
            .unwrap();
        let batch = self
            .runtime
            .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
            .unwrap();
        assert!(
            batch.events().iter().any(
                |event| matches!(event, AdapterEvent::Turn { state, .. } if state.is_terminal())
            )
        );
    }
    fn prepare(&mut self, tool: &str, args: Value) -> AdapterBatch {
        self.prepare_as("tool-1", "prepare-1", tool, args)
    }
    fn prepare_as(&mut self, tool_id: &str, id: &str, tool: &str, args: Value) -> AdapterBatch {
        self.runtime.ingest(&frame(json!({"type":"tool_execution_start","toolCallId":tool_id,"toolName":tool,"args":args})), &Mapper).unwrap();
        self.dialog(
            id,
            "sage.pi.prepare.v1",
            &json!({"version":1,"toolCallId":tool_id,"tool":tool,"args":args}).to_string(),
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
    assert_eq!(
        request(&replay.egress()[0])["value"],
        Refusal::NotApproved.message()
    );
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}
#[test]
fn pi_bridge_journey_read_only_denies_edits_and_commands_without_side_effects() {
    for (mode, tool, args, refusal) in [
        (
            SessionAccessMode::ReadOnly,
            "write",
            json!({"path":"a.txt","content":"bad"}),
            Refusal::ReadOnly,
        ),
        (
            SessionAccessMode::ReadOnly,
            "bash",
            json!({"command":"touch a.txt"}),
            Refusal::ReadOnly,
        ),
    ] {
        let mut j = Journey::new(mode);
        let batch = j.prepare(tool, args);
        assert!(batch.events().is_empty());
        assert_eq!(request(&batch.egress()[0])["value"], refusal.message());
        assert!(!j.root.join("a.txt").exists());
    }
}
#[test]
fn pi_bridge_journey_conflict_and_unwritten_approval_deny_commit() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    let early = j.dialog("commit-early", "sage.pi.commit.v1", "tool-1");
    assert_eq!(
        request(&early.egress()[0])["value"],
        Refusal::NotApproved.message()
    );
    assert!(!j.root.join("a.txt").exists());
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.allow();
    std::fs::write(j.root.join("a.txt"), "user").unwrap();
    let conflict = j.dialog("commit-conflict", "sage.pi.commit.v1", "tool-1");
    assert_eq!(
        request(&conflict.egress()[0])["value"],
        Refusal::Changed.message()
    );
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
    assert_eq!(
        deliver(&mut j.runtime, deny)["value"],
        Refusal::Denied.message()
    );
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
    assert!(!j.root.join("a.txt").exists());
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.runtime.command(AdapterCommand::Cancel).unwrap();
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}

#[test]
fn pi_bridge_write_authority_ends_with_the_turn_that_granted_it() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.allow();
    j.settle();
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
    let stale = j.dialog("commit-stale", "sage.pi.commit.v1", "tool-1");
    assert_eq!(
        request(&stale.egress()[0])["value"],
        Refusal::Inactive.message()
    );

    j.turn("read-only-turn", SessionAccessMode::ReadOnly);
    let batch = j.prepare_as(
        "tool-2",
        "prepare-2",
        "write",
        json!({"path":"a.txt","content":"later"}),
    );
    assert!(batch.events().is_empty());
    assert_eq!(
        request(&batch.egress()[0])["value"],
        Refusal::ReadOnly.message()
    );
    assert!(!j.root.join("a.txt").exists());
}

#[test]
fn pi_without_admitted_bridge_refuses_write_dialogs_even_in_write_mode() {
    let mut j = Journey::with_bridge(SessionAccessMode::WorkspaceWrite, false);
    let body = json!({"version":1,"toolCallId":"tool-1","tool":"write","args":{"path":"a.txt","content":"agent"}});
    let batch = j.dialog("prepare-1", "sage.pi.prepare.v1", &body.to_string());
    assert!(batch.events().is_empty());
    assert_eq!(
        request(&batch.egress()[0])["value"],
        Refusal::Unsupported.message()
    );
    let commit = j.dialog("commit-1", "sage.pi.commit.v1", "tool-1");
    assert_eq!(
        request(&commit.egress()[0])["value"],
        Refusal::Unsupported.message()
    );
    assert!(!j.root.join("a.txt").exists());
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

#[test]
fn pi_bridge_refuses_protected_aliases_and_paths_the_prompt_cannot_show() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    std::fs::create_dir(j.root.join(".git")).unwrap();
    std::fs::write(j.root.join(".git").join("config"), "[core]\n").unwrap();
    let long = format!("{}.txt", "d".repeat(480));
    for (index, path) in [
        ".GIT/config",
        ".Git/hooks/pre-commit",
        "GIT~1/config",
        ".git./config",
        "NODE_M~1/pkg.js",
        "notes. ",
        "line\nbreak.txt",
        "right\u{202e}txt.exe",
        long.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        let batch = j.prepare_as(
            &format!("tool-{index}"),
            &format!("prepare-{index}"),
            "write",
            json!({"path":path,"content":"[core]\n\tfsmonitor = calc\n"}),
        );
        assert!(batch.events().is_empty(), "{path:?}");
        assert_eq!(
            request(&batch.egress()[0])["value"],
            Refusal::ProtectedPath.message(),
            "{path:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(j.root.join(".git").join("config")).unwrap(),
        "[core]\n"
    );
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
}

#[test]
fn pi_bridge_expired_prompt_is_withdrawn_durably_and_frees_the_turn() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"first"}));
    let withdrawn = j.dialog("withdraw-1", "sage.pi.withdraw.v1", "tool-1");
    assert!(matches!(
        withdrawn.events(),
        [AdapterEvent::PermissionResolved { external_request_id, state, .. }]
            if external_request_id.as_str() == "prepare-1"
                && *state == crate::acp::lifecycle::ServerRequestState::Cancelled
    ));
    assert_eq!(
        request(&withdrawn.egress()[0])["value"],
        Refusal::Withdrawn.message()
    );
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
    assert!(
        j.runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse("prepare-1").unwrap(),
                decision: PermissionDecision::AllowOnce,
            })
            .is_err()
    );
    let next = j.prepare_as(
        "tool-2",
        "prepare-2",
        "write",
        json!({"path":"a.txt","content":"second"}),
    );
    assert!(matches!(
        next.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
    assert!(!j.root.join("a.txt").exists());
}

#[test]
fn pi_bridge_approval_recorded_before_expiry_still_commits() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    j.prepare("write", json!({"path":"a.txt","content":"agent"}));
    j.allow();
    let settled = j.dialog("withdraw-1", "sage.pi.withdraw.v1", "tool-1");
    assert!(settled.events().is_empty());
    assert_eq!(request(&settled.egress()[0])["value"], "approved");
    let commit = j.dialog("commit-1", "sage.pi.commit.v1", "tool-1");
    assert_eq!(request(&commit.egress()[0])["value"], "written");
    assert_eq!(
        std::fs::read_to_string(j.root.join("a.txt")).unwrap(),
        "agent"
    );
}
#[test]
fn pi_bridge_journey_shell_command_asks_and_its_approval_frees_the_slot() {
    let mut j = Journey::new(SessionAccessMode::WorkspaceWrite);
    let batch = j.prepare("bash", json!({"command":"cargo test"}));
    let [AdapterEvent::PermissionRequest { payload, .. }] = batch.events() else {
        panic!("a command did not ask: {:?}", batch.events());
    };
    let NormalizedPayload::PermissionRequest {
        category, summary, ..
    } = payload
    else {
        panic!("not a permission payload");
    };
    assert_eq!(*category, crate::acp::intent::ToolCategory::Execute);
    assert!(
        summary.as_str().ends_with("\ncargo test"),
        "{}",
        summary.as_str()
    );
    j.allow();
    assert!(j.runtime.bridge.as_ref().unwrap().pending.is_none());
    let commit = j.dialog("commit-1", "sage.pi.commit.v1", "tool-1");
    assert_eq!(
        request(&commit.egress()[0])["value"],
        Refusal::NotApproved.message()
    );
    let replay = j.prepare_as(
        "tool-1",
        "prepare-2",
        "bash",
        json!({"command":"cargo test"}),
    );
    assert_eq!(
        request(&replay.egress()[0])["value"],
        Refusal::Replayed.message()
    );
    let next = j.prepare_as("tool-2", "prepare-3", "bash", json!({"command":"ls"}));
    assert!(matches!(
        next.events(),
        [AdapterEvent::PermissionRequest { .. }]
    ));
}
