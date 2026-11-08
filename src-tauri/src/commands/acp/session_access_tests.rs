use super::*;
use crate::acp::contract::RuntimeEpoch;
use crate::acp::session_config::{
    ConfigId, ConfigMutation, ConfigRevision, ConfigValue, SessionConfig,
};
use serde_json::json;
use std::sync::Mutex;

/// A live session that reports `current` as its mode and records each
/// mode change Sage asks for.
struct Session {
    config: Option<SessionConfig>,
    configure: Result<(), HostError>,
    applied: Mutex<Vec<ConfigMutation>>,
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
    // Already there: nothing to change.
    let session = Session::reporting("plan", CLAUDE);
    select(&session, SessionAccessMode::Plan).unwrap();
    assert!(session.applied().is_empty());
}

#[test]
fn a_failed_select_stops_the_prompt() {
    // The send path returns this error before dispatching the prompt.
    let mut session = Session::reporting("default", CLAUDE);
    session.configure = Err(HostError::ConfigMutationRejected);
    assert!(matches!(
        select(&session, SessionAccessMode::AcceptEdits),
        Err(AcpIpcError::Rejected { .. })
    ));
    // A session that has not reported its modes is not guessed at.
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
    let (results, captured) = crate::test_diagnostics::capture_diagnostics(|| {
        // The first turn switches the session; the next ones find it there.
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
    // A turn that never reached the session is not recorded as run.
    let session = Session::reporting("default", &["default"]);
    let (result, captured) = crate::test_diagnostics::capture_diagnostics(|| {
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
        assert!(refuse_governed_mode_change(true, live, &bypass).is_err());
        assert!(refuse_governed_mode_change(true, live, &mode_option).is_err());
        assert!(refuse_governed_mode_change(true, live, &model_option).is_ok());
        // Harnesses whose access Sage enforces itself keep their mode menu.
        assert!(refuse_governed_mode_change(false, live, &bypass).is_ok());
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
