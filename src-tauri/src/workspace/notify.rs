#![allow(dead_code)]

use std::collections::BTreeSet;

use serde_json::Value;

const MAX_IDENTIFIER_BYTES: usize = 512;
const MAX_RESOURCE_PATH_BYTES: usize = 16 * 1024;
const MAX_ROOTS: usize = 8;
const MAX_QUEUE: usize = 64;

pub const WORKSPACE_NOTIFY_EVENT: &str = "workspace-notify-v1";

#[derive(Debug)]
pub struct WorkspaceNotifyError {
    code: &'static str,
}

impl WorkspaceNotifyError {
    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl std::fmt::Display for WorkspaceNotifyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for WorkspaceNotifyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceNotifyNotice {
    pub project_id: String,
    pub project_generation: u64,
    pub transition_epoch: u64,
    pub root_id: String,
    pub relative_path: String,
    pub kind: String,
}

#[derive(Debug)]
pub struct WorkspaceNotifySession {
    project_id: String,
    project_generation: u64,
    transition_epoch: u64,
    roots: BTreeSet<String>,
    queue: Vec<WorkspaceNotifyNotice>,
}

fn reject(code: &'static str) -> WorkspaceNotifyError {
    WorkspaceNotifyError { code }
}

fn is_object(value: &Value) -> bool {
    value.as_object().is_some()
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn bounded_identifier(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0') && value.len() <= MAX_IDENTIFIER_BYTES
}

fn relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RESOURCE_PATH_BYTES
        && !value.contains('\0')
        && !value.contains('\\')
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn safe_counter(value: &Value) -> Option<u64> {
    let number = value.as_i64()?;
    if (0..=9_007_199_254_740_991).contains(&number) {
        Some(number as u64)
    } else {
        None
    }
}

fn stamp(value: &Value) -> Result<(String, u64, u64), WorkspaceNotifyError> {
    if !exact_keys(
        value,
        &["projectId", "projectGeneration", "transitionEpoch"],
    ) {
        return Err(reject("schema-invalid"));
    }
    let project_id = value["projectId"]
        .as_str()
        .filter(|item| bounded_identifier(item))
        .ok_or_else(|| reject("schema-invalid"))?;
    let project_generation =
        safe_counter(&value["projectGeneration"]).ok_or_else(|| reject("schema-invalid"))?;
    let transition_epoch =
        safe_counter(&value["transitionEpoch"]).ok_or_else(|| reject("schema-invalid"))?;
    Ok((project_id.to_string(), project_generation, transition_epoch))
}

fn root_id_value(value: &Value) -> Result<String, WorkspaceNotifyError> {
    if !exact_keys(value, &["rootId"]) {
        return Err(reject("schema-invalid"));
    }
    value["rootId"]
        .as_str()
        .filter(|item| bounded_identifier(item))
        .map(ToOwned::to_owned)
        .ok_or_else(|| reject("schema-invalid"))
}

fn notice(value: &Value) -> Result<WorkspaceNotifyNotice, WorkspaceNotifyError> {
    if !exact_keys(
        value,
        &[
            "projectId",
            "projectGeneration",
            "transitionEpoch",
            "rootId",
            "relativePath",
            "kind",
        ],
    ) {
        return Err(reject("schema-invalid"));
    }
    let project_id = value["projectId"]
        .as_str()
        .filter(|item| bounded_identifier(item))
        .ok_or_else(|| reject("schema-invalid"))?;
    let project_generation =
        safe_counter(&value["projectGeneration"]).ok_or_else(|| reject("schema-invalid"))?;
    let transition_epoch =
        safe_counter(&value["transitionEpoch"]).ok_or_else(|| reject("schema-invalid"))?;
    let root_id = value["rootId"]
        .as_str()
        .filter(|item| bounded_identifier(item))
        .ok_or_else(|| reject("schema-invalid"))?;
    let relative = value["relativePath"]
        .as_str()
        .filter(|item| relative_path(item))
        .ok_or_else(|| reject("schema-invalid"))?;
    let kind = value["kind"]
        .as_str()
        .filter(|item| matches!(*item, "created" | "changed" | "deleted"))
        .ok_or_else(|| reject("schema-invalid"))?;
    Ok(WorkspaceNotifyNotice {
        project_id: project_id.to_string(),
        project_generation,
        transition_epoch,
        root_id: root_id.to_string(),
        relative_path: relative.to_string(),
        kind: kind.to_string(),
    })
}

fn kind_for_action(action: u32) -> Option<&'static str> {
    match action {
        1 | 5 => Some("created"),
        2 | 4 => Some("deleted"),
        3 => Some("changed"),
        _ => None,
    }
}

pub fn workspace_notify_event_name() -> &'static str {
    WORKSPACE_NOTIFY_EVENT
}

