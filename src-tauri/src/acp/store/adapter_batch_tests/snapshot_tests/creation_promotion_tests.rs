use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use uuid::Uuid;

use super::*;
use crate::acp::{
    bindings::{
        adapter::ResourceMapper as NativeMapping,
        standard_acp::{
            profile_for,
            translator::{
                AdapterEvent as StandardEvent, ResourceMapper as StandardMapper,
                StandardAcpTranslator,
            },
        },
    },
    contract::SageRootId,
    file_diff::{DIFF_SIDE_BYTES_MAX, DIFFS_PER_TURN},
    intent::{SafeResourceRef, UnavailableResourceReason},
    projection::{FileChangeProjection, FilePairState},
    resource_mapper::{ConfiguredResourceRoot, NativeResourceMapper},
};

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-creation-promote-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct NativeRoots(NativeResourceMapper);

impl NativeRoots {
    fn new(root: &Path) -> Self {
        Self(
            NativeResourceMapper::new([ConfiguredResourceRoot {
                root_id: SageRootId::parse("root-one").unwrap(),
                canonical_native_root: root.to_path_buf(),
            }])
            .unwrap(),
        )
    }
}

impl StandardMapper for NativeRoots {
    fn map_absolute(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        NativeMapping::map_existing(&self.0, absolute_path)
    }
}

struct Harness {
    fixture: Fixture,
    translator: StandardAcpTranslator,
    resources: NativeRoots,
    #[allow(
        dead_code,
        reason = "held so the temporary tree lives as long as the harness"
    )]
    tree: TempTree,
    project: PathBuf,
    now: i64,
    deferred: Option<Vec<StandardEvent>>,
}

impl Harness {
    fn new() -> Self {
        let tree = TempTree::new();
        let project = tree.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let project = project.canonicalize().unwrap();
        let resources = NativeRoots::new(&project);
        let mut translator =
            StandardAcpTranslator::new(profile_for("claude-code").unwrap(), "session-1").unwrap();
        translator.begin_prompt("prompt-1").unwrap();
        Self {
            fixture: snapshot_fixture(),
            translator,
            resources,
            tree,
            project,
            now: 20,
            deferred: None,
        }
    }

    fn target(&self, name: &str) -> PathBuf {
        self.project.join(name)
    }

    fn notify(&mut self, update: Value) -> Vec<StandardEvent> {
        let frame = json!({
            "method": "session/update",
            "params": { "sessionId": "session-1", "update": update },
        });
        let events = self
            .translator
            .translate_notification_value(frame, &self.resources)
            .unwrap();
        self.ingest(&events);
        events
    }

