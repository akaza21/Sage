use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{sync::Notify, task::JoinHandle};

use crate::acp::{contract::ProjectionRevision, intent::ProjectionChangedV1};

pub(crate) type NotificationSinkFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), NotificationError>> + Send + 'a>>;

pub(crate) trait NotificationSink: Send + Sync + 'static {
    /// Observes every committed change, including background chats suppressed
    /// by disposable projection routing. Must not wait for platform I/O.
    fn observe_committed(&self, _event: &ProjectionChangedV1) {}
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotificationError {
    #[error("the projection notification sink is unavailable")]
    SinkUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationRecovery {
    TupleContention,
    SinkFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishOutcome {
    Scheduled,
    Coalesced,
    SuppressedInactive,
    BootstrapRequired,
    RecoveryRequired(NotificationRecovery),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PublishError {
    #[error("projection notification revision must be positive")]
    InvalidRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectionKey {
    project_id: crate::acp::contract::SageProjectId,
    chat_id: crate::acp::contract::SageChatId,
    session_id: crate::acp::contract::SageAgentSessionId,
}

impl ProjectionKey {
    fn from_event(event: &ProjectionChangedV1) -> Self {
        Self {
            project_id: event.project_id.clone(),
            chat_id: event.chat_id.clone(),
            session_id: event.session_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Running,
    RecoveryRequired(NotificationRecovery),
    Shutdown,
}

#[derive(Debug)]
struct State {
    lifecycle: Lifecycle,
    active_project: Option<crate::acp::contract::SageProjectId>,
    active_key: Option<ProjectionKey>,
    greatest_revision: Option<(ProjectionRevision, u64)>,
    pending: Option<ProjectionChangedV1>,
    in_flight: bool,
    bootstrap_ack_pending: bool,
    wake_scheduled: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            lifecycle: Lifecycle::Running,
            active_project: None,
            active_key: None,
            greatest_revision: None,
            pending: None,
            in_flight: false,
            bootstrap_ack_pending: false,
            wake_scheduled: false,
        }
    }
}

struct Inner {
    state: Mutex<State>,
    wake: Notify,
    recovery_wake: Notify,
    sink: Arc<dyn NotificationSink>,
    #[cfg(test)]
    scheduled_wakes: std::sync::atomic::AtomicUsize,
}

pub(crate) struct ProjectionNotifier {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl ProjectionNotifier {
    #[cfg(test)]
    pub(crate) fn start(sink: Arc<dyn NotificationSink>) -> Self {
        Self::start_with(sink, tokio::spawn)
    }

    pub(crate) fn start_with(
        sink: Arc<dyn NotificationSink>,
        spawn: impl FnOnce(Pin<Box<dyn Future<Output = ()> + Send + 'static>>) -> JoinHandle<()>,
    ) -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(State::default()),
            wake: Notify::new(),
            recovery_wake: Notify::new(),
            sink,
            #[cfg(test)]
            scheduled_wakes: std::sync::atomic::AtomicUsize::new(0),
        });
        let worker_inner = Arc::clone(&inner);
        let worker = spawn(Box::pin(async move { run(worker_inner).await }));
        Self {
            inner,
            worker: Mutex::new(Some(worker)),
        }
    }

    /// Accepts a projection change only after its durable transaction committed.
    pub(crate) fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<PublishOutcome, PublishError> {
        if event.projection_revision == ProjectionRevision::ZERO {
            return Err(PublishError::InvalidRevision);
        }
        self.inner.sink.observe_committed(&event);
        let key = ProjectionKey::from_event(&event);
        let mut notify = false;
        let outcome = {
            let mut state = lock(&self.inner.state);
            match state.lifecycle {
                Lifecycle::Shutdown => PublishOutcome::Shutdown,
                Lifecycle::RecoveryRequired(reason) => PublishOutcome::RecoveryRequired(reason),
                Lifecycle::Running
                    if state
                        .active_project
                        .as_ref()
                        .is_some_and(|project| project != &event.project_id) =>
                {
                    PublishOutcome::SuppressedInactive
                }
                Lifecycle::Running if state.active_key.as_ref().is_some_and(|it| it != &key) => {
                    state.lifecycle =
                        Lifecycle::RecoveryRequired(NotificationRecovery::TupleContention);
                    state.pending = None;
                    state.greatest_revision = None;
                    state.bootstrap_ack_pending = false;
                    PublishOutcome::BootstrapRequired
                }
                Lifecycle::Running => {
                    state.active_key.get_or_insert(key);
                    let outcome = match state.greatest_revision {
                        Some(revision)
                            if (event.projection_revision, event.child_projection_epoch)
                                <= revision =>
                        {
                            PublishOutcome::Coalesced
                        }
                        _ => {
                            state.greatest_revision =
                                Some((event.projection_revision, event.child_projection_epoch));
                            let replacing_pending = state.pending.replace(event).is_some();
                            if replacing_pending {
                                PublishOutcome::Coalesced
                            } else {
                                PublishOutcome::Scheduled
                            }
                        }
                    };
                    if state.pending.is_some() && !state.wake_scheduled {
                        state.wake_scheduled = true;
                        notify = true;
                    }
                    outcome
                }
            }
        };
        if notify {
            #[cfg(test)]
            self.inner
                .scheduled_wakes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.wake.notify_one();
        }
        if outcome == PublishOutcome::BootstrapRequired {
            self.inner.recovery_wake.notify_one();
        }
        Ok(outcome)
    }

    /// Changes only disposable projection routing. Durable background Turns
    /// continue to commit, but their old-project notices cannot contend with
    /// or repopulate the newly active WebView projection.
    pub(crate) fn set_active_project(&self, project_id: crate::acp::contract::SageProjectId) {
        let mut state = lock(&self.inner.state);
        state.active_project = Some(project_id.clone());
        if state
            .active_key
            .as_ref()
            .is_some_and(|key| key.project_id != project_id)
        {
            state.pending = None;
            state.active_key = None;
            state.greatest_revision = None;
            if !state.in_flight {
                state.wake_scheduled = false;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn recovery(&self) -> Option<NotificationRecovery> {
        match lock(&self.inner.state).lifecycle {
            Lifecycle::RecoveryRequired(reason) => Some(reason),
            Lifecycle::Running | Lifecycle::Shutdown => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn scheduled_wakes(&self) -> usize {
        self.inner
            .scheduled_wakes
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Returns an actionable recovery reason, or `None` when an in-flight
    /// recovery is acknowledged and settles before becoming actionable (and
    /// on shutdown).
    #[cfg(test)]
    pub(crate) async fn wait_for_recovery(&self) -> Option<NotificationRecovery> {
        let mut observed_recovery = false;
        loop {
            let notified = self.inner.recovery_wake.notified();
            let (lifecycle, in_flight) = {
                let state = lock(&self.inner.state);
                (state.lifecycle, state.in_flight)
            };
            match lifecycle {
                Lifecycle::RecoveryRequired(reason) if !in_flight => return Some(reason),
                Lifecycle::Shutdown => return None,
                Lifecycle::Running if observed_recovery => return None,
                Lifecycle::Running => notified.await,
                Lifecycle::RecoveryRequired(_) => {
                    observed_recovery = true;
                    notified.await;
                }
            }
        }
    }

    /// Call only after an authoritative projection bootstrap has completed.
    pub(crate) fn acknowledge_authoritative_bootstrap(&self) -> bool {
        let mut state = lock(&self.inner.state);
        if !matches!(state.lifecycle, Lifecycle::RecoveryRequired(_)) {
            return false;
        }
        if state.in_flight {
            state.bootstrap_ack_pending = true;
            return true;
        }
        finish_authoritative_bootstrap(&mut state);
        drop(state);
        self.inner.recovery_wake.notify_waiters();
        true
    }

    #[cfg(test)]
    pub(crate) async fn shutdown(&self) -> bool {
        self.shutdown_with_timeout(Duration::from_millis(
            crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
        ))
        .await
    }

    pub(crate) async fn shutdown_with_timeout(&self, timeout: Duration) -> bool {
        {
            let mut state = lock(&self.inner.state);
            state.lifecycle = Lifecycle::Shutdown;
            state.pending = None;
            state.active_project = None;
            state.active_key = None;
            state.greatest_revision = None;
            state.bootstrap_ack_pending = false;
            state.wake_scheduled = false;
        }
        self.inner.recovery_wake.notify_one();
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            worker.abort();
            return tokio::time::timeout(timeout, worker).await.is_ok();
        }
        true
    }
}

impl Drop for ProjectionNotifier {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.inner.state);
            state.lifecycle = Lifecycle::Shutdown;
            state.pending = None;
            state.active_project = None;
            state.active_key = None;
            state.greatest_revision = None;
            state.bootstrap_ack_pending = false;
            state.wake_scheduled = false;
        }
        if let Some(worker) = lock(&self.worker).take() {
            worker.abort();
        }
    }
}

async fn run(inner: Arc<Inner>) {
    loop {
        inner.wake.notified().await;
        loop {
            let event = {
                let mut state = lock(&inner.state);
                if state.lifecycle == Lifecycle::Shutdown {
                    return;
                }
                match state.pending.take() {
                    Some(event) if state.lifecycle == Lifecycle::Running => {
                        state.in_flight = true;
                        Some(event)
                    }
                    _ => {
                        state.in_flight = false;
                        state.wake_scheduled = false;
                        None
                    }
                }
            };
            let Some(event) = event else { break };
            let result = inner.sink.publish(event).await;
            let mut state = lock(&inner.state);
            state.in_flight = false;
            if result.is_err() {
                state.lifecycle = Lifecycle::RecoveryRequired(NotificationRecovery::SinkFailure);
                state.pending = None;
                state.active_key = None;
                state.greatest_revision = None;
                state.bootstrap_ack_pending = false;
                state.wake_scheduled = false;
                drop(state);
                inner.recovery_wake.notify_one();
                break;
            }
            let recovery_settled = if state.bootstrap_ack_pending
                && matches!(state.lifecycle, Lifecycle::RecoveryRequired(_))
            {
                finish_authoritative_bootstrap(&mut state);
                true
            } else {
                false
            };
            if state.lifecycle != Lifecycle::Running || state.pending.is_none() {
                let recovery_ready = matches!(state.lifecycle, Lifecycle::RecoveryRequired(_));
                state.active_key = None;
                state.greatest_revision = None;
                state.wake_scheduled = false;
                drop(state);
                if recovery_settled {
                    inner.recovery_wake.notify_waiters();
                } else if recovery_ready {
                    inner.recovery_wake.notify_one();
                }
                break;
            }
        }
    }
}

fn finish_authoritative_bootstrap(state: &mut State) {
    state.lifecycle = Lifecycle::Running;
    state.active_key = None;
    state.greatest_revision = None;
    state.bootstrap_ack_pending = false;
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