pub fn bind_workspace_notify(
    value: &Value,
) -> Result<WorkspaceNotifySession, WorkspaceNotifyError> {
    if !exact_keys(value, &["stamp", "roots"]) {
        return Err(reject("schema-invalid"));
    }
    let (project_id, project_generation, transition_epoch) = stamp(&value["stamp"])?;
    let Some(roots) = value["roots"].as_array() else {
        return Err(reject("schema-invalid"));
    };
    if roots.is_empty() || roots.len() > MAX_ROOTS {
        return Err(reject("schema-invalid"));
    }
    let mut bound = BTreeSet::new();
    for root in roots {
        let root_id = root_id_value(root)?;
        if !bound.insert(root_id) {
            return Err(reject("schema-invalid"));
        }
    }
    Ok(WorkspaceNotifySession {
        project_id,
        project_generation,
        transition_epoch,
        roots: bound,
        queue: Vec::new(),
    })
}

impl WorkspaceNotifySession {
    pub fn classify(
        &self,
        root_id: &str,
        relative: &str,
        action: u32,
    ) -> Result<Option<WorkspaceNotifyNotice>, WorkspaceNotifyError> {
        if !bounded_identifier(root_id) || !relative_path(relative) {
            return Err(reject("schema-invalid"));
        }
        let Some(kind) = kind_for_action(action) else {
            return Ok(None);
        };
        if !self.roots.contains(root_id) {
            return Ok(None);
        }
        Ok(Some(WorkspaceNotifyNotice {
            project_id: self.project_id.clone(),
            project_generation: self.project_generation,
            transition_epoch: self.transition_epoch,
            root_id: root_id.to_string(),
            relative_path: relative.to_string(),
            kind: kind.to_string(),
        }))
    }