    fn start_write(&mut self, tool: &str, target: &Path) -> Vec<StandardEvent> {
        self.notify(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": tool,
            "kind": "edit",
            "status": "pending",
            "content": [{
                "type": "diff",
                "path": target.to_str().unwrap(),
                "newText": "body",
            }],
        }))
    }

    fn settle(&mut self, tool: &str, status: &str) -> Vec<StandardEvent> {
        self.notify(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": tool,
            "status": status,
        }))
    }

    fn ingest(&mut self, events: &[StandardEvent]) {
        if let Some(deferred) = self.deferred.as_mut() {
            deferred.extend(events.iter().cloned());
            return;
        }
        self.now += 1;
        let owner = &self.fixture.owner;
        let mut inbound = Vec::new();
        let mut bytes = 4096;
        for event in events {
            match event {
                StandardEvent::Item {
                    external_key,
                    lifecycle,
                    tool_state,
                    payload,
                } => {
                    let json = serde_json::to_string(payload).unwrap();
                    bytes += json.len();
                    inbound.push(AdapterInboundEvent::Item(ItemIngressCommand {
                        project_id: owner.project_id.clone(),
                        chat_id: owner.chat_id.clone(),
                        session_id: owner.session_id.clone(),
                        turn_id: owner.turn_id.clone(),
                        runtime_epoch: owner.runtime_epoch,
                        external_item_id: external_key.clone(),
                        activity_id: None,
                        payload: payload.clone(),
                        payload_digest: facts::hex_digest(json.as_bytes()),
                        lifecycle: *lifecycle,
                        tool_state: *tool_state,
                        now_millis: self.now,
                    }));
                }
                StandardEvent::FilePair {
                    external_key,
                    old_text,
                    new_text,
                } => {
                    bytes += old_text.len() + new_text.len();
                    inbound.push(AdapterInboundEvent::FilePair(FilePairCommand {
                        project_id: owner.project_id.clone(),
                        chat_id: owner.chat_id.clone(),
                        session_id: owner.session_id.clone(),
                        turn_id: owner.turn_id.clone(),
                        runtime_epoch: owner.runtime_epoch,
                        external_item_id: external_key.clone(),
                        old_text: old_text.clone(),
                        new_text: new_text.clone(),
                        now_millis: self.now,
                    }));
                }
                _ => {}
            }
        }
        if inbound.is_empty() {
            return;
        }
        apply_adapter_inbound_batch(
            &mut self.fixture.connection,
            AdapterInboundBatchCommand::new(owner.clone(), bytes, bytes, inbound).unwrap(),
        )
        .unwrap();
    }

    fn changes(&self) -> Vec<FileChangeProjection> {
        load_chat_page(&self.fixture.connection, project(), chat(), None)
            .unwrap()
            .turns
            .as_slice()
            .iter()
            .flat_map(|turn| turn.items.as_slice())
            .filter_map(|item| match &item.content {
                ItemContent::FileChangeDiff { changes, .. } => Some(changes.as_slice().to_vec()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn only_change(&self) -> FileChangeProjection {
        let changes = self.changes();
        assert_eq!(changes.len(), 1);
        changes.into_iter().next().unwrap()
    }

    fn openable(&self) -> usize {
        self.changes()
            .iter()
            .filter(|change| change.pair_state == FilePairState::Available)
            .count()
    }

    fn coalesce(&mut self) {
        self.deferred = Some(Vec::new());
    }

    fn flush(&mut self) {
        let events = self.deferred.take().unwrap_or_default();
        self.ingest(&events);
    }
}

fn install_terminal_item_triggers(fixture: &Fixture) {
    fixture
        .connection
        .execute_batch(
            "CREATE TRIGGER deny_item_invalid_transition
             BEFORE UPDATE OF lifecycle ON items
             WHEN NOT (
               OLD.lifecycle = NEW.lifecycle OR
               (OLD.lifecycle = 'started' AND NEW.lifecycle IN ('streaming', 'completed', 'failed', 'cancelled', 'unsupported')) OR
               (OLD.lifecycle = 'streaming' AND NEW.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported'))
             )
             BEGIN
               SELECT RAISE(ABORT, 'invalid item lifecycle transition');
             END;
             CREATE TRIGGER deny_terminal_item_mutation
             BEFORE UPDATE ON items
             WHEN OLD.lifecycle IN ('completed', 'failed', 'cancelled', 'unsupported') AND (
               OLD.project_id IS NOT NEW.project_id OR OLD.chat_id IS NOT NEW.chat_id OR
               OLD.session_id IS NOT NEW.session_id OR OLD.turn_id IS NOT NEW.turn_id OR
               OLD.runtime_epoch IS NOT NEW.runtime_epoch OR OLD.ordinal IS NOT NEW.ordinal OR
               OLD.origin IS NOT NEW.origin OR OLD.activity_id IS NOT NEW.activity_id OR
               OLD.kind IS NOT NEW.kind OR OLD.lifecycle IS NOT NEW.lifecycle OR
               OLD.tool_state IS NOT NEW.tool_state OR OLD.external_item_id IS NOT NEW.external_item_id OR
               OLD.payload_json IS NOT NEW.payload_json OR OLD.payload_bytes IS NOT NEW.payload_bytes OR
               OLD.payload_digest IS NOT NEW.payload_digest OR OLD.revision IS NOT NEW.revision OR
               OLD.created_at IS NOT NEW.created_at OR OLD.updated_at IS NOT NEW.updated_at OR
               OLD.terminal_at IS NOT NEW.terminal_at
             )
             BEGIN
               SELECT RAISE(ABORT, 'terminal item is immutable; identical duplicate is a no-op');
             END;",
        )
        .unwrap();
}

#[test]
fn an_approved_write_promotes_without_mutating_an_immutable_terminal_item() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("hr6-permission.txt");
    let path = target.to_str().unwrap().to_owned();
    let tool = "toolu_019P3dyzqj4bj6hZTaR7UrVG";
    let body = "hr6 permission granted\n";

    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call",
        "rawInput": {},
        "status": "pending",
        "title": "Preparing file",
        "kind": "edit",
        "content": [],
        "locations": [],
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "rawInput": {"file_path": path, "content": body},
        "title": "Write hr6-permission.txt",
        "kind": "edit",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
        "locations": [{"path": path}],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    let permission = harness
        .translator
        .translate_permission_request_value(json!({
            "id": 0,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": tool,
                    "name": "Write",
                    "status": "pending",
                    "rawInput": {"file_path": path, "content": body},
                    "title": "Write hr6-permission.txt",
                    "kind": "edit",
                    "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
                    "locations": [{"path": path}],
                },
                "_meta": {"permission": {"version": 1, "title": "Write hr6-permission.txt"}},
                "options": [
                    {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
                    {"optionId": "reject", "name": "No", "kind": "reject_once"},
                ],
            },
        }))
        .unwrap();
    assert!(!permission.is_empty());

    fs::write(&target, body).unwrap();
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "status": "completed",
        "rawOutput": "File created successfully",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "status": "completed",
        "rawOutput": "File created successfully",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
    }));

    let promoted = harness.only_change();
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_late_update_after_settlement_never_admits_a_new_diff_index() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let missing = harness.target("missing.txt");
    let present = harness.target("present.txt");
    fs::write(&present, "body").unwrap();

    harness.start_write("tool-1", &missing);
    harness.settle("tool-1", "completed");
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "completed",
        "content": [
            {"type": "diff", "path": missing.to_str().unwrap(), "newText": "body"},
            {"type": "diff", "path": present.to_str().unwrap(), "newText": "body"},
        ],
    }));
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn the_captured_claude_write_approval_promotes_inside_a_single_coalesced_batch() {
    let mut harness = Harness::new();
    let target = harness.target("hr6-permission.txt");
    let path = target.to_str().unwrap().to_owned();
    let tool = "toolu_01VxPvQHiywMnCJRonXWy9oU";
    let body = "hr6 permission granted\n";
    harness.coalesce();

    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call",
        "rawInput": {},
        "status": "pending",
        "title": "Preparing file",
        "kind": "edit",
        "content": [],
        "locations": [],
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "rawInput": {"file_path": path, "content": body},
        "title": "Write hr6-permission.txt",
        "kind": "edit",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
        "locations": [{"path": path}],
    }));

    fs::write(&target, body).unwrap();
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "status": "completed",
        "rawOutput": "File created successfully",
    }));
    harness.flush();

    let promoted = harness.only_change();
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_creation_verified_at_completion_becomes_openable_in_the_projection() {
    let mut harness = Harness::new();
    let target = harness.target("created.txt");
    harness.start_write("tool-1", &target);
    let pending = harness.only_change();
    assert_eq!(pending.pair_state, FilePairState::Unavailable);
    assert_eq!(
        pending.resource,
        SafeResourceLocation::unavailable(UnavailableResourceReason::Missing)
    );

    fs::write(&target, "body").unwrap();
    harness.settle("tool-1", "completed");
    let promoted = harness.only_change();
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    let SafeResourceLocation::Resource { resource } = &promoted.resource else {
        panic!("a verified creation carries its resource");
    };
    assert_eq!(resource.relative_path.as_str(), "created.txt");
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_failed_tool_never_becomes_openable_even_once_the_file_lands() {
    let mut harness = Harness::new();
    let target = harness.target("created.txt");
    harness.start_write("tool-1", &target);
    fs::write(&target, "body").unwrap();
    harness.settle("tool-1", "failed");
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_cancelled_turn_never_becomes_openable_even_once_the_file_lands() {
    let mut harness = Harness::new();
    let target = harness.target("created.txt");
    harness.start_write("tool-1", &target);
    let terminal = harness
        .translator
        .translate_prompt_result_value(
            json!({"id": "prompt-1", "result": {"stopReason": "cancelled"}}),
        )
        .unwrap();
    harness.ingest(&terminal);
    fs::write(&target, "body").unwrap();
    let late = harness.settle("tool-1", "completed");
    assert!(
        late.iter()
            .all(|event| matches!(event, StandardEvent::IgnoredLate))
    );
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_mapped_diff_past_the_hold_limit_terminalizes_with_its_streamed_resource() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let attempted = DIFFS_PER_TURN as usize + 1;
    let mut targets = Vec::new();
    for index in 0..attempted {
        let target = harness.target(&format!("existing-{index}.txt"));
        fs::write(&target, "one\n").unwrap();
        targets.push(target);
    }
    for (index, target) in targets.iter().enumerate() {
        let opened = harness.notify(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("tool-{index}"),
            "kind": "edit",
            "status": "pending",
            "content": [{
                "type": "diff",
                "path": target.to_str().unwrap(),
                "oldText": "one\n",
                "newText": "two\n",
            }],
        }));
        assert_eq!(diff_lifecycles(&opened), vec!["streaming"]);
    }
    for index in 0..attempted {
        harness.settle(&format!("tool-{index}"), "completed");
    }

    assert_eq!(harness.changes().len(), attempted);
    assert_eq!(harness.openable(), DIFFS_PER_TURN as usize);
    assert_eq!(pair_count(&harness.fixture), DIFFS_PER_TURN);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_vanished_file_keeps_the_resource_identity_its_diff_first_published() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("vanishing.txt");
    fs::write(&target, "one\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let opened = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\n"}],
    }));
    assert_eq!(diff_lifecycles(&opened), vec!["streaming"]);
    let published = harness.only_change().resource;

    fs::remove_file(&target).unwrap();
    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\nthree\n"}],
    }));
    assert_eq!(harness.only_change().resource, published);

    harness.settle("tool-1", "failed");
    let settled = harness.only_change();
    assert_eq!(settled.resource, published);
    assert_eq!(settled.pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn oversized_diffs_consume_the_turn_hold_limit() {
    let mut harness = Harness::new();
    let oversized = "x".repeat(DIFF_SIDE_BYTES_MAX + 1);
    for index in 0..DIFFS_PER_TURN as usize {
        let target = harness.target(&format!("big-{index}.txt"));
        harness.notify(json!({
            "sessionUpdate": "tool_call",
            "toolCallId": format!("tool-big-{index}"),
            "kind": "edit",
            "status": "pending",
            "content": [{
                "type": "diff",
                "path": target.to_str().unwrap(),
                "newText": oversized,
            }],
        }));
    }
    let target = harness.target("small.txt");
    harness.start_write("tool-small", &target);
    fs::write(&target, "body").unwrap();
    harness.settle("tool-small", "completed");

    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[cfg(windows)]
#[test]
fn a_junction_redirect_at_completion_stays_unavailable() {
    let mut harness = Harness::new();
    let outside = harness.tree.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let target = harness.project.join("sub").join("created.txt");
    harness.start_write("tool-1", &target);
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(harness.project.join("sub"))
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(outside.join("created.txt"), "body").unwrap();

    harness.settle("tool-1", "completed");
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn creations_beyond_the_turn_hold_limit_stay_unavailable() {
    let mut harness = Harness::new();
    let held = DIFFS_PER_TURN as usize;
    let attempted = held + 3;
    let mut targets = Vec::new();
    for index in 0..attempted {
        let target = harness.target(&format!("created-{index}.txt"));
        harness.start_write(&format!("tool-{index}"), &target);
        targets.push(target);
    }
    for target in &targets {
        fs::write(target, "body").unwrap();
    }
    for index in 0..attempted {
        harness.settle(&format!("tool-{index}"), "completed");
    }
    assert_eq!(harness.changes().len(), attempted);
    assert_eq!(harness.openable(), held);
    assert_eq!(pair_count(&harness.fixture), held as i64);
    assert_reconciled(&harness.fixture);
}

#[test]
fn the_captured_claude_write_approval_promotes_exactly_one_openable_pair() {
    let mut harness = Harness::new();
    let target = harness.target("hr6-permission.txt");
    let path = target.to_str().unwrap().to_owned();
    let tool = "toolu_01VxPvQHiywMnCJRonXWy9oU";
    let body = "hr6 permission granted\n";

    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call",
        "rawInput": {},
        "status": "pending",
        "title": "Preparing file",
        "kind": "edit",
        "content": [],
        "locations": [],
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "rawInput": {"file_path": path},
        "title": "Write hr6-permission.txt",
        "kind": "edit",
        "locations": [{"path": path}],
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "rawInput": {"file_path": path, "content": body},
        "title": "Write hr6-permission.txt",
        "kind": "edit",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
        "locations": [{"path": path}],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    let permission = harness
       .translator
       .translate_permission_request_value(json!({
            "id": 0,
            "method": "session/request_permission",
            "params": {
                "sessionId": "session-1",
                "toolCall": {
                    "toolCallId": tool,
                    "name": "Write",
                    "status": "pending",
                    "rawInput": {"file_path": path, "content": body},
                    "title": "Write hr6-permission.txt",
                    "kind": "edit",
                    "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
                    "locations": [{"path": path}],
                },
                "_meta": {"permission": {"version": 1, "title": "Write hr6-permission.txt"}},
                "options": [
                    {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
                    {"optionId": "reject", "name": "No", "kind": "reject_once"},
                ],
            },
       }))
       .unwrap();
    assert!(!permission.is_empty());

    fs::write(&target, body).unwrap();
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolResponse": {
            "type": "create",
            "filePath": path,
            "content": body,
            "structuredPatch": [],
            "originalFile": Value::Null,
            "userModified": false,
        }, "toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
    }));
    harness.notify(json!({
        "_meta": {"claudeCode": {"toolName": "Write"}},
        "toolCallId": tool,
        "sessionUpdate": "tool_call_update",
        "status": "completed",
        "rawOutput": "File created successfully",
    }));

    let promoted = harness.only_change();
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn an_oversized_creation_rebound_to_an_existing_path_never_becomes_openable() {
    let mut harness = Harness::new();
    let pending = harness.target("pending.txt");
    let present = harness.target("present.txt");
    fs::write(&present, "body").unwrap();
    let oversized = "x".repeat(DIFF_SIDE_BYTES_MAX + 1);

    harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{
            "type": "diff",
            "path": pending.to_str().unwrap(),
            "newText": oversized,
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": present.to_str().unwrap(),
            "newText": "body",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_creation_past_the_hold_limit_rebound_to_an_existing_path_never_becomes_openable() {
    let mut harness = Harness::new();
    for index in 0..DIFFS_PER_TURN as usize {
        let target = harness.target(&format!("created-{index}.txt"));
        harness.start_write(&format!("tool-{index}"), &target);
    }
    let pending = harness.target("pending.txt");
    let present = harness.target("present.txt");
    fs::write(&present, "body").unwrap();
    harness.start_write("tool-overflow", &pending);

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-overflow",
        "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": present.to_str().unwrap(),
            "newText": "body",
        }],
    }));
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn an_unavailable_update_rebound_to_an_existing_path_never_becomes_openable() {
    let mut harness = Harness::new();
    let pending = harness.target("pending.txt");
    let present = harness.target("present.txt");
    fs::write(&present, "body").unwrap();

    harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{
            "type": "diff",
            "path": pending.to_str().unwrap(),
            "oldText": "before",
            "newText": "after",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": present.to_str().unwrap(),
            "oldText": "before",
            "newText": "after",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_creation_that_failed_verification_never_becomes_openable_on_a_later_frame() {
    let mut harness = Harness::new();
    let target = harness.target("created.txt");
    harness.start_write("tool-1", &target);
    harness.settle("tool-1", "completed");
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    fs::write(&target, "body").unwrap();
    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "completed",
        "content": [{
            "type": "diff",
            "path": target.to_str().unwrap(),
            "newText": "body",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_diff_index_rebound_to_another_path_never_becomes_openable() {
    let mut harness = Harness::new();
    let pending = harness.target("pending.txt");
    let present = harness.target("present.txt");
    fs::write(&present, "body").unwrap();
    harness.start_write("tool-1", &pending);
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": present.to_str().unwrap(),
            "newText": "body",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);

    fs::write(&pending, "body").unwrap();
    harness.settle("tool-1", "completed");
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_held_creation_waits_for_completion_even_once_the_file_lands() {
    let mut harness = Harness::new();
    let target = harness.target("created.txt");
    harness.start_write("tool-1", &target);
    fs::write(&target, "body").unwrap();

    harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{
            "type": "diff",
            "path": target.to_str().unwrap(),
            "newText": "body",
        }],
    }));
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(harness.openable(), 0);
    assert_eq!(pair_count(&harness.fixture), 0);

    harness.settle("tool-1", "completed");
    let promoted = harness.only_change();
    assert_eq!(promoted.pair_state, FilePairState::Available);
    assert_eq!(promoted.operation, FileOperation::Created);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

fn diff_lifecycles(events: &[StandardEvent]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| match event {
            StandardEvent::Item {
                lifecycle,
                payload: crate::acp::intent::NormalizedPayload::FileChangeDiff { .. },
                ..
            } => Some(match lifecycle {
                crate::acp::lifecycle::ItemLifecycle::Streaming => "streaming",
                crate::acp::lifecycle::ItemLifecycle::Completed => "completed",
                _ => "other",
            }),
            _ => None,
        })
        .collect()
}

fn emitted_pairs(events: &[StandardEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, StandardEvent::FilePair { .. }))
        .count()
}

