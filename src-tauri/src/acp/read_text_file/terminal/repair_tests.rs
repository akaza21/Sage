use super::*;
use crate::acp::{
    read_text_file::{ReadTextFileService, ReadTextFlushAck},
    service::AcpService,
    transport::DeadlineAsyncWriter,
};
use crate::persistence::Database;
use sage_acp_protocol::contract::{
    ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
};
use std::{
    fs,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::AsyncWrite;

struct Fixture {
    root: std::path::PathBuf,
    service: ReadTextFileService,
    owner: ReadTextOwner,
    resource: ResourceRef,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-p02-repair-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("file.txt"), "hello").unwrap();
        let database = Database::open(":memory:".as_ref()).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let workspace = Workspace::open(&project.id, &project.roots[0].id, &root).unwrap();
        Self {
            root,
            service: ReadTextFileService::new(database),
            owner: ReadTextOwner {
                project_id: SageProjectId::parse(&project.id).unwrap(),
                project_generation: ProjectGeneration::new(1).unwrap(),
                chat_id: SageChatId::parse("chat").unwrap(),
                session_id: SageAgentSessionId::parse("session").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            },
            resource: workspace.resource("file.txt").unwrap(),
        }
    }
    fn begin(&self) -> ReadTextBegin {
        self.service
            .begin(
                self.owner.clone(),
                self.resource.clone(),
                ReadTextRange {
                    line: None,
                    limit: None,
                },
            )
            .unwrap()
    }
    fn classified(&self) -> ReadTextBegin {
        let begin = self.begin();
        self.service
            .acknowledge(ack(
                &begin,
                ReadTextFlushState::Classify {
                    state: ReadTextClassification::NotOpen,
                    editor_revision: None,
                },
            ))
            .unwrap();
        begin
    }
    fn lease(&self) -> ReadTextCompletionLease {
        let begin = self.classified();
        self.service.acknowledge(ack(&begin, disk())).unwrap();
        begin.receiver.blocking_receive().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn disk() -> ReadTextFlushState {
    ReadTextFlushState::DiskAllowed {
        state: super::super::DiskAllowedState::NotOpen,
        editor_revision: None,
    }
}
fn ack(begin: &ReadTextBegin, state: ReadTextFlushState) -> ReadTextFlushAck {
    ReadTextFlushAck {
        owner: begin.event.owner.clone(),
        resource: begin.event.resource.clone(),
        expected_editor_revision: begin.event.expected_editor_revision,
        token: begin.event.token.clone().into(),
        state,
    }
}
struct Script {
    remaining: usize,
    bytes: Vec<u8>,
}
impl AsyncWrite for Script {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.remaining == 0 {
            return Poll::Pending;
        }
        let count = self.remaining.min(bytes.len());
        self.remaining -= count;
        self.bytes.extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
async fn poll_pending(future: Pin<&mut impl Future>) {
    let mut future = future;
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}
fn assert_reclaimed(f: &Fixture) {
    let inner = state_machine::lock(&f.service.state);
    assert!(inner.attempts.is_empty());
    assert!(inner.reservations.is_empty());
    assert!(
        inner.receipts.is_empty(),
        "receipt already handed to caller"
    );
    drop(inner);
    drop(f.begin());
}
#[tokio::test]
async fn encode_failure_emits_one_zero_receipt_and_frees_slot() {
    let f = Fixture::new();
    let mut lease = f.lease();
    lease.completion.as_mut().unwrap().content =
        "x".repeat(sage_core::limits::preview::P02_READ_TEXT_SOURCE_BYTES + 1);
    let mut writer = DeadlineAsyncWriter::new(Vec::<u8>::new());
    let delivery = lease
        .deliver_response(&ReadTextRequestId::Number(1), &mut writer)
        .await
        .unwrap();
    assert!(matches!(delivery, ReadTextDelivery::ZeroByteFailure(_)));
    assert!(writer.into_inner().is_empty());
    assert_reclaimed(&f);
}
#[tokio::test]
async fn delivering_cancel_zero_and_partial_preserve_one_receipt_and_cause() {
    for accepted in [0, 1] {
        let f = Fixture::new();
        let lease = f.lease();
        let token = lease.token.clone();
        let mut writer = DeadlineAsyncWriter::new(Script {
            remaining: accepted,
            bytes: vec![],
        });
        let mut delivery =
            Box::pin(lease.deliver_response(&ReadTextRequestId::Number(1), &mut writer));
        poll_pending(delivery.as_mut()).await;
        assert!(cancel_exact(&f.service.state, &f.owner, &token).is_some());
        let result = delivery.await.unwrap();
        assert_eq!(
            matches!(result, ReadTextDelivery::ZeroByteFailure(_)),
            accepted == 0
        );
        assert_eq!(
            matches!(result, ReadTextDelivery::EffectUnknown(_)),
            accepted == 1
        );
        let payload = serde_json::to_string(&result.receipt().payload).unwrap();
        assert!(payload.contains(if accepted == 0 {
            "cancelled"
        } else {
            "deliveryUnknown"
        }));
        assert_eq!(writer.into_inner().bytes.len(), accepted);
        assert_reclaimed(&f);
    }
}
#[tokio::test]
async fn complete_after_deadline_demotes_to_effect_unknown() {
    let f = Fixture::new();
    let lease = f.lease();
    // Capture the full physical frame, then move the settlement deadline behind
    // now during flush; this deterministically covers the post-I/O race.
    struct Capture {
        state: Arc<ReadState>,
        token: String,
        bytes: Vec<u8>,
    }
    impl AsyncWrite for Capture {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            state_machine::lock(&self.state)
                .attempts
                .get_mut(&self.token)
                .unwrap()
                .pending
                .deadline = Instant::now() - Duration::from_secs(1);
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    let mut writer = DeadlineAsyncWriter::new(Capture {
        state: Arc::clone(&f.service.state),
        token: lease.token.clone(),
        bytes: vec![],
    });
    let result = lease
        .deliver_response(&ReadTextRequestId::Number(1), &mut writer)
        .await
        .unwrap();
    assert!(matches!(result, ReadTextDelivery::EffectUnknown(_)));
    assert!(writer.into_inner().bytes.ends_with(b"\n"));
    assert_reclaimed(&f);
}
#[tokio::test]
async fn pending_terminalize_waits_until_delivery_or_future_drop() {
    for drop_future in [false, true] {
        let f = Fixture::new();
        let mut writer = DeadlineAsyncWriter::new(Script {
            remaining: 0,
            bytes: vec![],
        });
        let mut delivery = Box::pin(
            f.lease()
                .deliver_response(&ReadTextRequestId::Number(1), &mut writer),
        );
        poll_pending(delivery.as_mut()).await;
        let settlement = f.service.terminalize_all();
        assert!(!settlement.wait_until(Instant::now()));
        if drop_future {
            drop(delivery);
            assert_eq!(f.service.receipt_sink().drain_owner(&f.owner).len(), 1);
        } else {
            let result = delivery.await.unwrap();
            assert!(matches!(result, ReadTextDelivery::ZeroByteFailure(_)));
            assert!(
                serde_json::to_string(&result.receipt().payload)
                    .unwrap()
                    .contains("unavailable")
            );
        }
        assert!(settlement.wait_until(Instant::now()));
        assert_reclaimed(&f);
    }
}
#[test]
fn terminal_ack_foreign_stale_duplicate_and_late_are_fail_closed() {
    for case in ["foreign", "stale", "duplicate", "late"] {
        let f = Fixture::new();
        let begin = f.classified();
        let mut invalid = ack(&begin, disk());
        match case {
            "foreign" => invalid.owner.session_id = SageAgentSessionId::parse("foreign").unwrap(),
            "stale" => invalid.owner.runtime_epoch = RuntimeEpoch::new(2).unwrap(),
            "duplicate" => {
                f.service.acknowledge(ack(&begin, disk())).unwrap();
            }
            "late" => {
                state_machine::lock(&f.service.state)
                    .attempts
                    .get_mut(&begin.event.token)
                    .unwrap()
                    .pending
                    .deadline = Instant::now() - Duration::from_secs(1);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            f.service.acknowledge(invalid).unwrap_err().kind,
            ReadTextFailureKind::Unavailable
        );
        if matches!(case, "foreign" | "stale") {
            assert!(
                state_machine::lock(&f.service.state)
                    .attempts
                    .get(&begin.event.token)
                    .unwrap()
                    .phase
                    == AttemptPhase::Waiting
            );
        }
        drop(begin);
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_project_switch_globally_revokes_target_and_foreign_async_writers() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("other")).unwrap();
    fs::write(f.root.join("other/file.txt"), "other").unwrap();
    let other = f
        .service
        .database
        .create_project_from_root(&f.root.join("other"))
        .unwrap();
    assert_ne!(other.id, f.owner.project_id.as_str());
    let service = AcpService::new(f.service.database.clone()).unwrap();
    let mut leases = Vec::new();
    for (project, session) in [
        (&f.owner.project_id, "target"),
        (&SageProjectId::parse(&other.id).unwrap(), "foreign"),
    ] {
        let mut owner = f.owner.clone();
        owner.project_id = project.clone();
        owner.session_id = SageAgentSessionId::parse(session).unwrap();
        let resource = ResourceRef {
            root_id: if session == "target" {
                f.resource.root_id.clone()
            } else {
                other.roots[0].id.clone()
            },
            relative_path: "file.txt".into(),
        };
        let begin = service
            .read_text_files
            .begin(
                owner,
                resource,
                ReadTextRange {
                    line: None,
                    limit: None,
                },
            )
            .unwrap();
        service
            .read_text_files
            .acknowledge(ack(
                &begin,
                ReadTextFlushState::Classify {
                    state: ReadTextClassification::NotOpen,
                    editor_revision: None,
                },
            ))
            .unwrap();
        service
            .read_text_files
            .acknowledge(ack(&begin, disk()))
            .unwrap();
        leases.push(begin.receiver.blocking_receive().unwrap());
    }
    let mut writers = [
        DeadlineAsyncWriter::new(Script {
            remaining: 0,
            bytes: vec![],
        }),
        DeadlineAsyncWriter::new(Script {
            remaining: 0,
            bytes: vec![],
        }),
    ];
    let mut futures: Vec<_> = leases
        .into_iter()
        .zip(writers.iter_mut())
        .map(|(lease, writer)| {
            Box::pin(lease.deliver_response(&ReadTextRequestId::Number(1), writer))
        })
        .collect();
    for future in &mut futures {
        poll_pending(future.as_mut()).await;
    }
    tokio::task::block_in_place(|| {
        std::thread::scope(|scope| {
            let switch = scope.spawn(|| service.project_switched(&f.owner.project_id));
            tokio::runtime::Handle::current().block_on(async {
                for future in futures {
                    let result = future.await.unwrap();
                    assert!(matches!(result, ReadTextDelivery::ZeroByteFailure(_)));
                    assert!(
                        serde_json::to_string(&result.receipt().payload)
                            .unwrap()
                            .contains("unavailable")
                    );
                }
            });
            switch.join().unwrap().unwrap();
        })
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_and_service_drop_settle_pending_async_writers() {
    for mode in ["shutdown", "acp-drop", "read-drop"] {
        for accepted in [0, 1] {
            let f = Fixture::new();
            let mut service = AcpService::new(f.service.database.clone()).unwrap();
            let begin = service
                .read_text_files
                .begin(
                    f.owner.clone(),
                    f.resource.clone(),
                    ReadTextRange {
                        line: None,
                        limit: None,
                    },
                )
                .unwrap();
            service
                .read_text_files
                .acknowledge(ack(
                    &begin,
                    ReadTextFlushState::Classify {
                        state: ReadTextClassification::NotOpen,
                        editor_revision: None,
                    },
                ))
                .unwrap();
            service
                .read_text_files
                .acknowledge(ack(&begin, disk()))
                .unwrap();
            let lease = begin.receiver.blocking_receive().unwrap();
            let sink = service.read_text_files.receipt_sink();
            let state = Arc::clone(&service.read_text_files.state);
            let mut writer = DeadlineAsyncWriter::new(Script {
                remaining: accepted,
                bytes: vec![],
            });
            let mut delivery =
                Box::pin(lease.deliver_response(&ReadTextRequestId::Number(1), &mut writer));
            poll_pending(delivery.as_mut()).await;
            let database = f.service.database.clone();
            tokio::task::block_in_place(|| {
                std::thread::scope(|scope| {
                    let teardown = scope.spawn(move || match mode {
                        "shutdown" => service.shutdown_blocking().unwrap(),
                        "acp-drop" => drop(service),
                        "read-drop" => {
                            let reads = std::mem::replace(
                                &mut service.read_text_files,
                                ReadTextFileService::new(database),
                            );
                            drop(reads);
                        }
                        _ => unreachable!(),
                    });
                    let result = tokio::runtime::Handle::current()
                        .block_on(delivery)
                        .unwrap();
                    assert_eq!(
                        matches!(result, ReadTextDelivery::ZeroByteFailure(_)),
                        accepted == 0
                    );
                    assert_eq!(
                        matches!(result, ReadTextDelivery::EffectUnknown(_)),
                        accepted == 1
                    );
                    assert!(
                        serde_json::to_string(&result.receipt().payload)
                            .unwrap()
                            .contains(if accepted == 0 {
                                "unavailable"
                            } else {
                                "deliveryUnknown"
                            })
                    );
                    teardown.join().unwrap();
                })
            });
            assert_eq!(writer.into_inner().bytes.len(), accepted);
            assert!(sink.drain_owner(&f.owner).is_empty());
            assert!(state_machine::lock(&state).attempts.is_empty());
        }
    }
}
