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
fn scheduler(db: &Database, admission: &Arc<TitleAdmission>) -> TitleScheduler {
    TitleScheduler::new(
        db.clone(),
        Arc::new(ProjectionMailbox::default()),
        TitleCapability::GenerateIsolated,
        Arc::clone(admission),
    )
}
fn shown(db: &Database) -> String {
    db.connection()
        .query_row("SELECT title FROM chats", [], |r| r.get(0))
        .unwrap()
}
fn finish(scheduler: &mut TitleScheduler, runtime: &mut CodexRuntime) {
    for _ in 0..8 {
        if let Some(egress) = scheduler.poll(runtime) {
            let value: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
            runtime
                .resolve_egress(egress.into_token(), EgressResolution::Written)
                .unwrap();
            for event in crate::acp::fixture::title_wire(&value) {
                runtime.ingest(&frame(event), &Mapper).unwrap();
            }
        }
        if !scheduler.pending() {
            return;
        }
    }
    assert!(!scheduler.pending(), "bounded cleanup completed");
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
    // Busy admission is spent durably, including on a new scheduler after restart.
    scheduler(&other_db, &admission).completed(&other_intent, &mut other);
    assert!(!other.title_pending());
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