    pub fn push(&mut self, value: &Value) -> Result<(), WorkspaceNotifyError> {
        if !is_object(value) {
            return Err(reject("schema-invalid"));
        }
        let parsed = notice(value)?;
        if parsed.project_id != self.project_id
            || parsed.project_generation != self.project_generation
            || parsed.transition_epoch != self.transition_epoch
        {
            return Err(reject("notify-stale"));
        }
        if !self.roots.contains(&parsed.root_id) {
            return Err(reject("notify-substituted"));
        }
        if self.queue.len() >= MAX_QUEUE {
            return Err(reject("notify-overflow"));
        }
        self.queue.push(parsed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn stamp_json() -> Value {
        json!({
            "projectId": "project-a",
            "projectGeneration": 3,
            "transitionEpoch": 7
        })
    }

    fn bound() -> Value {
        json!({
            "stamp": stamp_json(),
            "roots": [{ "rootId": "root-a" }]
        })
    }

    fn notice_json() -> Value {
        json!({
            "projectId": "project-a",
            "projectGeneration": 3,
            "transitionEpoch": 7,
            "rootId": "root-a",
            "relativePath": "src/main.ts",
            "kind": "changed"
        })
    }

    fn bind_ok() -> WorkspaceNotifySession {
        bind_workspace_notify(&bound()).expect("bound session")
    }

    fn rejects_bind(value: Value, code: &str) {
        let error = bind_workspace_notify(&value).expect_err("expected reject");
        assert_eq!(error.code(), code);
    }

    fn rejects_push(session: &mut WorkspaceNotifySession, value: Value, code: &str) {
        let error = session.push(&value).expect_err("expected reject");
        assert_eq!(error.code(), code);
    }

    fn fill_queue(session: &mut WorkspaceNotifySession) {
        for _ in 0..MAX_QUEUE {
            session.push(&notice_json()).expect("queue fill");
        }
    }

    #[test]
    fn bound_notify_bind_returns_a_session() {
        assert!(bind_workspace_notify(&bound()).is_ok());
    }

    #[test]
    fn exactly_eight_unique_roots_bind() {
        let roots: Vec<Value> = (0..8)
            .map(|index| json!({ "rootId": format!("root-{index}") }))
            .collect();
        assert!(bind_workspace_notify(&json!({ "stamp": stamp_json(), "roots": roots })).is_ok());
    }

    #[test]
    fn missing_or_extra_keys_on_bind_stamp_root_and_notice_are_schema_invalid() {
        let mut missing_roots = bound();
        missing_roots.as_object_mut().unwrap().remove("roots");
        rejects_bind(missing_roots, "schema-invalid");
        let mut extra_bind = bound();
        extra_bind
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        rejects_bind(extra_bind, "schema-invalid");
        rejects_bind(json!("nope"), "schema-invalid");

        let mut extra_stamp = stamp_json();
        extra_stamp
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        rejects_bind(
            json!({ "stamp": extra_stamp, "roots": [{ "rootId": "root-a" }] }),
            "schema-invalid",
        );
        let mut missing_stamp = stamp_json();
        missing_stamp.as_object_mut().unwrap().remove("projectId");
        rejects_bind(
            json!({ "stamp": missing_stamp, "roots": [{ "rootId": "root-a" }] }),
            "schema-invalid",
        );
        rejects_bind(
            json!({
                "stamp": { "projectId": 1, "projectGeneration": 3, "transitionEpoch": 7 },
                "roots": [{ "rootId": "root-a" }]
            }),
            "schema-invalid",
        );

        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": [{ "rootId": "root-a", "extra": true }] }),
            "schema-invalid",
        );
        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": [{ }] }),
            "schema-invalid",
        );
        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": [{ "rootId": 1 }] }),
            "schema-invalid",
        );
        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": {} }),
            "schema-invalid",
        );

        let mut session = bind_ok();
        let mut extra_notice = notice_json();
        extra_notice
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        rejects_push(&mut session, extra_notice, "schema-invalid");
        let mut missing_notice = notice_json();
        missing_notice.as_object_mut().unwrap().remove("kind");
        rejects_push(&mut session, missing_notice, "schema-invalid");
        rejects_push(
            &mut session,
            json!({
                "projectId": "project-a",
                "projectGeneration": 3,
                "transitionEpoch": 7,
                "rootId": "root-a",
                "relativePath": "src/main.ts",
                "kind": 1
            }),
            "schema-invalid",
        );
    }

    #[test]
    fn root_count_0_or_9_is_schema_invalid() {
        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": [] }),
            "schema-invalid",
        );
        let roots: Vec<Value> = (0..9)
            .map(|index| json!({ "rootId": format!("root-{index}") }))
            .collect();
        rejects_bind(
            json!({ "stamp": stamp_json(), "roots": roots }),
            "schema-invalid",
        );
    }

    #[test]
    fn duplicate_root_id_is_schema_invalid() {
        rejects_bind(
            json!({
                "stamp": stamp_json(),
                "roots": [{ "rootId": "root-a" }, { "rootId": "root-a" }]
            }),
            "schema-invalid",
        );
    }

    #[test]
    fn empty_nul_or_513_byte_project_id_and_root_id_are_schema_invalid() {
        let long = "a".repeat(513);
        for project_id in ["", "a\0b", long.as_str()] {
            rejects_bind(
                json!({
                    "stamp": {
                        "projectId": project_id,
                        "projectGeneration": 3,
                        "transitionEpoch": 7
                    },
                    "roots": [{ "rootId": "root-a" }]
                }),
                "schema-invalid",
            );
        }
        for root_id in ["", "a\0b", long.as_str()] {
            rejects_bind(
                json!({ "stamp": stamp_json(), "roots": [{ "rootId": root_id }] }),
                "schema-invalid",
            );
        }
    }

    #[test]
    fn generation_or_epoch_minus_1_or_1_5_is_schema_invalid() {
        for value in [json!(-1), json!(1.5)] {
            rejects_bind(
                json!({
                    "stamp": {
                        "projectId": "project-a",
                        "projectGeneration": value,
                        "transitionEpoch": 7
                    },
                    "roots": [{ "rootId": "root-a" }]
                }),
                "schema-invalid",
            );
            rejects_bind(
                json!({
                    "stamp": {
                        "projectId": "project-a",
                        "projectGeneration": 3,
                        "transitionEpoch": value
                    },
                    "roots": [{ "rootId": "root-a" }]
                }),
                "schema-invalid",
            );
        }
    }

    #[test]
    fn classify_added_on_a_bound_root_is_created() {
        let notice = bind_ok()
            .classify("root-a", "src/main.ts", 1)
            .unwrap()
            .unwrap();
        assert_eq!(notice.kind, "created");
    }

    #[test]
    fn classify_removed_on_a_bound_root_is_deleted() {
        let notice = bind_ok()
            .classify("root-a", "src/main.ts", 2)
            .unwrap()
            .unwrap();
        assert_eq!(notice.kind, "deleted");
    }

    #[test]
    fn classify_modified_on_a_bound_root_is_changed() {
        let notice = bind_ok()
            .classify("root-a", "src/main.ts", 3)
            .unwrap()
            .unwrap();
        assert_eq!(notice.kind, "changed");
    }

    #[test]
    fn classify_rename_old_is_deleted_and_rename_new_is_created() {
        let session = bind_ok();
        assert_eq!(
            session
                .classify("root-a", "src/old.ts", 4)
                .unwrap()
                .unwrap()
                .kind,
            "deleted"
        );
        assert_eq!(
            session
                .classify("root-a", "src/new.ts", 5)
                .unwrap()
                .unwrap()
                .kind,
            "created"
        );
    }

    #[test]
    fn classify_unknown_action_is_ignore() {
        assert!(bind_ok()
            .classify("root-a", "src/main.ts", 9)
            .unwrap()
            .is_none());
    }

    #[test]
    fn classify_unbound_root_id_is_ignore() {
        assert!(bind_ok()
            .classify("root-b", "src/main.ts", 3)
            .unwrap()
            .is_none());
    }

    #[test]
    fn classify_invalid_relative_path_is_schema_invalid_on_bound_unbound_and_unknown_action() {
        let session = bind_ok();
        for path in ["src\\main.ts", "/src/main.ts", "src/../x.ts"] {
            assert_eq!(
                session.classify("root-a", path, 3).unwrap_err().code(),
                "schema-invalid"
            );
            assert_eq!(
                session.classify("root-b", path, 3).unwrap_err().code(),
                "schema-invalid"
            );
            assert_eq!(
                session.classify("root-a", path, 9).unwrap_err().code(),
                "schema-invalid"
            );
        }
    }

    #[test]
    fn classify_invalid_root_id_is_schema_invalid_even_when_unbound() {
        let session = bind_ok();
        assert_eq!(
            session.classify("", "src/main.ts", 3).unwrap_err().code(),
            "schema-invalid"
        );
        assert_eq!(
            session
                .classify("a\0b", "src/main.ts", 3)
                .unwrap_err()
                .code(),
            "schema-invalid"
        );
    }

    #[test]
    fn push_stamp_project_id_mismatch_is_notify_stale() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["projectId"] = json!("other");
        rejects_push(&mut session, value, "notify-stale");
    }

    #[test]
    fn push_stamp_generation_mismatch_is_notify_stale() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["projectGeneration"] = json!(4);
        rejects_push(&mut session, value, "notify-stale");
    }

    #[test]
    fn push_stamp_epoch_mismatch_is_notify_stale() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["transitionEpoch"] = json!(8);
        rejects_push(&mut session, value, "notify-stale");
    }

    #[test]
    fn push_unbound_root_id_is_notify_substituted() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["rootId"] = json!("root-b");
        rejects_push(&mut session, value, "notify-substituted");
    }

    #[test]
    fn schema_beats_stale() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["projectGeneration"] = json!(9);
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        rejects_push(&mut session, value, "schema-invalid");
    }

    #[test]
    fn stale_beats_substitution() {
        let mut session = bind_ok();
        let mut value = notice_json();
        value["projectGeneration"] = json!(4);
        value["rootId"] = json!("root-b");
        rejects_push(&mut session, value, "notify-stale");
    }

    #[test]
    fn substitution_beats_overflow() {
        let mut session = bind_ok();
        fill_queue(&mut session);
        let mut value = notice_json();
        value["rootId"] = json!("root-b");
        rejects_push(&mut session, value, "notify-substituted");
    }

    #[test]
    fn sixty_fifth_push_is_notify_overflow() {
        let mut session = bind_ok();
        fill_queue(&mut session);
        rejects_push(&mut session, notice_json(), "notify-overflow");
    }

    #[test]
    fn event_name_is_workspace_notify_v1() {
        assert_eq!(workspace_notify_event_name(), "workspace-notify-v1");
    }

    #[test]
    fn rejects_expose_code_not_message_only() {
        let error = bind_workspace_notify(&json!({})).expect_err("expected reject");
        assert_eq!(error.code(), "schema-invalid");
    }

    #[test]
    fn notify_codes_never_become_a_session() {
        assert!(bind_workspace_notify(&json!({})).is_err());
        let mut session = bind_ok();
        rejects_push(&mut session, json!({}), "schema-invalid");
        let mut stale = notice_json();
        stale["projectId"] = json!("other");
        rejects_push(&mut session, stale, "notify-stale");
        let mut substituted = notice_json();
        substituted["rootId"] = json!("root-b");
        rejects_push(&mut session, substituted, "notify-substituted");
    }
}
