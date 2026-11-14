use serde_json::{Value, json};

use super::profile::{StandardAcpProfile, profile_for};
use super::sanitize::normalized_label_text;
use super::translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator};
use sage_acp_protocol::intent::{
    NormalizedPayload, SafeResourceRef, ToolCategory, UnavailableResourceReason,
};

struct ForeignResources;

impl ResourceMapper for ForeignResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn claude() -> &'static StandardAcpProfile {
    profile_for("claude-code").expect("claude profile")
}

#[test]
fn only_structural_acp_subagent_signals_choose_the_subagent_category() {
    for (signal, expected) in [
        (
            json!({"kind": "subagent", "title": "Delegated work"}),
            ToolCategory::Subagent,
        ),
        (
            json!({"_meta": {"toolName": "Task"}, "title": "Delegated work"}),
            ToolCategory::Subagent,
        ),
        (
            json!({"_meta": {"claudeCode": {"toolName": "Agent"}}, "title": "Delegated work"}),
            ToolCategory::Subagent,
        ),
        (
            json!({"title": "Task Agent delegation"}),
            ToolCategory::Other,
        ),
    ] {
        let mut translator = StandardAcpTranslator::new(claude(), "session-1").unwrap();
        translator.begin_prompt("prompt-1").unwrap();
        let mut update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "tool-1",
            "status": "pending"
        });
        update
            .as_object_mut()
            .unwrap()
            .extend(signal.as_object().unwrap().clone());
        let events = translator.translate_notification_value(
            json!({"method": "session/update", "params": {"sessionId": "session-1", "update": update}}),
            &ForeignResources,
        ).unwrap();
        assert!(events.iter().any(|event| matches!(event,
            AdapterEvent::Item { payload: NormalizedPayload::ToolCall { category, .. }, .. }
            if *category == expected)));
    }
}

fn started(profile: &'static StandardAcpProfile, tool: Value) -> StandardAcpTranslator {
    let mut translator = StandardAcpTranslator::new(profile, "session-1").expect("translator");
    translator.begin_prompt("prompt-1").expect("prompt opens");
    let mut update = json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "tool-1",
        "status": "pending",
    });
    let object = update.as_object_mut().expect("update object");
    for (key, value) in tool.as_object().expect("tool object") {
        object.insert(key.clone(), value.clone());
    }
    let frame = json!({
        "method": "session/update",
        "params": { "sessionId": "session-1", "update": update },
    });
    translator
        .translate_notification_value(frame, &ForeignResources)
        .expect("tool starts");
    translator
}

fn permission_events(translator: &mut StandardAcpTranslator, tool: Value) -> Vec<AdapterEvent> {
    let mut call = json!({ "toolCallId": "tool-1" });
    let object = call.as_object_mut().expect("tool call object");
    for (key, value) in tool.as_object().expect("tool object") {
        object.insert(key.clone(), value.clone());
    }
    let frame = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "session/request_permission",
        "params": {
            "sessionId": "session-1",
            "toolCall": call,
            "options": [{ "optionId": "allow-once", "name": "Yes", "kind": "allow_once" }],
        },
    });
    translator
        .translate_permission_request_value(frame)
        .expect("permission request translates")
}

#[test]
fn reject_only_permission_preserves_offered_option_kinds() {
    let mut translator = started(claude(), json!({"title":"Read page"}));
    let events = translator
        .translate_permission_request_value(json!({
            "jsonrpc":"2.0","id":9,"method":"session/request_permission",
            "params":{"sessionId":"session-1","toolCall":{"toolCallId":"tool-1"},
                "options":[{"optionId":"reject","name":"No","kind":"reject_once"}]}
        }))
        .unwrap();
    let payload = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: payload @ NormalizedPayload::PermissionRequest { .. },
                ..
            } => Some(serde_json::to_value(payload).unwrap()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        payload["payload"]["offeredOptionKinds"],
        json!(["reject_once"])
    );
}

