use super::*;
use crate::acp::{
    bindings::pi::{ModelSummary, PiLaunch, title_launch::TitleLaunch},
    process::LaunchSpec,
    service::chat_title::{
        TitleAdmission, TitleScheduler,
        tests::{next_completed, scheduler, shown},
    },
};
use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

struct FixtureAuthority;
impl PiLaunch for FixtureAuthority {
    fn launch_spec(&mut self, _: RuntimeEpoch) -> Result<LaunchSpec, AdapterRuntimeError> {
        Err(AdapterRuntimeError::LaunchUnavailable)
    }
    fn revalidate_title(&self) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../work")
            .join(format!("pi-title-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn runtime(&self, scenario: &str, intent: &crate::acp::runtime::PumpIntent) -> PiRuntime {
        let mut command = Command::new("node");
        command
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/acp/bindings/pi/title_request/fixture.mjs"
            ))
            .arg(scenario)
            .args(crate::acp::bindings::pi::title_request::TITLE_ARGS)
            .env("TITLE_FIXTURE_DIR", &self.0);
        for key in ["PATH", "SystemRoot"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut runtime = PiRuntime::fixture(intent.runtime_epoch, intent.session_id.clone())
            .with_launch(FixtureAuthority);
        runtime.title_launch = Some(TitleLaunch::fixture(command));
        runtime.available_models = vec![ModelSummary {
            provider: "fixture".into(),
            id: "same-model".into(),
            name: "Fixture".into(),
            context_window: None,
        }];
        runtime.current_model = Some("fixture:same-model".into());
        runtime
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < end, "bounded Pi operation did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn finish(titles: &mut TitleScheduler, runtime: &mut PiRuntime) {
    wait_until(|| {
        assert!(
            titles.poll(runtime).is_none(),
            "Pi titles use their isolated process"
        );
        !titles.pending()
    });
}

#[test]
fn pi_busy_completion_stays_pending_until_the_next_completed_turn() {
    let _process = crate::acp::bindings::pi::title_request::tests::PROCESS_LOCK
        .lock()
        .unwrap();
    let directory = Directory::new();
    let (db, intent) = crate::acp::store::title_fixture();
    let admission = Arc::new(TitleAdmission::default());
    let mut titles = scheduler(&db, &admission);
    let mut runtime = directory.runtime("success", &intent);
    let foreground = titles.foreground_turn();
    titles.completed(&intent, &mut runtime);
    assert!(!titles.pending());
    assert_eq!(shown(&db), "Chat: Prompt fallback");
    drop(foreground);
    titles.completed(&next_completed(&db, &intent), &mut runtime);
    assert!(titles.pending(), "the isolated generation was admitted");
    finish(&mut titles, &mut runtime);
    assert_eq!(shown(&db), "Chat: Check Sage chat titles");
}

#[test]
fn pi_title_retry_waits_for_a_completion_without_pending_foreground_operations() {
    use sage_acp_protocol::session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue};
    let _process = crate::acp::bindings::pi::title_request::tests::PROCESS_LOCK
        .lock()
        .unwrap();
    for (operation, preempted) in [
        ("model", false),
        ("thinking", false),
        ("native", false),
        ("model", true),
        ("thinking", true),
        ("native", true),
    ] {
        let directory = Directory::new();
        let (db, intent) = crate::acp::store::title_fixture();
        let admission = Arc::new(TitleAdmission::default());
        let mut titles = scheduler(&db, &admission);
        let mut runtime =
            directory.runtime(if preempted { "hang-once" } else { "success" }, &intent);
        let models = runtime.available_models.clone();
        let model = runtime.current_model.clone();
        ready(&mut runtime);
        runtime.available_models = models;
        runtime.current_model = model;
        if preempted {
            titles.completed(&intent, &mut runtime);
            wait_until(|| directory.0.join("child.pid").exists());
        }
        let foreground = titles.foreground_turn();
        if preempted {
            titles.cancel(&mut runtime);
        } else {
            titles.completed(&intent, &mut runtime);
        }
        drop(foreground);
        let (egress, reply) = match operation {
            "native" => (
                runtime.read_usage().unwrap(),
                json!({
                    "sessionFile":"/sessions/1_pi.jsonl", "sessionId":"pi-session-1",
                    "userMessages":2, "assistantMessages":2, "toolCalls":1, "toolResults":1, "totalMessages":6,
                    "tokens":{"input":1200,"output":340,"cacheRead":800,"cacheWrite":60,"total":2400},
                    "cost":0.0125, "contextUsage":{"tokens":2000,"contextWindow":200000,"percent":1}
                }),
            ),
            _ => {
                let (id, value, reply) = if operation == "model" {
                    (
                        "pi.model",
                        "fixture:same-model",
                        json!({"provider":"fixture","id":"same-model","name":"Fixture"}),
                    )
                } else {
                    ("pi.thinking", "off", Value::Null)
                };
                let config = ConfigIntent {
                    owner: sage_acp_protocol::scheduler::RuntimeOwner {
                        project_id: intent.project_id.clone(),
                        chat_id: intent.chat_id.clone(),
                        session_id: intent.session_id.clone(),
                        runtime_epoch: intent.runtime_epoch,
                    },
                    expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
                    mutation: ConfigMutation::Option {
                        config_id: ConfigId::new("config", id).unwrap(),
                        value: ConfigValue::Select {
                            value: ConfigId::new("value", value).unwrap(),
                        },
                    },
                };
                (runtime.configure(&config).unwrap().unwrap(), reply)
            }
        };
        let command = deliver(&mut runtime, egress);
        let next = next_completed(&db, &intent);
        titles.completed(&next, &mut runtime);
        finish(&mut titles, &mut runtime);
        assert_eq!(shown(&db), "Chat: Prompt fallback", "{operation}");
        let input =
            std::fs::read_to_string(directory.0.join("isolated-input.jsonl")).unwrap_or_default();
        assert_eq!(input.lines().count(), usize::from(preempted), "{operation}");
        runtime.ingest(&response(&command, reply), &Mapper).unwrap();
        finish(&mut titles, &mut runtime);
        assert_eq!(
            shown(&db),
            "Chat: Prompt fallback",
            "settling is not a completion"
        );
        titles.completed(&next_completed(&db, &next), &mut runtime);
        finish(&mut titles, &mut runtime);
        assert_eq!(shown(&db), "Chat: Check Sage chat titles", "{operation}");
    }
}

