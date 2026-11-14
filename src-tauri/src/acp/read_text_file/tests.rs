use std::{
    fs, io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::AsyncWrite;
use uuid::Uuid;

use super::terminal::MAX_PENDING_READS;
use super::*;
use crate::acp::{runtime::read_text_file::ReadTextRequestId, transport::DeadlineAsyncWriter};
use sage_acp_protocol::contract::{
    ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
};

struct Capture(Vec<u8>);
impl AsyncWrite for Capture {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn owner(project: &str, session: &str) -> ReadTextOwner {
    ReadTextOwner {
        project_id: SageProjectId::parse(project).unwrap(),
        project_generation: ProjectGeneration::new(1).unwrap(),
        chat_id: SageChatId::parse("chat-p02").unwrap(),
        session_id: SageAgentSessionId::parse(session).unwrap(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
    }
}
fn fixture() -> (std::path::PathBuf, ReadTextFileService, String, ResourceRef) {
    let root = std::env::temp_dir().join(format!("sage-p02-egress-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("file.txt"), "one\ntwo\n").unwrap();
    let database = Database::open(":memory:".as_ref()).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let workspace = Workspace::open(&project.id, &project.roots[0].id, &root).unwrap();
    (
        root,
        ReadTextFileService::new(database),
        project.id,
        workspace.resource("file.txt").unwrap(),
    )
}
fn ack(event: &ReadTextFlushEvent, state: ReadTextFlushState) -> ReadTextFlushAck {
    ReadTextFlushAck {
        owner: event.owner.clone(),
        resource: event.resource.clone(),
        expected_editor_revision: event.expected_editor_revision,
        token: event.token.clone().into(),
        state,
    }
}
fn ready(service: &ReadTextFileService, begin: &ReadTextBegin) {
    service
        .acknowledge(ack(
            &begin.event,
            ReadTextFlushState::Classify {
                state: ReadTextClassification::NotOpen,
                editor_revision: None,
            },
        ))
        .unwrap();
    service
        .acknowledge(ack(
            &begin.event,
            ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::NotOpen,
                editor_revision: None,
            },
        ))
        .unwrap();
}

#[test]
fn async_delivery_has_one_lf_and_owns_one_success_receipt() {
    let (root, service, project, resource) = fixture();
    let begin = service
        .begin(
            owner(&project, "async"),
            resource,
            ReadTextRange {
                line: Some(2),
                limit: Some(1),
            },
        )
        .unwrap();
    ready(&service, &begin);
    let lease = begin.receiver.blocking_receive().unwrap();
    let mut egress = DeadlineAsyncWriter::new(Capture(vec![]));
    let delivery = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(lease.deliver_response(&ReadTextRequestId::Number(7), &mut egress))
        .unwrap();
    assert!(matches!(delivery, terminal::ReadTextDelivery::Complete(_)));
    assert_eq!(
        egress.into_inner().0,
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"content\":\"two\\n\"}}\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn receipt_capacity_is_attempts_plus_receipts_and_owner_drain_reopens_slot() {
    let (root, service, project, resource) = fixture();
    let first_owner = owner(&project, "0");
    let mut begins = Vec::new();
    for index in 0..MAX_PENDING_READS {
        begins.push(
            service
                .begin(
                    owner(&project, &index.to_string()),
                    resource.clone(),
                    ReadTextRange {
                        line: None,
                        limit: None,
                    },
                )
                .unwrap(),
        );
    }
    assert!(
        matches!(service.begin(owner(&project, "overflow"), resource.clone(), ReadTextRange { line: None, limit: None }), Err(error) if error.kind == ReadTextFailureKind::Busy)
    );
    drop(begins.remove(0).receiver);
    let sink = service.receipt_sink();
    let receipts = sink.drain_owner(&first_owner);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].resource.relative_path, "file.txt");
    assert_eq!(
        receipts[0].range,
        ReadTextRange {
            line: None,
            limit: None
        }
    );
    assert!(receipts[0].token.starts_with("p02-"));
    assert!(
        service
            .begin(
                first_owner,
                resource,
                ReadTextRange {
                    line: None,
                    limit: None
                }
            )
            .is_ok()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dropped_lease_transfers_one_unknown_fallback_receipt() {
    let (root, service, project, resource) = fixture();
    let owner = owner(&project, "drop");
    let begin = service
        .begin(
            owner.clone(),
            resource,
            ReadTextRange {
                line: None,
                limit: None,
            },
        )
        .unwrap();
    ready(&service, &begin);
    drop(begin.receiver.blocking_receive().unwrap());
    let receipts = service.receipt_sink().drain_owner(&owner);
    assert_eq!(receipts.len(), 1);
    assert!(
        serde_json::to_string(&receipts[0].payload)
            .unwrap()
            .contains("deliveryUnknown")
    );
    fs::remove_dir_all(root).unwrap();
}