#[test]
fn allow_once_without_usable_option_id_is_not_offered() {
    let mut translator = started(claude(), json!({"title":"Read page"}));
    let events = translator
        .translate_permission_request_value(json!({
            "jsonrpc":"2.0","id":10,"method":"session/request_permission",
            "params":{"sessionId":"session-1","toolCall":{"toolCallId":"tool-1"},
                "options":[{"optionId":"","name":"Yes","kind":"allow_once"},
                    {"optionId":"reject","name":"No","kind":"reject_once"}]}
        }))
        .unwrap();
    let payload = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: payload @ NormalizedPayload::PermissionRequest { .. },
                ..
            } => Some(serde_json::to_value(payload).unwrap()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        payload["payload"]["offeredOptionKinds"],
        json!(["reject_once"])
    );
}

fn permission(translator: &mut StandardAcpTranslator, tool: Value) -> (ToolCategory, String) {
    let events = permission_events(translator, tool);
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::PermissionRequest {
                        category, summary, ..
                    },
                ..
            } => Some((*category, summary.as_str().to_owned())),
            _ => None,
        })
        .expect("one permission request item")
}

fn card_label(events: &[AdapterEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            external_key,
            payload: NormalizedPayload::ToolCall { label, .. },
            ..
        } if external_key.as_str() == "tool-1" => Some(label.as_str().to_owned()),
        _ => None,
    })
}

fn card_input(events: &[AdapterEvent]) -> Option<String> {
    events.iter().find_map(|event| match event {
        AdapterEvent::Item {
            external_key,
            payload: NormalizedPayload::ToolCall { input_summary, .. },
            ..
        } if external_key.as_str() == "tool-1" => input_summary
            .as_ref()
            .map(|value| value.as_str().to_owned()),
        _ => None,
    })
}

const SHELL_COMMAND: &str = "$hr6Deadline=(Get-Date).AddSeconds(120); while (-not (Test-Path -LiteralPath hr6-release.txt) -and (Get-Date) -lt $hr6Deadline) { Start-Sleep -Seconds 2 }";

fn shell_permission(title: &str, command: Value) -> Vec<AdapterEvent> {
    let mut translator = started(claude(), json!({ "title": title }));
    permission_events(
        &mut translator,
        json!({ "title": title, "rawInput": { "command": command } }),
    )
}

fn shell_summary(title: &str, command: Value) -> String {
    let events = shell_permission(title, command);
    summary_of(&events)
}

fn summary_of(events: &[AdapterEvent]) -> String {
    events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::PermissionRequest { summary, .. },
                ..
            } => Some(summary.as_str().to_owned()),
            _ => None,
        })
        .expect("one permission request item")
}

#[test]
fn a_shell_permission_shows_the_exact_command_it_would_run() {
    assert_eq!(
        shell_summary("PowerShell", Value::from(SHELL_COMMAND)),
        SHELL_COMMAND
    );
}

#[test]
fn powershell_identity_is_an_execute_tool_even_without_a_kind() {
    let mut translator = started(claude(), json!({ "title": "PowerShell" }));
    let (category, _) = permission(
        &mut translator,
        json!({ "title": "PowerShell", "rawInput": { "command": "Get-Location" } }),
    );
    assert_eq!(category, ToolCategory::Execute);
}

#[test]
fn harness_refusal_carries_a_visible_reason() {
    let mut translator = started(claude(), json!({ "kind": "execute" }));
    let events = translator
        .translate_prompt_result_value(json!({
            "jsonrpc": "2.0", "id": "prompt-1",
            "result": { "stopReason": "refusal" }
        }))
        .expect("refusal translates");
    assert!(events.iter().any(|event| matches!(event,
        AdapterEvent::Item {
            payload: NormalizedPayload::Error { message, .. }, ..
        } if message.as_str() == "Declined by Claude Code"
    )));
}

#[test]
fn a_described_shell_permission_still_shows_the_command_behind_the_description() {
    assert_eq!(
        shell_summary("Wait for the release file", Value::from(SHELL_COMMAND)),
        SHELL_COMMAND
    );
}

