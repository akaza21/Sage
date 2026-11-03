use super::*;
use crate::acp::{
    browser::{
        BrowserCapability, BrowserFailure, BrowserRequest, BrowserResult, FakeBrowserCapability,
        permission::{BrowserCapabilityDispatch, BrowserDispatchPermit, PendingBrowserResult},
        session_capability::attached::{
            AttachedSessionCapability, AttachedTabFactory, BrowserSessionContext,
        },
    },
    contract::SageServerRequestId,
    runtime::{BoundedThread, JournalHandle, SessionCoordinator},
    store::{
        AcpStore, DatabaseAcpStore, StoreCommit, StoreError, StoreMutation, StoreMutationResult,
    },
};
use crate::browser::state_query::bootstrap_fixture::BrowserDocumentFixture;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

pub(super) const WAIT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(super) enum Event {
    Prepared(SageTurnId),
    Turn(SageTurnId, TurnState),
    Permission(SageServerRequestId),
    BrowserDone(Result<BrowserResult, BrowserFailure>),
}

pub(super) fn next(events: &mpsc::Receiver<Event>) -> Event {
    events
        .recv_timeout(WAIT)
        .expect("causal bootstrap event before deadline")
}

pub(super) struct Release(pub(super) Option<mpsc::SyncSender<()>>);
impl Release {
    pub(super) fn once(&mut self) {
        self.0
            .take()
            .expect("release exactly once")
            .send(())
            .unwrap();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.try_send(());
        }
    }
}

pub(super) struct CausalStore {
    pub(super) inner: DatabaseAcpStore,
    pub(super) database: crate::persistence::Database,
    pub(super) project: SageProjectId,
    pub(super) chat: SageChatId,
    pub(super) events: mpsc::SyncSender<Event>,
    pub(super) release: mpsc::Receiver<()>,
    pub(super) prepared: usize,
    pub(super) states: Vec<(SageTurnId, TurnState)>,
}

impl AcpStore for CausalStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let committed = self.inner.commit(batch)?;
        // inner.commit has returned: its WriteConnection/database mutex is gone.
        if matches!(batch, [StoreMutation::PrepareTurn(_)]) {
            self.prepared += 1;
            assert!(self.prepared <= 2, "bounded two-turn journey");
            if self.prepared == 2 {
                let [StoreMutationResult::Prepared(turn)] = committed.results.as_slice() else {
                    panic!("real prepared turn")
                };
                self.events
                    .try_send(Event::Prepared(turn.turn_id.clone()))
                    .unwrap();
                self.release
                    .recv_timeout(WAIT)
                    .expect("release turn-two acknowledgement");
            }
        }
        if let [StoreMutation::ReceivePermission { intent, .. }] = batch {
            self.events
                .try_send(Event::Permission(intent.request_id.clone()))
                .unwrap();
        }
        if matches!(batch, [StoreMutation::AdapterInboundBatch(_)]) {
            let page = crate::acp::store::load_chat_page(
                &self.database.connection(),
                self.project.clone(),
                self.chat.clone(),
                None,
            )
            .unwrap();
            assert!(page.turns.as_slice().len() <= 2);
            for turn in page.turns.as_slice() {
                if !self.states.contains(&(turn.id.clone(), turn.state))
                    && matches!(
                        turn.state,
                        TurnState::Running | TurnState::Completed | TurnState::Cancelled
                    )
                {
                    self.events
                        .try_send(Event::Turn(turn.id.clone(), turn.state))
                        .unwrap();
                }
            }
            self.states = page
                .turns
                .as_slice()
                .iter()
                .map(|t| (t.id.clone(), t.state))
                .collect();
        }
        Ok(committed)
    }
}