#[test]
fn pi_fast_following_turn_retries_after_preemption_cleanup_and_reuses_the_first_exchange() {
    let _process = crate::acp::bindings::pi::title_request::tests::PROCESS_LOCK
        .lock()
        .unwrap();
    let directory = Directory::new();
    let (db, intent) = crate::acp::store::title_fixture();
    let admission = Arc::new(TitleAdmission::default());
    let mut titles = scheduler(&db, &admission);
    let mut runtime = directory.runtime("hang-once", &intent);
    titles.completed(&intent, &mut runtime);
    wait_until(|| directory.0.join("child.pid").exists());
    let foreground = titles.foreground_turn();
    titles.cancel(&mut runtime);
    let next = next_completed(&db, &intent);
    drop(foreground);
    // The scheduler still owns the cancelling operation when this turn ends.
    titles.completed(&next, &mut runtime);
    finish(&mut titles, &mut runtime);
    assert_eq!(shown(&db), "Chat: Check Sage chat titles");
    let input = std::fs::read_to_string(directory.0.join("isolated-input.jsonl")).unwrap();
    let prompts: Vec<Value> = input
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(prompts.len(), 2);
    for prompt in prompts {
        assert_eq!(prompt["message"], "User:\nprompt\nAssistant:\nadapter text");
    }
}

#[test]
fn pi_real_failure_racing_foreground_preemption_stays_terminal_after_reopen() {
    let _process = crate::acp::bindings::pi::title_request::tests::PROCESS_LOCK
        .lock()
        .unwrap();
    for failure in ["malformed", "nonzero"] {
        let directory = Directory::new();
        let (db, intent) = crate::acp::store::title_fixture();
        let admission = Arc::new(TitleAdmission::default());
        let mut titles = scheduler(&db, &admission);
        let mut runtime = directory.runtime(failure, &intent);
        titles.completed(&intent, &mut runtime);
        wait_until(|| runtime.title_request.as_ref().unwrap().tree_empty());
        let foreground = titles.foreground_turn();
        titles.cancel(&mut runtime);
        finish(&mut titles, &mut runtime);
        drop(foreground);
        drop(titles);
        let mut reopened = scheduler(&db, &admission);
        reopened.completed(&next_completed(&db, &intent), &mut runtime);
        assert!(!reopened.pending(), "{failure} must not retry");
        assert_eq!(shown(&db), "Chat: Prompt fallback");
    }
}