#[test]
fn a_shell_permission_carries_its_command_onto_the_tool_card() {
    let events = shell_permission("PowerShell", Value::from(SHELL_COMMAND));
    assert_eq!(card_input(&events).as_deref(), Some(SHELL_COMMAND));
}

#[test]
fn an_oversized_command_fails_closed_instead_of_showing_a_prefix() {
    let command = "a".repeat(65_537);
    let summary = shell_summary("PowerShell", Value::from(command.clone()));
    assert_eq!(summary, "command too long to display");
    assert!(!command.starts_with(&summary), "{summary}");
}

#[test]
fn a_command_hiding_its_tail_behind_a_null_never_reaches_the_card() {
    let command = format!("Get-Content secrets.txt{}; Remove-Item everything", '\0');
    let summary = shell_summary("PowerShell", Value::from(command));
    assert_eq!(summary, "command cannot be displayed safely");
}

#[test]
fn a_bidi_override_cannot_reorder_the_command_the_card_shows() {
    let command = format!("Remove-Item {}tmp{} keep", '\u{202e}', '\u{202c}');
    assert_eq!(
        shell_summary("PowerShell", Value::from(command)),
        "Remove-Item tmp keep"
    );
}

#[test]
fn permission_command_summaries_redact_sensitive_forms() {
    for (command, private) in [
        ("API_KEY=hunter2", "hunter2"),
        ("MY_SECRET=hunter2", "hunter2"),
        ("--auth-token=abc123", "abc123"),
        ("--password abc123", "abc123"),
        ("-Password abc123", "abc123"),
        ("--pass abc123", "abc123"),
        ("--pw=abc123", "abc123"),
        ("AUTH=Bearer abc123", "abc123"),
        ("Bearer abc123", "abc123"),
        ("Bearer:abc123", "abc123"),
        ("Basic abc123", "abc123"),
        ("Basic:abc123", "abc123"),
        ("mysql -u x -p secret", "secret"),
        ("mysql -psecret", "secret"),
        ("sshpass -p secret", "secret"),
        ("-H 'Authorization: Token abc123'", "abc123"),
        ("Authorization: abc123", "abc123"),
        ("Proxy-Authorization: abc123", "abc123"),
        ("Cookie: abc123", "abc123"),
        ("X-Api-Key: abc123", "abc123"),
        ("X-Auth-Token: abc123", "abc123"),
        ("Authorization: 'abc123'", "abc123"),
        ("X-Api-Key: \"abc123\"", "abc123"),
        ("Authorization : 'abc123'", "abc123"),
        ("mysql -p\"my secret\"", "my secret"),
        ("mysql -pmy\\ secret", "my\\ secret"),
        ("mysql -pmy` secret", "my` secret"),
        ("curl -u user:pass https://example.test", "user:pass"),
        ("curl --user user:pass https://example.test", "user:pass"),
        (
            "curl --proxy-user user:pass https://example.test",
            "user:pass",
        ),
        ("curl -uuser:pass https://example.test", "user:pass"),
        ("curl --user=user:pass https://example.test", "user:pass"),
        ("$env:API_KEY = 'hunter2'", "hunter2"),
        ("export API_KEY = hunter2", "hunter2"),
        (
            "--password 'mixed \"quote\" secret'",
            "mixed \"quote\" secret",
        ),
        ("sk-abc123", "sk-abc123"),
        ("ghp_abc123", "ghp_abc123"),
        ("xoxb-abc123", "xoxb-abc123"),
        ("https://user:pass@example.com/", "user:pass"),
        ("git clone https://tok@host.test/repo", "tok"),
        ("git clone https://x-access-token:abc@host.test/repo", "abc"),
        ("git clone https://oauth2:abc@host.test/repo", "abc"),
        ("--with-token < secret.txt", "secret.txt"),
        ("$env:API_KEY='hunter2'", "hunter2"),
    ] {
        let shown = shell_summary("PowerShell", Value::from(command));
        assert!(!shown.contains(private), "{command}: {shown}");
        assert!(shown.contains("[redacted]"), "{command}: {shown}");
    }
    let shown = shell_summary("PowerShell", Value::from("Get-\u{202e}Content"));
    assert_eq!(shown, "Get-Content");
    let shown = shell_summary("PowerShell", Value::from("Get-\u{2066}Content\u{2069}"));
    assert_eq!(shown, "Get-Content");
}

