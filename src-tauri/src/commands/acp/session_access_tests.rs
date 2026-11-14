use super::*;
use crate::acp::contract::RuntimeEpoch;
use crate::acp::session_config::{
    ConfigId, ConfigMutation, ConfigRevision, ConfigValue, SessionConfig,
};
use serde_json::json;
use std::sync::Mutex;

struct Session {
    config: Option<SessionConfig>,
    configure: Result<(), HostError>,
    applied: Mutex<Vec<ConfigMutation>>,
    turn: Option<Result<(), AcpIpcError>>,
    cancelled: Mutex<u32>,
}

impl Session {
    fn reporting(current: &str, advertised: &[&str]) -> Self {
        let modes = advertised
            .iter()
            .map(|id| json!({"id": id, "name": id}))
            .collect::<Vec<_>>();
        Self {
            config: Some(
                SessionConfig::from_wire(
                    &json!({"modes": {"currentModeId": current, "availableModes": modes}}),
                    true,
                )
                .unwrap(),
            ),
            configure: Ok(()),
            applied: Mutex::default(),
            turn: None,
            cancelled: Mutex::default(),
        }
    }
    fn applied(&self) -> Vec<ConfigMutation> {
        self.applied.lock().unwrap().clone()
    }
}

impl SessionModeHost for Session {
    fn live_config(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay> {
        Some(SessionConfigOverlay {
            sage_agent_session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            config_revision: ConfigRevision::first(),
            removed: false,
            config: self.config.clone()?,
        })
    }
    fn configure(&self, intent: ConfigIntent) -> Result<(), HostError> {
        self.configure.clone()?;
        // As Claude's runtime does, a running turn keeps its mode.
        if self.turn.is_some() {
            return Err(HostError::State);
        }
        self.applied.lock().unwrap().push(intent.mutation);
        Ok(())
    }
}

const CLAUDE: &[&str] = &[
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "bypassPermissions",
];

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-access").unwrap(),
        chat_id: SageChatId::parse("chat-access").unwrap(),
        session_id: SageAgentSessionId::parse("session-access").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
}

fn select(session: &Session, mode: SessionAccessMode) -> Result<(), AcpIpcError> {
    let native = crate::acp::bindings::access_policy::session_mode("claude-agent-acp", mode);
    apply_session_mode(session, owner(), "claude-agent-acp", mode, native)
}

#[test]
fn the_chosen_mode_is_selected_before_the_prompt() {
    let session = Session::reporting("default", CLAUDE);
    select(&session, SessionAccessMode::Plan).unwrap();
    assert_eq!(
        session.applied(),
        vec![ConfigMutation::Mode(ConfigId::new("id", "plan").unwrap())]
    );
    let session = Session::reporting("plan", CLAUDE);
    select(&session, SessionAccessMode::Plan).unwrap();
    assert!(session.applied().is_empty());
}

#[test]
fn a_failed_select_stops_the_prompt() {
    let mut session = Session::reporting("default", CLAUDE);
    session.configure = Err(HostError::ConfigMutationRejected);
    assert!(matches!(
        select(&session, SessionAccessMode::AcceptEdits),
        Err(AcpIpcError::Rejected { .. })
    ));
    let mut session = Session::reporting("default", CLAUDE);
    session.config = None;
    assert!(matches!(
        select(&session, SessionAccessMode::AcceptEdits),
        Err(AcpIpcError::InvalidRequest { .. })
    ));
    assert!(session.applied().is_empty());
}

#[test]
fn a_mode_the_session_does_not_offer_is_refused() {
    // Claude run as root advertises no bypass mode.
    let session = Session::reporting("default", &["default", "acceptEdits", "plan"]);
    for mode in [SessionAccessMode::FullAccess, SessionAccessMode::Auto] {
        assert!(matches!(
            select(&session, mode),
            Err(AcpIpcError::InvalidRequest { .. })
        ));
    }
    assert!(session.applied().is_empty());
}

