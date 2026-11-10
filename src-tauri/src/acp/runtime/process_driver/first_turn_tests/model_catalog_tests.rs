use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};
use crate::acp::{contract::SageAgentSessionId, runtime::AdapterPumpError};
use serde_json::{Value, json};

use super::*;

#[path = "model_catalog_tests/lifecycle.rs"]
mod lifecycle;

struct CatalogTransport {
    writes: Arc<Mutex<Vec<Value>>>,
    mailbox: Arc<DriverMailbox>,
    shutdown_after: Option<usize>,
    skills_reads: Arc<AtomicUsize>,
}

impl CatalogTransport {
    fn new(
        writes: &Arc<Mutex<Vec<Value>>>,
        mailbox: Arc<DriverMailbox>,
        shutdown_after: Option<usize>,
    ) -> Self {
        Self {
            writes: Arc::clone(writes),
            mailbox,
            shutdown_after,
            skills_reads: Default::default(),
        }
    }
}

impl crate::acp::runtime::EgressTransport for CatalogTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let request: Value = serde_json::from_slice(frame.payload()).unwrap();
        // The idle skills read is its own traffic; these tests follow the
        // model catalog, and `skill_command_tests` follow the skills.
        if request["method"] == "skills/list" {
            self.skills_reads.fetch_add(1, Ordering::Relaxed);
            return FrameWriteOutcome::Complete;
        }
        let mut writes = self.writes.lock().unwrap();
        writes.push(request);
        if self.shutdown_after == Some(writes.len()) {
            drop(self.mailbox.submit_worker(WorkerCommand::Shutdown).unwrap());
        }
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for CatalogTransport {
    fn close(&mut self) {}
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap()
}

fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn ready_runtime(external_session: &ExternalId) -> CodexRuntime {
    ready_adapter(
        CodexRuntime::fixture(Platform::WindowsX86_64),
        external_session,
        true,
    )
}

pub(super) fn ready_adapter<Adapter: HarnessAdapter>(
    mut adapter: Adapter,
    external_session: &ExternalId,
    confirmed_config: bool,
) -> Adapter {
    let initialize = adapter.command(AdapterCommand::Initialize).unwrap();
    adapter
        .ingest(
            &frame(json!({"id":request_id(&initialize),"result":{
                "userAgent":user_agent("sage", "unknown"),
                "codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    let start = adapter
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("model", "selected").unwrap()),
        })
        .unwrap();
    let mut result = json!({"thread":{"id":external_session.as_str()}});
    if confirmed_config {
        result["model"] = json!("selected");
        result["reasoningEffort"] = json!("high");
    }
    adapter
        .ingest(
            &frame(json!({"id":request_id(&start),"result":result})),
            &Mapper,
        )
        .unwrap();
    adapter
}

#[test]
fn idle_worker_hydrates_model_catalog_and_bounds_pagination() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for selected_on_last_page in [false, true] {
            let fixture = prepare_first_turn_fixture().await;
            let adapter = ready_runtime(&fixture.external_session);
            let writes = Arc::new(Mutex::new(Vec::new()));
            let skills_reads = Arc::new(AtomicUsize::new(0));
            let mailbox = Arc::new(DriverMailbox::new().unwrap());
            let mut completed = CompletedActivationDriverParts {
                parts: IdleActivationParts::new(
                    adapter,
                    fixture.activation_journal,
                    Mapper,
                    ActivatedSession {
                        binding_digest: [0x11; 32],
                        project_id: fixture.project_id.clone(),
                        chat_id: fixture.chat_id.clone(),
                        session_id: fixture.started.session_id.clone(),
                        external_session_id: fixture.external_session,
                        runtime_epoch: fixture.epoch,
                        capabilities: fixture.capabilities,
                        models: Vec::new(),
                        next_model_cursor: None,
                    },
                ),
                idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
                transport: CatalogTransport {
                    skills_reads: Arc::clone(&skills_reads),
                    ..CatalogTransport::new(&writes, Arc::clone(&mailbox), Some(4))
                },
                control: NoChildProcessControl {
                    zero_checks: Arc::new(AtomicUsize::new(0)),
                },
                owner: fixture.owner.clone(),
                clock: Arc::new(SystemDriverClock),
            };
            completed
                .parts
                .hydrate_model_config(&mut completed.transport);
            let first_id = writes.lock().unwrap()[0]["id"].as_u64().unwrap();
            let authority = Arc::new(Authority(IntentSnapshot {
                project_id: fixture.project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id: fixture.chat_id,
                projection_revision: fixture.ready_projection_revision,
                intent_revision: fixture.ready_projection_revision,
                session_id: Some(fixture.started.session_id),
                runtime_epoch: Some(fixture.epoch),
                turn_id: None,
                turn_state: None,
                turn_revision: None,
            }));
            let context = FirstTurnWorkerContext {
                owner: fixture.owner,
                coordinator: SendCoordinator::new(
                    authority,
                    fixture.journal.clone(),
                    fixture.publisher.clone(),
                ),
                journal: fixture.journal.clone(),
                publisher: fixture.publisher,
                clock: Arc::new(SystemDriverClock),
                mailbox: Arc::clone(&mailbox),
                session_recovery: None,
                zero_sequence: std::sync::atomic::AtomicU64::new(0),
            };
            let mut chunk = Vec::new();
            for page in 0..4 {
                let model = if selected_on_last_page && page == 3 {
                    json!({"id":"selected","displayName":"Selected","isDefault":false,
                    "defaultReasoningEffort":"high",
                    "supportedReasoningEfforts":[{"reasoningEffort":"high","description":"Deep"}]})
                } else {
                    json!({"id":format!("other-{page}"),"displayName":format!("Other {page}"),
                    "isDefault":false})
                };
                chunk.extend(
                    serde_json::to_vec(&json!({"id":first_id + page,"result":{
                        "data":[model],"nextCursor":format!("page-{}",page + 1)
                    }}))
                    .unwrap(),
                );
                chunk.push(b'\n');
            }
            mailbox.try_push_chunk(chunk).unwrap();
            let (state, reason) = super::super::live_runner::drive_for_test(
                FirstTurnWorkerState::Idle(completed),
                &context,
            )
            .await;
            assert_eq!(reason, TreeExit::ParentShutdown);
            assert_eq!(writes.lock().unwrap().len(), 4);
            // The skills are read once while idle, beside the catalog.
            assert_eq!(skills_reads.load(Ordering::Relaxed), 1);
            for (page, request) in writes.lock().unwrap().iter().enumerate().skip(1) {
                assert_eq!(request["method"], "model/list");
                assert_eq!(request["params"]["cursor"], format!("page-{page}"));
            }
            let FirstTurnWorkerState::Idle(idle) = state else {
                panic!("idle worker")
            };
            assert_eq!(idle.parts.session().models.len(), 4);
            let (adapter, _, _, _) = idle.parts.into_parts();
            if selected_on_last_page {
                let config = serde_json::to_value(adapter.session_config().unwrap()).unwrap();
                assert_eq!(config["configOptions"][0]["currentValue"], "high");
                assert_eq!(
                    config["confirmedModel"],
                    json!({"id":"selected","name":"Selected"})
                );
            } else {
                let config = serde_json::to_value(adapter.session_config().unwrap()).unwrap();
                assert_eq!(config["configOptions"], json!([]));
                assert_eq!(config["confirmedModel"], json!({"id":"selected"}));
            }
            drop(context);
            drop(fixture.sessions);
            drop(fixture.scheduler);
            drop(fixture.journal);
            fixture.journal_task.finish();
        }
    });
}