#[test]
fn embedded_absolute_paths_hide_the_whole_permission_command() {
    for command in [
        "file:///C:/secret",
        "note:/home/secret",
        "mysql -p'unbalanced",
        "cat <<EOF\nhunter2\nEOF",
        "cat <<'EOF'\nhunter2\nEOF",
        "@'hunter2'@",
    ] {
        assert_eq!(
            shell_summary("PowerShell", Value::from(command)),
            "command hidden: contains content Sage will not display; review before approving"
        );
    }
}

#[test]
fn title_only_permission_summaries_share_secret_and_bidi_redaction() {
    for (title, private) in [
        ("API_KEY=hunter2", "hunter2"),
        ("Token=hunter2", "hunter2"),
        ("--password hunter2", "hunter2"),
        ("$env:API_KEY='hunter2'", "hunter2"),
    ] {
        let mut translator = started(claude(), json!({ "kind": "execute" }));
        let events = permission_events(
            &mut translator,
            json!({ "kind": "execute", "title": title }),
        );
        let summary = summary_of(&events);
        assert!(!summary.contains(private), "{title}: {summary}");
        assert!(summary.contains("[redacted]"), "{title}: {summary}");
        assert_eq!(card_label(&events).as_deref(), Some(summary.as_str()));
    }
    let mut translator = started(claude(), json!({ "kind": "execute" }));
    let events = permission_events(
        &mut translator,
        json!({ "kind": "execute", "title": "Get-\u{202e}Content" }),
    );
    assert_eq!(summary_of(&events), "Get-Content");
    assert_eq!(card_label(&events).as_deref(), Some("Get-Content"));
}

#[test]
fn a_permission_without_a_raw_command_keeps_the_title_summary() {
    let mut translator = started(claude(), json!({ "kind": "execute" }));
    let (_, summary) = permission(
        &mut translator,
        json!({ "kind": "execute", "title": "rg --files src", "rawInput": { "file_path": "a" } }),
    );
    assert_eq!(summary, "rg --files src");
}

#[test]
fn every_standard_harness_shows_the_command_and_not_the_tool_name() {
    for harness in ["claude-code", "gemini-cli", "opencode"] {
        let profile = profile_for(harness).expect("profile");
        let mut translator = started(profile, json!({ "title": "PowerShell" }));
        let events = permission_events(
            &mut translator,
            json!({ "title": "PowerShell", "rawInput": { "command": SHELL_COMMAND } }),
        );
        assert_eq!(summary_of(&events), SHELL_COMMAND, "{harness}");
        assert_eq!(
            card_input(&events).as_deref(),
            Some(SHELL_COMMAND),
            "{harness}"
        );
    }
}

#[test]
fn a_permission_title_relabels_the_tool_card_in_the_same_batch() {
    let mut translator = started(claude(), json!({ "kind": "edit" }));
    let events = permission_events(
        &mut translator,
        json!({ "kind": "edit", "title": "Write hr6-permission.txt" }),
    );
    let summary = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::PermissionRequest { summary, .. },
                ..
            } => Some(summary.as_str().to_owned()),
            _ => None,
        })
        .expect("one permission request item");
    assert_eq!(
        card_label(&events).as_deref(),
        Some("Write hr6-permission.txt")
    );
    assert_eq!(summary, "Write hr6-permission.txt");
}

#[test]
fn a_stale_generic_request_title_does_not_clobber_the_specific_card_label() {
    let mut translator = started(
        claude(),
        json!({ "kind": "edit", "title": "Write hr6-permission.txt" }),
    );
    let events = permission_events(
        &mut translator,
        json!({ "kind": "edit", "title": "Edit file" }),
    );
    assert_eq!(card_label(&events), None);
    let summary = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::PermissionRequest { summary, .. },
                ..
            } => Some(summary.as_str().to_owned()),
            _ => None,
        })
        .expect("one permission request item");
    assert_eq!(summary, "Write hr6-permission.txt");
}

