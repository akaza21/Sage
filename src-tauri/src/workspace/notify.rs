use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

mod batch;
mod relevance;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) mod unix;
#[cfg(any(not(windows), test))]
pub(crate) mod unix_events;
mod watch;
#[cfg(windows)]
mod windows;

pub use watch::relative_from_notify_name;

const MAX_IDENTIFIER_BYTES: usize = 512;
const MAX_RESOURCE_PATH_BYTES: usize = 16 * 1024;
const MAX_ROOTS: usize = 8;
const MAX_WATCHES: usize = 8;
pub(crate) const OVERFLOW_NOTIFY_ACTION: u32 = 8;
pub(crate) const OVERFLOW_RELATIVE_PATH: &str = "lost-notices";
/// Something changed inside the ignored folder the record names.
pub(crate) const IGNORED_BELOW_NOTIFY_ACTION: u32 = 9;

/// Each event carries a [`batch::WorkspaceNotifyBatch`].
pub const WORKSPACE_NOTIFY_EVENT: &str = "workspace-notify-v2";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

fn kind_for_action(action: u32) -> Option<&'static str> {
    match action {
        1 | 5 => Some("created"),
        2 | 4 => Some("deleted"),
        3 => Some("changed"),
        OVERFLOW_NOTIFY_ACTION => Some("resync"),
        IGNORED_BELOW_NOTIFY_ACTION => Some("ignored"),
        _ => None,
    }
}

#[cfg(test)]
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

struct BoundWatch {
    generation: u32,
    session: WorkspaceNotifySession,
    cancelled: Arc<AtomicBool>,
}

struct NotifyRegistry {
    watches: BTreeMap<String, BoundWatch>,
    generations: BTreeMap<String, u32>,
}