type IdleCatalog = CompletedActivationDriverParts<
    CodexRuntime,
    (),
    Mapper,
    CatalogTransport,
    NoChildProcessControl,
>;

fn accept_reply(idle: &mut IdleCatalog, value: Value) -> Result<(), AdapterPumpError> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
        .chunks(8_192)
        .try_for_each(|chunk| idle.accept_idle_stdout_chunk(chunk))
}

#[test]
fn idle_catalog_keeps_exact_aggregate_limits_and_ignores_one_more_model_without_paging() {
    for full_width in [false, true] {
        let external_session = ExternalId::new("thread", "catalog-thread").unwrap();
        let project_id = SageProjectId::parse("project-catalog").unwrap();
        let chat_id = SageChatId::parse("chat-catalog").unwrap();
        let session_id = SageAgentSessionId::parse("session-catalog").unwrap();
        let runtime_epoch = RuntimeEpoch::new(1).unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut idle = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                ready_runtime(&external_session),
                (),
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch,
                    capabilities: capabilities(),
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: CatalogTransport::new(
                &writes,
                Arc::new(DriverMailbox::new().unwrap()),
                None,
            ),
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
            },
            owner: RuntimeOwner {
                project_id,
                chat_id,
                session_id,
                runtime_epoch,
            },
            clock: Arc::new(SystemDriverClock),
        };
        idle.parts.hydrate_model_config(&mut idle.transport);
        for page in 0..2 {
            let request_id = writes.lock().unwrap().last().unwrap()["id"]
                .as_u64()
                .unwrap();
            let models: Vec<_> = (0..64)
                .map(|index| {
                    let id = format!("model-{}", page * 64 + index);
                    json!({"id":if full_width {format!("{id:x<512}")} else {id},
                    "displayName":if full_width {"n".repeat(512)} else {"Model".to_owned()},
                    "isDefault":false})
                })
                .collect();
            accept_reply(
                &mut idle,
                json!({"id":request_id,"result":{
                    "data":models,"nextCursor":format!("page-{}", page + 1)
                }}),
            )
            .unwrap();
        }
        assert_eq!(idle.parts.session().models.len(), 128);
        if full_width {
            let bytes: usize = idle
                .parts
                .session()
                .models
                .iter()
                .map(|model| model.actual_bytes().unwrap())
                .sum();
            assert_eq!(bytes, 131_072);
        }
        let retained_models = idle.parts.session().models.clone();
        let retained_cursor = idle.parts.session().next_model_cursor.clone();
        let written = writes.lock().unwrap().len();
        let request_id = writes.lock().unwrap().last().unwrap()["id"]
            .as_u64()
            .unwrap();
        accept_reply(
            &mut idle,
            json!({"id":request_id,"result":{
                "data":[{"id":"extra","displayName":"Extra","isDefault":false}],"nextCursor":"page-3"
            }}),
        )
        .unwrap();
        assert_eq!(idle.parts.session().models, retained_models);
        assert_eq!(idle.parts.session().next_model_cursor, retained_cursor);
        for _ in 0..2 {
            idle.parts.hydrate_model_config(&mut idle.transport);
        }
        assert_eq!(writes.lock().unwrap().len(), written);
    }
}