#[test]
fn every_full_access_turn_is_logged() {
    let session = Session::reporting("default", CLAUDE);
    let (results, captured) = sage_test_support::test_diagnostics::capture_diagnostics(|| {
        let first = select(&session, SessionAccessMode::FullAccess);
        let session = Session::reporting("bypassPermissions", CLAUDE);
        let second = select(&session, SessionAccessMode::FullAccess);
        let third = select(&session, SessionAccessMode::FullAccess);
        [first, second, third]
    });
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(
        captured
            .matches("Permissive access mode for this turn")
            .count(),
        3
    );
    let session = Session::reporting("default", &["default"]);
    let (result, captured) = sage_test_support::test_diagnostics::capture_diagnostics(|| {
        select(&session, SessionAccessMode::FullAccess)
    });
    assert!(result.is_err());
    assert!(!captured.contains("Permissive access mode for this turn"));
}

#[test]
fn a_governed_binding_refuses_mode_changes_outside_the_access_path() {
    use super::super::session_config::refuse_governed_mode_change;
    let id = |value: &str| ConfigId::new("id", value).unwrap();
    let bypass = ConfigMutation::Mode(id("bypassPermissions"));
    let mode_option = ConfigMutation::Option {
        config_id: id("mode"),
        value: ConfigValue::Select {
            value: id("bypassPermissions"),
        },
    };
    let model_option = ConfigMutation::Option {
        config_id: id("model"),
        value: ConfigValue::Select { value: id("opus") },
    };
    let live = Session::reporting("default", CLAUDE).live_config(&owner());
    for live in [live.as_ref(), None] {
        assert!(refuse_governed_mode_change(Some(true), live, &bypass).is_err());
        assert!(refuse_governed_mode_change(Some(true), live, &mode_option).is_err());
        assert!(refuse_governed_mode_change(Some(true), live, &model_option).is_ok());
        assert!(refuse_governed_mode_change(Some(false), live, &bypass).is_ok());
        assert!(refuse_governed_mode_change(None, live, &bypass).is_err());
        assert!(refuse_governed_mode_change(None, live, &mode_option).is_err());
        assert!(refuse_governed_mode_change(None, live, &model_option).is_ok());
    }
}

#[test]
fn a_deleted_chat_keeps_no_access_choice() {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let settings = crate::settings_store::SettingsStore::in_memory().unwrap();
    let token = settings
        .prepare_chat_access("session-gone", SessionAccessMode::FullAccess, 1)
        .unwrap();
    settings
        .grant_chat_access("session-gone", SessionAccessMode::FullAccess, &token, 2)
        .unwrap();
    crate::settings_store::forget_removed_chat_access(&database, &settings);
    assert_eq!(settings.chat_access("session-gone").unwrap(), None);
}

impl chat::LiveChatHost for Session {
    fn cancel_running_turn(&self, _owner: &RuntimeOwner) -> Result<bool, AcpIpcError> {
        let Some(turn) = &self.turn else {
            return Ok(false);
        };
        turn.clone()?;
        *self.cancelled.lock().unwrap() += 1;
        Ok(true)
    }
}

fn choose(session: &Session, mode: SessionAccessMode, stepped_down: bool) -> bool {
    let native = crate::acp::bindings::access_policy::session_mode("claude-agent-acp", mode);
    chat::apply_live_chat_access(
        session,
        owner(),
        "claude-agent-acp",
        mode,
        native,
        stepped_down,
    )
    .turn_cancelled
}

#[test]
fn a_chat_choice_reaches_an_idle_session_at_once() {
    let session = Session::reporting("bypassPermissions", CLAUDE);
    assert!(!choose(&session, SessionAccessMode::Ask, true));
    assert_eq!(
        session.applied(),
        vec![ConfigMutation::Mode(
            ConfigId::new("id", "default").unwrap()
        )]
    );
    assert_eq!(*session.cancelled.lock().unwrap(), 0);
}

