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
