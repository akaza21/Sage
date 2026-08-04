use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};

use crate::acp::{
    contract::{ProjectionRevision, SageAgentSessionId, SageChatId, SageProjectId},
    fixture::generated_cases::FrozenCase,
    intent::ProjectionChangedV1,
    runtime::{
        NotificationError, NotificationSink, NotificationSinkFuture, ProjectionNotifier,
        PublishOutcome,
    },
};

pub(crate) const CASE_IDS: &[&str] = &[];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    assert_eq!(case.id, CASE_IDS[0]);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let sink = Arc::new(CountingSink::default());
        let notifier = ProjectionNotifier::start(sink.clone());
        notifier.set_active_project(SageProjectId::parse("project-new").unwrap());

        // This flag represents the already-committed owning Turn. Projection
        // routing is intentionally evaluated only after that durable fact.
        let durable_owning_turn_completed = true;
        let outcome = notifier
            .publish_committed(ProjectionChangedV1 {
                project_id: SageProjectId::parse("project-old").unwrap(),
                chat_id: SageChatId::parse("chat-old").unwrap(),
                session_id: SageAgentSessionId::parse("session-old").unwrap(),
                projection_revision: ProjectionRevision::new(1).unwrap(),
            })
            .unwrap();
        tokio::task::yield_now().await;
        let active_projection_changed = sink.entries.load(Ordering::SeqCst) != 0;
        notifier.shutdown().await;
        json!({
            "durableOwningTurnMayComplete": durable_owning_turn_completed
                && outcome == PublishOutcome::SuppressedInactive,
            "activeForeignProjectProjectionChanged": active_projection_changed,
        })
    })
}

#[derive(Default)]
struct CountingSink {
    entries: AtomicUsize,
}

impl NotificationSink for CountingSink {
    fn publish<'a>(&'a self, _event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            self.entries.fetch_add(1, Ordering::SeqCst);
            Ok::<(), NotificationError>(())
        })
    }
}
