use super::*;
use crate::acp::fixture::{
    external_fixture_adapters::ExternalCase, handoff_support::HandoffJourney,
};
use crate::acp::{contract::ClientIntentId, service::AcpService, store::load_chat_page};

pub(super) async fn continue_after_refusal(
    mut environment: FixtureEnvironment,
    deadline: FixtureDeadline,
) {
    let service = AcpService::new_with_registry_publisher_for_test(
        environment.database.clone(),
        Arc::new(crate::acp::bindings::registry::BindingRegistry::default()),
        environment.publisher.clone(),
    )
    .unwrap();
    let page = load_chat_page(
        &environment.database.connection(),
        environment.project_id.clone(),
        environment.chat_id.clone(),
        None,
    )
    .unwrap();
    let current = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| Some(&session.id) == page.current_session_id.as_ref())
        .unwrap();
    assert_eq!(current.state, SessionState::Ready);
    assert_eq!(current.residency, crate::acp::lifecycle::Residency::Cold);
    let retry = service
        .replace_refused_resume(&crate::acp::store::ActivateSessionCommand {
            project_id: environment.project_id.clone(),
            chat_id: environment.chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::parse("resume-fixture-activate-2").unwrap(),
            session_id: environment.started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            artifact_digest: String::new(),
            now_millis: environment.now_millis,
        })
        .unwrap()
        .expect("definite refusal must prepare a fresh activation");
    let replaced = load_chat_page(
        &environment.database.connection(),
        environment.project_id.clone(),
        environment.chat_id.clone(),
        None,
    )
    .unwrap();
    environment.started = crate::acp::store::SessionMutation {
        session_id: retry.session_id,
        runtime_epoch: None,
        state: SessionState::Created,
        revision: crate::acp::contract::EntityRevision::ZERO,
        projection_revision: replaced.projection_revision,
        disposition: crate::acp::store::MutationDisposition::Applied,
    };
    assert_ne!(
        Some(&environment.started.session_id),
        page.current_session_id.as_ref()
    );
    service.shutdown_blocking().unwrap();
    drop(service);
    let mut journey =
        HandoffJourney::from_environment(ExternalCase::TurnTextSuccess, deadline, environment)
            .await;
    let plan = journey.plan().0;
    assert!(plan.contains("immutable historical prompt"));
    journey.complete().await;
    assert_eq!(
        journey.summary().state,
        crate::acp::context_handoff::HandoffState::Inline
    );
    assert!(journey.summary().carried_items > 0);
    assert_eq!(journey.send_count(), 1);
    journey.finish().await;
}
