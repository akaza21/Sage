use super::*;
use crate::acp::{
    bindings::{
        adapter::{AdapterCommand, EgressResolution, ResourceMapper},
        codex::{
            CodexRuntime, Platform,
            test_identity::{MANAGED_CODEX_HOME, user_agent},
        },
    },
    runtime::ProjectionMailbox,
    transport::{Frame, IncrementalFrameDecoder},
};
use sage_acp_protocol::intent::{SafeResourceRef, UnavailableResourceReason};
use serde_json::{Value, json};

struct Mapper;
impl ResourceMapper for Mapper {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}
fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap()
}
fn runtime() -> CodexRuntime {
    let mut runtime = CodexRuntime::fixture(Platform::WindowsX86_64);
    let init = runtime.command(AdapterCommand::Initialize).unwrap();
    let id: Value = serde_json::from_slice(init.frame().payload()).unwrap();
    runtime
        .resolve_egress(init.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = runtime
        .ingest(
            &frame(
                json!({"id":id["id"],"result":{"userAgent":user_agent("sage","unknown"),
        "codexHome":MANAGED_CODEX_HOME,"platformFamily":"windows","platformOs":"windows"}}),
            ),
            &Mapper,
        )
        .unwrap();
    for egress in initialized.egress() {
        runtime
            .resolve_egress(egress.token().clone(), EgressResolution::Queued)
            .unwrap();
    }
    let start = runtime
        .command(AdapterCommand::NewSession {
            // This runtime pins Windows protocol semantics on every test host.
            native_root: MANAGED_CODEX_HOME.to_owned(),
            model: None,
        })
        .unwrap();
    let id: Value = serde_json::from_slice(start.frame().payload()).unwrap();
    runtime.ingest(&frame(json!({"id":id["id"],"result":{"thread":{"id":"thread-main"},"model":"fixture-model",
        "modelProvider":"openai","cwd":"."}})),&Mapper).unwrap();
    runtime
}
pub(crate) fn scheduler(db: &Database, admission: &Arc<TitleAdmission>) -> TitleScheduler {
    TitleScheduler::new(
        db.clone(),
        Arc::new(ProjectionMailbox::default()),
        TitleCapability::GenerateIsolated,
        Arc::clone(admission),
    )
}
pub(crate) fn shown(db: &Database) -> String {
    db.connection()
        .query_row("SELECT title FROM chats", [], |r| r.get(0))
        .unwrap()
}
fn finish(scheduler: &mut TitleScheduler, runtime: &mut CodexRuntime) {
    finish_with_title(scheduler, runtime, None);
}
fn finish_with_title(
    scheduler: &mut TitleScheduler,
    runtime: &mut CodexRuntime,
    title: Option<&str>,
) {
    for _ in 0..8 {
        if let Some(egress) = scheduler.poll(runtime) {
            let value: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
            runtime
                .resolve_egress(egress.into_token(), EgressResolution::Written)
                .unwrap();
            for mut event in crate::acp::fixture::title_wire(&value) {
                if let Some(title) = title
                    && event["method"] == "item/completed"
                {
                    event["params"]["item"]["text"] = json!({"title":title}).to_string().into();
                }
                runtime.ingest(&frame(event), &Mapper).unwrap();
            }
        }
        if !scheduler.pending() {
            return;
        }
    }
    assert!(!scheduler.pending(), "bounded cleanup completed");
}

pub(crate) fn next_completed(db: &Database, intent: &PumpIntent) -> PumpIntent {
    let mut next = intent.clone();
    let id = format!("turn-{}", uuid::Uuid::new_v4());
    db.connection()
        .execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
         client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
         SELECT ?1,project_id,chat_id,session_id,runtime_epoch,
         (SELECT MAX(ordinal)+1 FROM turns),?1,'completed','correlated',0,100,100,100
         FROM turns WHERE id=?2",
            rusqlite::params![id, intent.active_turn.turn_id.as_str()],
        )
        .unwrap();
    next.active_turn = crate::acp::runtime::ActiveTurnIntent::store_minted(
        sage_acp_protocol::contract::SageTurnId::parse(id).unwrap(),
    )
    .unwrap();
    next
}

#[test]
fn busy_completion_retries_the_first_exchange_after_the_next_turn_even_after_reopen() {
    let admission = Arc::new(TitleAdmission::default());
    let (db, intent) = crate::acp::store::title_fixture();
    let mut titles = scheduler(&db, &admission);
    let mut live = runtime();
    let foreground = titles.foreground_turn();
    let mut completion = intent.clone();
    for _ in 0..5 {
        titles.completed(&completion, &mut live);
        assert!(!titles.pending());
        completion = next_completed(&db, &completion);
    }
    assert_eq!(shown(&db), "Chat: Prompt fallback");
    drop(foreground);
    drop(titles);
    let mut titles = scheduler(&db, &admission);
    let next = next_completed(&db, &completion);
    titles.completed(&next, &mut live);
    finish(&mut titles, &mut live);
    assert_eq!(shown(&db), "Chat: Investigate empty sidebar chats");
}

