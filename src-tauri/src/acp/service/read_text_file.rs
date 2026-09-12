//! Bounded P-02 read coordination for a future admitted ACP adapter pump.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    acp::{
        contract::SageRootId,
        limits::LIMITS,
        resource_mapper::{ConfiguredResourceRoot, NativeResourceAdmission, NativeResourceMapper},
        runtime::read_text_file::ReadTextRange,
    },
    domain::ResourceRef,
    persistence::Database,
    workspace::{Workspace, select_text_lines},
};

const MAX_EDITABLE_BUFFER_BYTES: usize = 1024 * 1024;
mod lifecycle;
mod source;
mod terminal;
use terminal::{
    ClassificationRequest, PendingRead, ReadState, ReadTextCompletion, ReadTextFailure,
    ReadTextFailureKind, ReadTextReceiptSink, ReadTextResponseReceiver, ReadTextResult,
    TerminalReceipt, acknowledge_start, cancel_exact, classify_attempt, drain_receipts, expire_all,
    expire_at, finish_read, insert_attempt,
};

pub(crate) use terminal::ReadTextBegin;
mod types;
pub(crate) use types::{
    DiskAllowedState, ReadTextClassification, ReadTextFlushAck, ReadTextFlushEvent,
    ReadTextFlushState, ReadTextNextAction, ReadTextOwner,
};