#[test]
fn a_permission_without_a_usable_title_leaves_the_tool_card_alone() {
    let mut translator = started(claude(), json!({ "kind": "edit", "title": "Edit a file" }));
    let events = permission_events(&mut translator, json!({ "kind": "edit", "title": "   " }));
    assert_eq!(card_label(&events), None);
}

#[test]
fn a_write_permission_names_the_file_it_would_write() {
    let mut translator = started(claude(), json!({ "kind": "edit" }));
    let (category, summary) = permission(
        &mut translator,
        json!({ "kind": "edit", "title": "Write hr6-permission.txt" }),
    );
    assert_eq!(category, ToolCategory::Edit);
    assert_eq!(summary, "Write hr6-permission.txt");
}

#[test]
fn a_command_permission_names_the_command_it_would_run() {
    let mut translator = started(claude(), json!({ "kind": "execute" }));
    let (category, summary) = permission(
        &mut translator,
        json!({ "kind": "execute", "title": "rg --files src" }),
    );
    assert_eq!(category, ToolCategory::Execute);
    assert_eq!(summary, "rg --files src");
}

#[test]
fn a_permission_without_a_title_reuses_the_title_the_tool_card_already_shows() {
    let mut translator = started(
        claude(),
        json!({ "kind": "edit", "title": "Write hr6-permission.txt" }),
    );
    let (category, summary) = permission(&mut translator, json!({ "kind": "edit" }));
    assert_eq!(category, ToolCategory::Edit);
    assert_eq!(summary, "Write hr6-permission.txt");
}

#[test]
fn a_permission_with_no_title_anywhere_falls_back_to_the_harness_generic_text() {
    for (harness, expected) in [
        (
            "claude-code",
            "Claude tool permission requested; awaiting your decision",
        ),
        (
            "gemini-cli",
            "Gemini tool permission requested; awaiting your decision",
        ),
        (
            "opencode",
            "OpenCode tool permission requested; awaiting your decision",
        ),
    ] {
        let profile = profile_for(harness).expect("profile");
        let mut translator = started(profile, json!({}));
        let (category, summary) = permission(&mut translator, json!({}));
        assert_eq!(category, ToolCategory::Other, "{harness}");
        assert_eq!(summary, expected, "{harness}");
    }
}

#[test]
fn an_unusable_title_degrades_to_the_generic_text_without_failing_the_frame() {
    for title in [
        Value::from("w".repeat(513)),
        Value::from(format!("Write{}hr6.txt", '\0')),
        Value::from("   "),
        Value::from(4_i64),
        Value::Null,
    ] {
        let mut translator = started(claude(), json!({ "kind": "edit" }));
        let (category, summary) =
            permission(&mut translator, json!({ "kind": "edit", "title": title }));
        assert_eq!(category, ToolCategory::Edit, "{title}");
        assert_eq!(
            summary, "Claude tool permission requested; awaiting your decision",
            "{title}"
        );
    }
}

#[test]
fn a_control_char_title_reaches_the_summary_exactly_as_it_reaches_the_tool_card_label() {
    let title = format!("Write{}hr6.txt", '\u{7}');
    let mut translator = started(claude(), json!({ "kind": "edit" }));
    let (_, summary) = permission(
        &mut translator,
        json!({ "kind": "edit", "title": title.clone() }),
    );
    let label = normalized_label_text::<512>("toolLabel", &title).expect("tool card label");
    assert_eq!(summary, label.as_str());
}

#[test]
fn an_absolute_path_in_a_title_is_redacted_exactly_as_the_tool_card_redacts_it() {
    let mut translator = started(claude(), json!({ "kind": "edit" }));
    let (_, summary) = permission(
        &mut translator,
        json!({ "kind": "edit", "title": r"Write C:\secrets\key.txt" }),
    );
    assert!(!summary.contains("secrets"), "{summary}");
    assert!(summary.starts_with("Write "), "{summary}");
}