#[test]
fn preempted_title_retries_after_the_next_turn_and_absorbs_late_previous_replies() {
    let admission = Arc::new(TitleAdmission::default());
    let (db, intent) = crate::acp::store::title_fixture();
    let mut titles = scheduler(&db, &admission);
    let mut live = runtime();
    titles.completed(&intent, &mut live);
    let config = titles.poll(&mut live).unwrap();
    let request: Value = serde_json::from_slice(config.frame().payload()).unwrap();
    live.resolve_egress(config.into_token(), EgressResolution::Written)
        .unwrap();
    let foreground = titles.foreground_turn();
    titles.cancel(&mut live);
    finish(&mut titles, &mut live);
    assert_eq!(shown(&db), "Chat: Prompt fallback");
    titles.completed(&intent, &mut live);
    assert!(
        !titles.pending(),
        "no retry while foreground work is active"
    );
    drop(foreground);
    let next = next_completed(&db, &intent);
    titles.completed(&next, &mut live);
    // An oversized old reply must stay absorbed even after the retry replaces
    // the original operation; it must not fail the main conversation.
    live.discard_invalid_response(&frame(
        json!({"id":request["id"],"result":{"oversized":vec![0;513]}}),
    ))
    .unwrap();
    finish(&mut titles, &mut live);
    assert_eq!(shown(&db), "Chat: Investigate empty sidebar chats");
}

#[test]
fn three_foreground_preemptions_stop_durably_with_the_local_title() {
    let admission = Arc::new(TitleAdmission::default());
    let (db, mut intent) = crate::acp::store::title_fixture();
    let mut live = runtime();
    for attempt in 0..3 {
        let mut titles = scheduler(&db, &admission);
        titles.completed(&intent, &mut live);
        assert!(titles.pending(), "try {} is admitted", attempt + 1);
        let foreground = titles.foreground_turn();
        if attempt % 2 == 0 {
            titles.cancel(&mut live);
        }
        finish(&mut titles, &mut live);
        assert_eq!(shown(&db), "Chat: Prompt fallback");
        drop(foreground);
        intent = next_completed(&db, &intent);
    }
    let mut titles = scheduler(&db, &admission);
    titles.completed(&intent, &mut live);
    assert!(
        !titles.pending(),
        "the fourth try is refused after reopening"
    );
    assert_eq!(shown(&db), "Chat: Prompt fallback");
}

#[test]
fn timeout_rejection_and_malformed_output_are_terminal_even_after_reopen() {
    for failure in ["timeout", "rejection", "malformed"] {
        let admission = Arc::new(TitleAdmission::default());
        let (db, intent) = crate::acp::store::title_fixture();
        let mut titles = scheduler(&db, &admission);
        let mut live = runtime();
        titles.completed(&intent, &mut live);
        match failure {
            "timeout" => {
                titles.active.as_mut().unwrap().deadline = Instant::now();
                titles.cancel(&mut live);
                finish(&mut titles, &mut live);
            }
            "rejection" => {
                let config = titles.poll(&mut live).unwrap();
                let request: Value = serde_json::from_slice(config.frame().payload()).unwrap();
                live.resolve_egress(config.into_token(), EgressResolution::Written)
                    .unwrap();
                live.ingest(
                    &frame(
                        json!({"id":request["id"],"error":{"code":-32601,"message":"Unavailable"}}),
                    ),
                    &Mapper,
                )
                .unwrap();
                // A real failure already received must win over a racing send.
                titles.cancel(&mut live);
                finish(&mut titles, &mut live);
            }
            "malformed" => finish_with_title(&mut titles, &mut live, Some("New conversation")),
            _ => unreachable!(),
        }
        assert_eq!(shown(&db), "Chat: Prompt fallback", "{failure}");
        drop(titles);
        let next = next_completed(&db, &intent);
        let mut reopened = scheduler(&db, &admission);
        reopened.completed(&next, &mut live);
        assert!(!reopened.pending(), "{failure} cannot retry");
        assert_eq!(shown(&db), "Chat: Prompt fallback");
    }
}

#[test]
fn title_scheduler_saturation_timeout_exit_and_preemption_preserve_local_titles_and_release_capacity()
 {
    let admission = Arc::new(TitleAdmission::default());
    let (db, intent) = crate::acp::store::title_fixture();
    let mut first = scheduler(&db, &admission);
    let mut live = runtime();
    first.completed(&intent, &mut live);
    assert!(first.pending());
    let (other_db, other_intent) = crate::acp::store::title_fixture();
    let mut busy = scheduler(&other_db, &admission);
    let mut other = runtime();
    busy.completed(&other_intent, &mut other);
    assert!(!busy.pending());
    assert_eq!(shown(&other_db), "Chat: Prompt fallback");
    finish(&mut first, &mut live);
    assert_eq!(shown(&db), "Chat: Investigate empty sidebar chats");
    first.completed(&intent, &mut live);
    assert!(!first.pending());
    // Capacity becoming available lets the pending chat try at its next completion.
    let next = next_completed(&other_db, &other_intent);
    let mut reopened = scheduler(&other_db, &admission);
    reopened.completed(&next, &mut other);
    finish(&mut reopened, &mut other);
    assert_eq!(shown(&other_db), "Chat: Investigate empty sidebar chats");
    for failure in ["timeout", "process exit", "foreground"] {
        let (db, intent) = crate::acp::store::title_fixture();
        let mut titles = scheduler(&db, &admission);
        let mut live = runtime();
        titles.completed(&intent, &mut live);
        assert!(
            titles.pending(),
            "previous operation released global capacity"
        );
        match failure {
            "timeout" => titles.active.as_mut().unwrap().deadline = Instant::now(),
            "foreground" => admission.foreground(),
            "process exit" => {
                drop(titles);
                drop(live);
                assert_eq!(shown(&db), "Chat: Prompt fallback");
                continue;
            }
            _ => unreachable!(),
        }
        finish(&mut titles, &mut live);
        assert_eq!(shown(&db), "Chat: Prompt fallback");
    }
    let (db, intent) = crate::acp::store::title_fixture();
    let mut titles = scheduler(&db, &admission);
    let mut live = runtime();
    titles.completed(&intent, &mut live);
    assert!(titles.pending());
    finish(&mut titles, &mut live);
    assert_eq!(shown(&db), "Chat: Investigate empty sidebar chats");
}