#[test]
fn stepping_down_from_a_permissive_mode_cancels_the_running_turn() {
    let mut session = Session::reporting("bypassPermissions", CLAUDE);
    session.turn = Some(Ok(()));
    assert!(choose(&session, SessionAccessMode::Plan, true));
    assert_eq!(*session.cancelled.lock().unwrap(), 1);
    assert!(session.applied().is_empty());
    let mut session = Session::reporting("acceptEdits", CLAUDE);
    session.turn = Some(Ok(()));
    assert!(!choose(&session, SessionAccessMode::Plan, false));
    assert_eq!(*session.cancelled.lock().unwrap(), 0);
    assert!(session.applied().is_empty());
    session.turn = Some(Err(AcpIpcError::RecoveryRequired {
        message: safe_message("The runtime is gone."),
    }));
    assert!(!choose(&session, SessionAccessMode::Ask, true));
}

#[test]
fn a_chat_can_step_down_while_its_harness_is_unavailable() {
    use SessionAccessMode::{AcceptEdits, Ask, Auto, FullAccess, Plan, ReadOnly, WorkspaceWrite};
    let registry = crate::acp::bindings::registry::BindingRegistry::default();
    for mode in [Ask, AcceptEdits, Plan] {
        assert!(chat::carries_mode(&registry, "claude-agent-acp", mode));
    }
    for mode in [ReadOnly, WorkspaceWrite, FullAccess, Auto] {
        assert!(!chat::carries_mode(&registry, "claude-agent-acp", mode));
    }
    assert!(!chat::carries_mode(&registry, "unknown-harness", Ask));
}

#[test]
fn every_mode_a_binding_admits_is_in_its_own_table() {
    use crate::acp::bindings::access_policy::{admitted_modes, lists_mode};
    let registry = crate::acp::bindings::registry::BindingRegistry::default();
    for binding in [
        "claude-agent-acp",
        "codex-app-server",
        "opencode-acp",
        "pi-rpc",
    ] {
        for characterised in [true, false] {
            for mode in admitted_modes(binding, characterised, Ok(())) {
                assert!(
                    lists_mode(&registry, binding, mode.id),
                    "{binding} {:?}",
                    mode.id
                );
            }
        }
    }
}

async fn grant(
    state: &AppState,
    dialog: &native_confirm::tests::Dialog,
    timing: native_confirm::Timing,
    token: &str,
) -> Result<(), AcpIpcError> {
    chat::grant_confirmed(
        state,
        dialog,
        &native_confirm::ConfirmGate::default(),
        timing,
        "claude-agent-acp",
        GrantChatAccessRequest {
            session_id: SageAgentSessionId::parse("session-access").unwrap(),
            mode: SessionAccessMode::FullAccess,
            confirm_token: RequiredText::new("confirmToken", token).unwrap(),
        },
    )
    .await
}

#[tokio::test]
async fn a_grant_needs_a_pending_token_and_a_deliberate_allow() {
    use native_confirm::tests::{Dialog, PATIENT};
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let state = AppState::try_new(database).unwrap();
    let access = || state.settings.chat_access("session-access").unwrap();
    let dialog = Dialog::answering(true);
    assert!(matches!(
        grant(&state, &dialog, PATIENT, "missing").await,
        Err(AcpIpcError::Conflict { .. })
    ));
    assert!(dialog.shown.lock().unwrap().is_empty());
    let token = state
        .settings
        .prepare_chat_access(
            "session-access",
            SessionAccessMode::FullAccess,
            sage_core::time::now_millis(),
        )
        .unwrap();
    let hasty = native_confirm::Timing {
        min_decision: std::time::Duration::from_secs(60),
        ..PATIENT
    };
    for (dialog, timing) in [
        (Dialog::answering(false), PATIENT),
        (Dialog::answering(true), hasty),
    ] {
        assert!(matches!(
            grant(&state, &dialog, timing, &token).await,
            Err(AcpIpcError::Rejected { .. })
        ));
        assert_eq!(dialog.shown.lock().unwrap().len(), 1);
        assert_eq!(access(), None);
    }
    let dialog = Dialog::answering(true);
    grant(&state, &dialog, PATIENT, &token).await.unwrap();
    assert_eq!(access(), Some(SessionAccessMode::FullAccess));
    assert!(dialog.shown.lock().unwrap()[0].0.contains("Full access"));
    assert!(matches!(
        grant(&state, &dialog, PATIENT, &token).await,
        Err(AcpIpcError::Conflict { .. })
    ));
}