fn registry() -> std::sync::MutexGuard<'static, NotifyRegistry> {
    static REGISTRY: OnceLock<Mutex<NotifyRegistry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            Mutex::new(NotifyRegistry {
                watches: BTreeMap::new(),
                generations: BTreeMap::new(),
            })
        })
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn apply_watch_workspace_notify(value: &Value) -> Result<u32, WorkspaceNotifyError> {
    let session = bind_workspace_notify(value)?;
    let project_id = session.project_id.clone();
    let mut registry = registry();
    if registry.watches.len() >= MAX_WATCHES && !registry.watches.contains_key(&project_id) {
        return Err(reject("notify-overflow"));
    }
    let generation = registry
        .generations
        .get(&project_id)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    registry.generations.insert(project_id.clone(), generation);
    if let Some(previous) = registry.watches.remove(&project_id) {
        previous.cancelled.store(true, Ordering::Release);
    }
    registry.watches.insert(
        project_id,
        BoundWatch {
            generation,
            session,
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    );
    Ok(generation)
}

pub fn apply_unwatch_workspace_notify(value: &Value) -> Result<(), WorkspaceNotifyError> {
    if !exact_keys(value, &["projectId"]) {
        return Err(reject("schema-invalid"));
    }
    let project_id = value["projectId"]
        .as_str()
        .filter(|item| bounded_identifier(item))
        .ok_or_else(|| reject("schema-invalid"))?;
    // The Project is closing or being switched away from: free Quick Open's
    // path index with it, even if it was never watched.
    crate::workspace::path_index::forget_project(project_id);
    let mut registry = registry();
    let Some(watch) = registry.watches.remove(project_id) else {
        return Err(reject("notify-substituted"));
    };
    watch.cancelled.store(true, Ordering::Release);
    Ok(())
}

pub fn apply_unwatch_if_generation(
    project_id: &str,
    generation: u32,
) -> Result<(), WorkspaceNotifyError> {
    let mut registry = registry();
    let Some(watch) = registry.watches.get(project_id) else {
        return Err(reject("notify-substituted"));
    };
    if watch.generation != generation {
        return Ok(());
    }
    let watch = registry
        .watches
        .remove(project_id)
        .expect("generation held");
    watch.cancelled.store(true, Ordering::Release);
    Ok(())
}

pub fn drain_queued_workspace_notices(project_id: &str) -> Vec<WorkspaceNotifyNotice> {
    registry()
        .watches
        .get_mut(project_id)
        .map(|watch| std::mem::take(&mut watch.session.queue))
        .unwrap_or_default()
}

fn emit_queued_workspace_notices(app: &AppHandle, bind: &Value) {
    let Some(project_id) = bind
        .get("stamp")
        .and_then(|stamp| stamp.get("projectId"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let notices = drain_queued_workspace_notices(project_id);
    if !notices.is_empty() {
        let _ = app.emit(
            WORKSPACE_NOTIFY_EVENT,
            batch::WorkspaceNotifyBatch { notices },
        );
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceNotifyWatchAck {
    pub generation: u32,
}

#[tauri::command]
pub fn watch_workspace_notify(
    app: AppHandle,
    state: State<'_, AppState>,
    value: Value,
) -> Result<WorkspaceNotifyWatchAck, String> {
    let generation =
        apply_watch_workspace_notify(&value).map_err(|error| error.code().to_string())?;
    let project_id = value
        .get("stamp")
        .and_then(|stamp| stamp.get("projectId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let roots = watch::workspace_notify_bound_roots(&project_id);
    if let Err(error) = roots.as_ref().map_or_else(
        || Err(reject("notify-substituted")),
        |root_ids| {
            watch::start_workspace_notify_watchers(&app, &state, &project_id, generation, root_ids)
        },
    ) {
        let _ = apply_unwatch_if_generation(&project_id, generation);
        return Err(error.code().to_string());
    }
    // A newly watched Project is the active one: Quick Open's index for any
    // other is no longer needed.
    crate::workspace::path_index::focus_project(&project_id);
    emit_queued_workspace_notices(&app, &value);
    Ok(WorkspaceNotifyWatchAck { generation })
}

#[tauri::command]
pub fn unwatch_workspace_notify(value: Value) -> Result<(), String> {
    apply_unwatch_workspace_notify(&value).map_err(|error| error.code().to_string())
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
}

#[cfg(test)]
mod tests {
    use super::watch::{authenticate_workspace_notify_root, deliver_workspace_notify_action};
    use super::*;
    use serde_json::{Value, json};
    use std::sync::{Mutex, OnceLock};

    const MAX_QUEUE: usize = 64;
    fn is_object(value: &Value) -> bool {
        value.as_object().is_some()
    }

    const NOTICE_KINDS: [&str; 5] = ["created", "changed", "deleted", "resync", "ignored"];

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
            .filter(|item| NOTICE_KINDS.contains(item))
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

    impl WorkspaceNotifySession {
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
        let mut missing_stamp_key = bound();
        missing_stamp_key.as_object_mut().unwrap().remove("stamp");
        rejects_bind(missing_stamp_key, "schema-invalid");
        rejects_bind(
            json!({ "stamp": "nope", "roots": [{ "rootId": "root-a" }] }),
            "schema-invalid",
        );

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
        for missing in ["projectId", "projectGeneration", "transitionEpoch"] {
            let mut stamp_value = stamp_json();
            stamp_value.as_object_mut().unwrap().remove(missing);
            rejects_bind(
                json!({ "stamp": stamp_value, "roots": [{ "rootId": "root-a" }] }),
                "schema-invalid",
            );
        }
        for (key, value) in [
            ("projectId", json!(1)),
            ("projectGeneration", json!("3")),
            ("transitionEpoch", json!("7")),
        ] {
            let mut stamp_value = stamp_json();
            stamp_value[key] = value;
            rejects_bind(
                json!({ "stamp": stamp_value, "roots": [{ "rootId": "root-a" }] }),
                "schema-invalid",
            );
        }

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
        for missing in [
            "projectId",
            "projectGeneration",
            "transitionEpoch",
            "rootId",
            "relativePath",
            "kind",
        ] {
            let mut notice_value = notice_json();
            notice_value.as_object_mut().unwrap().remove(missing);
            rejects_push(&mut session, notice_value, "schema-invalid");
        }
        for (key, value) in [
            ("projectId", json!(1)),
            ("projectGeneration", json!("3")),
            ("transitionEpoch", json!("7")),
            ("rootId", json!(1)),
            ("relativePath", json!(1)),
            ("kind", json!(1)),
        ] {
            let mut notice_value = notice_json();
            notice_value[key] = value;
            rejects_push(&mut session, notice_value, "schema-invalid");
        }
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
        assert!(
            bind_ok()
                .classify("root-a", "src/main.ts", 10)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn classify_ignored_below_action_names_the_ignored_folder() {
        let notice = bind_ok()
            .classify("root-a", "target", IGNORED_BELOW_NOTIFY_ACTION)
            .unwrap()
            .unwrap();
        assert_eq!(notice.kind, "ignored");
        assert_eq!(notice.relative_path, "target");
    }

    #[test]
    fn classify_overflow_action_is_resync_at_the_reserved_path() {
        let notice = bind_ok()
            .classify("root-a", OVERFLOW_RELATIVE_PATH, OVERFLOW_NOTIFY_ACTION)
            .unwrap()
            .unwrap();
        assert_eq!(notice.kind, "resync");
        assert_eq!(notice.relative_path, OVERFLOW_RELATIVE_PATH);
        assert_eq!(notice.root_id, "root-a");
    }

    #[test]
    fn classify_unbound_root_id_is_ignore() {
        assert!(
            bind_ok()
                .classify("root-b", "src/main.ts", 3)
                .unwrap()
                .is_none()
        );
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
    fn event_name_is_workspace_notify_v2() {
        assert_eq!(workspace_notify_event_name(), "workspace-notify-v2");
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

    fn with_isolated_watches<T>(body: impl FnOnce() -> T) -> T {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        {
            let mut registry = registry();
            registry.watches.clear();
            registry.generations.clear();
        }
        let result = body();
        {
            let mut registry = registry();
            registry.watches.clear();
            registry.generations.clear();
        }
        result
    }

    fn rejects_watch(value: Value, code: &str) {
        let error = apply_watch_workspace_notify(&value).expect_err("expected watch reject");
        assert_eq!(error.code(), code);
    }

    fn rejects_unwatch(value: Value, code: &str) {
        let error = apply_unwatch_workspace_notify(&value).expect_err("expected unwatch reject");
        assert_eq!(error.code(), code);
    }

    #[test]
    fn watch_bind_returns_generation() {
        with_isolated_watches(|| {
            let generation = apply_watch_workspace_notify(&bound()).expect("watch");
            assert_eq!(generation, 1);
            apply_unwatch_workspace_notify(&json!({ "projectId": "project-a" })).expect("unwatch");
        });
    }

    #[test]
    fn watch_extra_or_missing_bind_keys_are_schema_invalid() {
        with_isolated_watches(|| {
            let mut extra = bound();
            extra
                .as_object_mut()
                .unwrap()
                .insert("extra".into(), json!(true));
            rejects_watch(extra, "schema-invalid");
            rejects_watch(json!({ "stamp": stamp_json() }), "schema-invalid");
            rejects_watch(json!({}), "schema-invalid");
        });
    }

    #[test]
    fn rewatch_replaces_session_and_bumps_generation() {
        with_isolated_watches(|| {
            assert_eq!(apply_watch_workspace_notify(&bound()).expect("first"), 1);
            assert_eq!(apply_watch_workspace_notify(&bound()).expect("second"), 2);
            apply_unwatch_workspace_notify(&json!({ "projectId": "project-a" })).expect("unwatch");
            rejects_unwatch(json!({ "projectId": "project-a" }), "notify-substituted");
        });
    }

    #[test]
    fn unwatch_extra_or_missing_keys_are_schema_invalid() {
        with_isolated_watches(|| {
            apply_watch_workspace_notify(&bound()).expect("watch");
            rejects_unwatch(
                json!({ "projectId": "project-a", "extra": true }),
                "schema-invalid",
            );
            rejects_unwatch(json!({}), "schema-invalid");
            rejects_unwatch(json!({ "projectId": "" }), "schema-invalid");
            apply_unwatch_workspace_notify(&json!({ "projectId": "project-a" })).expect("cleanup");
        });
    }

    #[test]
    fn unwatch_unbound_project_id_is_notify_substituted() {
        with_isolated_watches(|| {
            rejects_unwatch(json!({ "projectId": "project-a" }), "notify-substituted");
        });
    }

    #[test]
    fn ninth_distinct_watch_is_notify_overflow() {
        with_isolated_watches(|| {
            for index in 0..8 {
                let stamp = json!({
                    "projectId": format!("project-{index}"),
                    "projectGeneration": 3,
                    "transitionEpoch": 7
                });
                apply_watch_workspace_notify(&json!({
                    "stamp": stamp,
                    "roots": [{ "rootId": "root-a" }]
                }))
                .expect("watch slot");
            }
            rejects_watch(bound(), "notify-overflow");
        });
    }

    #[test]
    fn drain_after_push_returns_queued_notice() {
        with_isolated_watches(|| {
            apply_watch_workspace_notify(&bound()).expect("watch");
            registry()
                .watches
                .get_mut("project-a")
                .expect("bound")
                .session
                .push(&notice_json())
                .expect("push");
            let notices = drain_queued_workspace_notices("project-a");
            assert_eq!(notices.len(), 1);
            assert_eq!(notices[0].kind, "changed");
            assert_eq!(workspace_notify_event_name(), "workspace-notify-v2");
            assert!(drain_queued_workspace_notices("project-a").is_empty());
        });
    }

    #[test]
    fn unwatch_keeps_generation_so_stale_workers_cannot_rebind() {
        with_isolated_watches(|| {
            assert_eq!(apply_watch_workspace_notify(&bound()).expect("first"), 1);
            assert_eq!(apply_watch_workspace_notify(&bound()).expect("replace"), 2);
            apply_unwatch_workspace_notify(&json!({ "projectId": "project-a" })).expect("unwatch");
            assert_eq!(
                apply_watch_workspace_notify(&bound()).expect("after unwatch"),
                3
            );
        });
    }

    #[test]
    fn notify_name_backslash_becomes_relative_path() {
        assert_eq!(
            relative_from_notify_name("src\\main.ts").expect("normalize"),
            "src/main.ts"
        );
    }

    #[test]
    fn notify_name_traversal_is_schema_invalid() {
        assert_eq!(
            relative_from_notify_name("src\\..\\x.ts")
                .expect_err("traversal")
                .code(),
            "schema-invalid"
        );
    }

    #[test]
    fn foreign_or_missing_root_directory_is_notify_substituted() {
        let missing = std::env::temp_dir().join("sage-notify-missing-root");
        assert_eq!(
            authenticate_workspace_notify_root(
                "project-a",
                "root-a",
                "project-b",
                std::env::temp_dir().as_path(),
            )
            .expect_err("foreign")
            .code(),
            "notify-substituted"
        );
        assert_eq!(
            authenticate_workspace_notify_root("project-a", "root-a", "project-a", &missing)
                .expect_err("missing")
                .code(),
            "notify-substituted"
        );
    }

    #[test]
    fn deliver_after_unwatch_is_notify_substituted() {
        with_isolated_watches(|| {
            let generation = apply_watch_workspace_notify(&bound()).expect("watch");
            apply_unwatch_workspace_notify(&json!({ "projectId": "project-a" })).expect("unwatch");
            assert_eq!(
                deliver_workspace_notify_action(
                    "project-a",
                    generation,
                    "root-a",
                    "src/main.ts",
                    3,
                )
                .expect_err("dropped")
                .code(),
                "notify-substituted"
            );
        });
    }

    #[test]
    fn deliver_stale_generation_is_ignored() {
        with_isolated_watches(|| {
            apply_watch_workspace_notify(&bound()).expect("first");
            let generation = apply_watch_workspace_notify(&bound()).expect("replace");
            assert!(
                deliver_workspace_notify_action(
                    "project-a",
                    generation - 1,
                    "root-a",
                    "src/main.ts",
                    3,
                )
                .expect("stale")
                .is_none()
            );
            let notice = deliver_workspace_notify_action(
                "project-a",
                generation,
                "root-a",
                "src/main.ts",
                1,
            )
            .expect("current")
            .expect("created");
            assert_eq!(notice.kind, "created");
        });
    }

    #[test]
    fn deliver_succeeds_when_push_queue_is_full() {
        with_isolated_watches(|| {
            let generation = apply_watch_workspace_notify(&bound()).expect("watch");
            fill_queue(
                &mut registry()
                    .watches
                    .get_mut("project-a")
                    .expect("bound")
                    .session,
            );
            let notice = deliver_workspace_notify_action(
                "project-a",
                generation,
                "root-a",
                "src/other.ts",
                3,
            )
            .expect("live emit")
            .expect("changed");
            assert_eq!(notice.kind, "changed");
        });
    }

    #[test]
    fn unwatch_if_generation_leaves_a_newer_session() {
        with_isolated_watches(|| {
            apply_watch_workspace_notify(&bound()).expect("first");
            let generation = apply_watch_workspace_notify(&bound()).expect("second");
            apply_unwatch_if_generation("project-a", generation - 1).expect("stale cleanup");
            assert!(
                deliver_workspace_notify_action(
                    "project-a",
                    generation,
                    "root-a",
                    "src/main.ts",
                    3,
                )
                .expect("kept")
                .is_some()
            );
        });
    }
}