#[test]
fn an_evolving_existing_file_diff_terminalizes_once_when_the_tool_settles() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("evolving.txt");
    fs::write(&target, "one\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let opened = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\n"}],
    }));
    assert_eq!(diff_lifecycles(&opened), vec!["streaming"]);
    assert_eq!(emitted_pairs(&opened), 0);
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);

    let grown = harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\nthree\n"}],
    }));
    assert_eq!(diff_lifecycles(&grown), vec!["streaming"]);
    assert_eq!(emitted_pairs(&grown), 0);
    assert_eq!(pair_count(&harness.fixture), 0);

    let repeated = harness.notify(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "tool-1",
        "status": "in_progress",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\nthree\n"}],
    }));
    assert!(
        repeated
            .iter()
            .all(|event| matches!(event, StandardEvent::IgnoredDuplicate))
    );

    let settled = harness.settle("tool-1", "completed");
    assert_eq!(diff_lifecycles(&settled), vec!["completed"]);
    assert_eq!(emitted_pairs(&settled), 1);

    let change = harness.only_change();
    assert_eq!(change.pair_state, FilePairState::Available);
    assert_eq!(change.operation, FileOperation::Updated);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_creation_completed_in_its_first_frame_promotes_in_that_frame() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("born.txt");
    fs::write(&target, "body\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let frame = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "completed",
        "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": "body\n"}],
    }));
    assert_eq!(diff_lifecycles(&frame), vec!["completed"]);
    assert_eq!(emitted_pairs(&frame), 1);

    let change = harness.only_change();
    assert_eq!(change.pair_state, FilePairState::Available);
    assert_eq!(change.operation, FileOperation::Created);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn an_edit_completed_in_its_first_frame_admits_its_pair_in_that_frame() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("touched.txt");
    fs::write(&target, "after\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let frame = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "completed",
        "content": [{"type": "diff", "path": path, "oldText": "before\n", "newText": "after\n"}],
    }));
    assert_eq!(diff_lifecycles(&frame), vec!["completed"]);
    assert_eq!(emitted_pairs(&frame), 1);

    let change = harness.only_change();
    assert_eq!(change.pair_state, FilePairState::Available);
    assert_eq!(change.operation, FileOperation::Updated);
    assert_eq!(pair_count(&harness.fixture), 1);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_denied_tool_terminalizes_its_mapped_diff_as_unavailable() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("denied.txt");
    fs::write(&target, "one\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let opened = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\n"}],
    }));
    assert_eq!(diff_lifecycles(&opened), vec!["streaming"]);
    assert_eq!(emitted_pairs(&opened), 0);

    let settled = harness.settle("tool-1", "failed");
    assert_eq!(diff_lifecycles(&settled), vec!["completed"]);
    assert_eq!(emitted_pairs(&settled), 0);
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}

#[test]
fn a_cancelled_turn_terminalizes_a_mapped_diff_without_admitting_a_pair() {
    let mut harness = Harness::new();
    install_terminal_item_triggers(&harness.fixture);
    let target = harness.target("cancelled.txt");
    fs::write(&target, "one\n").unwrap();
    let path = target.to_str().unwrap().to_owned();

    let opened = harness.notify(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "kind": "edit",
        "status": "pending",
        "content": [{"type": "diff", "path": path, "oldText": "one\n", "newText": "two\n"}],
    }));
    assert_eq!(diff_lifecycles(&opened), vec!["streaming"]);
    assert_eq!(emitted_pairs(&opened), 0);

    let terminal = harness
        .translator
        .translate_prompt_result_value(
            json!({"id": "prompt-1", "result": {"stopReason": "cancelled"}}),
        )
        .unwrap();
    harness.ingest(&terminal);

    let late = harness.settle("tool-1", "completed");
    assert!(
        late.iter()
            .all(|event| matches!(event, StandardEvent::IgnoredLate))
    );
    assert_eq!(harness.only_change().pair_state, FilePairState::Unavailable);
    assert_eq!(pair_count(&harness.fixture), 0);
    assert_reconciled(&harness.fixture);
}