pub(super) fn install_store(service: &mut AcpService, store: impl AcpStore + 'static) {
    service.journal.shutdown();
    service
        .journal_thread
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .wait_and_join(WAIT)
        .unwrap();
    let (journal, runner) = JournalHandle::new(store);
    let thread =
        BoundedThread::spawn("bootstrap-causal-journal", move || runner.run_blocking()).unwrap();
    service.session_starts = SessionCoordinator::new(
        journal.clone(),
        crate::acp::scheduler::Scheduler::new(),
        service.publisher.clone(),
    );
    service.lanes = (0..crate::acp::service::LIVE_RUNTIME_LANES)
        .map(|_| crate::acp::service::live_lanes::LiveLane::new(&journal, &service.publisher))
        .collect();
    service.journal = journal;
    *service.journal_thread.lock().unwrap() = Some(thread);
}

#[derive(Clone)]
pub(super) struct Context {
    pub(super) setup: Arc<FixtureSetup>,
    pub(super) document: Arc<BrowserDocumentFixture>,
    pub(super) session: crate::acp::contract::SageAgentSessionId,
}
impl BrowserSessionContext for Context {
    fn service(&self) -> &AcpService {
        &self.setup.service
    }
    fn settled_tab(&self, label: &str) -> Result<(String, u64), String> {
        self.document.settled_tab(label)
    }
}

#[derive(Default)]
pub(super) struct Trace {
    pub(super) browser: FakeBrowserCapability,
    pub(super) dispatched: usize,
    pub(super) issued: usize,
    pub(super) unknown: bool,
}
pub(super) struct Factory(pub(super) Arc<Mutex<Trace>>);
pub(super) struct Port {
    trace: Arc<Mutex<Trace>>,
    context: Context,
}

impl AttachedTabFactory<Context> for Factory {
    type Port<'a> = Port;
    fn port<'a>(
        &'a mut self,
        context: &'a Context,
        _: crate::acp::browser::attachment::AttachmentLease,
    ) -> Port {
        Port {
            trace: self.0.clone(),
            context: context.clone(),
        }
    }
}
impl BrowserCapabilityDispatch for Port {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        self.trace.lock().unwrap().dispatched += 1;
        let trace = self.trace.clone();
        let context = self.context.clone();
        let request = request.clone();
        Ok(Box::new(move || {
            let (outcome, unknown) = permit.with_live_lock(|| {
                permit.mark_issued();
                let mut trace = trace.lock().unwrap();
                trace.issued += 1;
                let unknown = std::mem::take(&mut trace.unknown);
                Ok((trace.browser.perform(&request), unknown))
            })?;
            if unknown {
                assert!(context.service().take_over_browser_tab("browser-1"));
                assert!(context.service().hand_back_browser_tab("browser-1"));
                Err(BrowserFailure::OutcomeUnknown)
            } else {
                outcome
            }
        }))
    }
}

pub(super) fn perform(
    context: Context,
    trace: Arc<Mutex<Trace>>,
    request: &BrowserRequest,
) -> Result<BrowserResult, BrowserFailure> {
    let mut factory = Factory(trace);
    let setup = context.setup.clone();
    let session = context.session.clone();
    AttachedSessionCapability::new(
        context,
        setup.project_id.clone(),
        setup.chat_id.clone(),
        session,
        &mut factory,
    )
    .perform(request)
}

pub(super) struct Call {
    thread: BoundedThread,
    context: Context,
}
impl Call {
    pub(super) fn spawn(
        context: Context,
        trace: Arc<Mutex<Trace>>,
        request: BrowserRequest,
        events: mpsc::SyncSender<Event>,
    ) -> Self {
        let worker_context = context.clone();
        Self {
            context,
            thread: BoundedThread::spawn("bootstrap-browser-call", move || {
                events
                    .try_send(Event::BrowserDone(perform(worker_context, trace, &request)))
                    .unwrap();
            })
            .unwrap(),
        }
    }
    pub(super) fn finish(mut self) {
        self.thread.wait_and_join(WAIT).unwrap();
    }
}
impl Drop for Call {
    fn drop(&mut self) {
        if self.thread.wait(Duration::ZERO).is_err() {
            self.context.service().revoke_browser_tab("browser-1");
            let _ = self.thread.wait(WAIT);
        }
    }
}

pub(super) struct Cleanup(pub(super) Arc<FixtureSetup>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.service.shutdown_blocking();
    }
}