pub(crate) struct ReadTextFileService {
    database: Database,
    next_token: AtomicU64,
    token_nonce: String,
    state: Arc<ReadState>,
    receipt_sink: ReadTextReceiptSink,
}
impl ReadTextFileService {
    pub(crate) fn new(database: Database) -> Self {
        let state = Arc::new(ReadState::new());
        let receipt_sink = ReadTextReceiptSink::new(Arc::clone(&state));
        Self {
            database,
            next_token: AtomicU64::new(0),
            token_nonce: uuid::Uuid::new_v4().to_string(),
            state,
            receipt_sink,
        }
    }
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter seam; exercised by conformance tests and not production-admitted"
    )]
    /// Resolves a pinned ACP absolute path under an owner Project's registered
    /// roots before beginning the ordinary two-step source handshake.
    pub(crate) fn begin_absolute(
        &self,
        owner: ReadTextOwner,
        absolute_path: &std::path::Path,
        range: ReadTextRange,
    ) -> ReadTextResult<ReadTextBegin> {
        if !absolute_path.is_absolute() {
            return Err(ReadTextFailure::terminal(ReadTextFailureKind::Forbidden));
        }
        // Map and pin before any ResourceRef or editor-buffer lookup. A fresh
        // mapper immediately revalidates that proof, closing the mapper-to-
        // begin replacement gap without ever rewriting the admitted ref.
        let mapper = self.mapper_for_owner(&owner)?;
        let absolute_path = absolute_path
            .to_str()
            .ok_or(ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        let admission = mapper
            .try_admit_existing(absolute_path)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        self.mapper_for_owner(&owner)?
            .revalidate_admission(&admission)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        self.begin_admitted(owner, admission, range)
    }

    fn begin_admitted(
        &self,
        owner: ReadTextOwner,
        admission: NativeResourceAdmission,
        range: ReadTextRange,
    ) -> ReadTextResult<ReadTextBegin> {
        let resource = ResourceRef {
            root_id: admission.resource().root_id.as_str().to_owned(),
            relative_path: admission.resource().relative_path.as_str().to_owned(),
        };
        let workspace = self.resolve_workspace(&owner, &resource)?;
        if workspace.root_identity() != admission.root_identity() {
            return Err(ReadTextFailure::terminal(ReadTextFailureKind::Forbidden));
        }
        self.begin_with_admission(owner, resource, range, Some(admission), workspace)
    }

    /// Legacy test seam. Production absolute ingress must use
    /// `begin_absolute`, which supplies a native admission proof.
    #[cfg(test)]
    pub(crate) fn begin(
        &self,
        owner: ReadTextOwner,
        resource: ResourceRef,
        range: ReadTextRange,
    ) -> ReadTextResult<ReadTextBegin> {
        let workspace = self.resolve_workspace(&owner, &resource)?;
        self.begin_with_admission(owner, resource, range, None, workspace)
    }

    fn begin_with_admission(
        &self,
        owner: ReadTextOwner,
        resource: ResourceRef,
        range: ReadTextRange,
        admission: Option<NativeResourceAdmission>,
        workspace: Workspace,
    ) -> ReadTextResult<ReadTextBegin> {
        // Authority is owned by the durable project registry, never by a
        // caller-constructed Workspace with a copied project label.
        // `begin_absolute` has already produced the exact root-qualified
        // identity. Validate it without rewriting it: rewriting here could
        // silently turn an accepted spelling into a different alias before
        // the post-renderer and held-handle checks use it.
        workspace
            .validate_resource(&resource, false)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        let expected_editor_revision = self
            .database
            .editor_buffer_revision(workspace.project_id(), &resource)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Corrupt))?;
        let sequence = self
            .next_token
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Unavailable))?
            .checked_add(1)
            .expect("fetch_update only accepts incrementable token values");
        let token = format!("p02-{}-{sequence}", self.token_nonce);
        let deadline = Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.ordinary_rpc);
        let (notify, receiver) = tokio::sync::oneshot::channel();
        insert_attempt(
            &self.state,
            token.clone(),
            PendingRead {
                owner: owner.clone(),
                resource: resource.clone(),
                expected_editor_revision,
                workspace,
                admission,
                range,
                deadline,
            },
            notify,
        )?;
        Ok(ReadTextBegin {
            event: ReadTextFlushEvent {
                owner,
                resource,
                expected_editor_revision,
                token: token.clone(),
            },
            receiver: ReadTextResponseReceiver::new(
                Arc::clone(&self.state),
                token,
                deadline,
                receiver,
            ),
        })
    }
    pub(super) fn mapper_for_owner(
        &self,
        owner: &ReadTextOwner,
    ) -> ReadTextResult<NativeResourceMapper> {
        let roots = self
            .database
            .root_records(owner.project_id.as_str())
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        NativeResourceMapper::new(
            roots
                .into_iter()
                .map(|root| {
                    Ok(ConfiguredResourceRoot {
                        root_id: SageRootId::parse(&root.id).map_err(|_| {
                            ReadTextFailure::terminal(ReadTextFailureKind::Forbidden)
                        })?,
                        canonical_native_root: root.absolute_path,
                    })
                })
                .collect::<ReadTextResult<Vec<_>>>()?,
        )
        .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))
    }

    fn resolve_workspace(
        &self,
        owner: &ReadTextOwner,
        resource: &ResourceRef,
    ) -> ReadTextResult<Workspace> {
        let root = self
            .database
            .root_record(&resource.root_id)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))?;
        if root.id != resource.root_id || root.project_id != owner.project_id.as_str() {
            return Err(ReadTextFailure::terminal(ReadTextFailureKind::Forbidden));
        }
        Workspace::open(&root.project_id, &root.id, &root.absolute_path)
            .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Forbidden))
    }
    /// Identity acknowledgement moves a still-tracked attempt into source I/O.
    /// It never owns content and a cancellation/lifecycle revocation wins over
    /// source completion before any completion lease is exposed.
    pub(crate) fn acknowledge(&self, ack: ReadTextFlushAck) -> ReadTextResult<ReadTextNextAction> {
        if let ReadTextFlushState::Classify {
            state,
            editor_revision,
        } = ack.state
        {
            let durable_revision = self
                .database
                .editor_buffer_revision(ack.owner.project_id.as_str(), &ack.resource)
                .map_err(|_| ReadTextFailure::terminal(ReadTextFailureKind::Corrupt))?;
            return classify_attempt(
                &self.state,
                ClassificationRequest {
                    owner: &ack.owner,
                    resource: &ack.resource,
                    token: &ack.token,
                    expected_revision: ack.expected_editor_revision,
                    classification: state,
                    editor_revision,
                    durable_revision,
                },
            );
        }
        let pending = acknowledge_start(
            &self.state,
            &ack.owner,
            &ack.resource,
            &ack.token,
            ack.expected_editor_revision,
            &ack.state,
        )?;
        let result = self.read_acknowledged(&pending, &ack);
        finish_read(&self.state, &ack.token, result);
        Ok(ReadTextNextAction::Complete)
    }
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter seam; exercised by conformance tests and not production-admitted"
    )]
    pub(crate) fn cancel(&self, owner: &ReadTextOwner, token: &str) -> ReadTextResult<()> {
        let deadline = cancel_exact(&self.state, owner, token)
            .ok_or(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable))?;
        let settlement = terminal::settlement_handle(
            Arc::clone(&self.state),
            std::iter::once(token.to_owned()).collect(),
        );
        if settlement.wait_until(deadline) {
            Ok(())
        } else {
            Err(ReadTextFailure::terminal(ReadTextFailureKind::Timeout))
        }
    }
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter seam; exercised by conformance tests and not production-admitted"
    )]
    pub(crate) fn expire(&self) -> Vec<TerminalReceipt> {
        expire_all(&self.state);
        Vec::new()
    }
    /// Scheduler-facing monotonic expiry seam. This is the same production
    /// state transition used by the ordinary scheduler, not a test-only jump.
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter scheduler seam; exercised by conformance tests"
    )]
    pub(crate) fn expire_at(&self, now: Instant) -> Vec<TerminalReceipt> {
        expire_at(&self.state, now);
        Vec::new()
    }
    /// A future admitted adapter retains this independent handle while it owns
    /// receiver/lease delivery, including after this service is dropped.
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter receipt sink seam"
    )]
    pub(crate) fn receipt_sink(&self) -> ReadTextReceiptSink {
        self.receipt_sink.clone()
    }
    /// Fallback obligations are drained only by the exact owner; there is no
    /// global unattributed receipt consumer.
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter seam; exercised by conformance tests and not production-admitted"
    )]
    pub(crate) fn take_terminal_receipts(&self, owner: &ReadTextOwner) -> Vec<TerminalReceipt> {
        drain_receipts(&self.state, owner)
    }
}
#[cfg(test)]
#[path = "read_text_file/tests.rs"]
mod tests;
