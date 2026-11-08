use super::{earlier_codex::continues_on_pinned_codex, *};
use crate::acp::bindings::codex::{CODEX_BINDING_ID, CODEX_PINNED_VERSION};

fn session_at(version: &str) -> DurableSession {
    DurableSession {
        binding_id: CODEX_BINDING_ID.into(),
        binding_digest: "e".repeat(64),
        harness_version: Some(version.into()),
        model_id: None,
        external_session_id: Some("thread-before-upgrade".into()),
        resumed_thread: None,
        state: "ready".into(),
        residency: "cold".into(),
    }
}

fn next_minor() -> String {
    let mut parts = CODEX_PINNED_VERSION.split('.');
    let major = parts.next().unwrap();
    let minor: u64 = parts.next().unwrap().parse().unwrap();
    format!("{major}.{}.0", minor + 1)
}

#[test]
fn only_an_earlier_parked_codex_session_continues_on_the_pinned_release() {
    for earlier in ["0.145.0", "0.146.0"] {
        assert!(
            continues_on_pinned_codex(&session_at(earlier), CODEX_BINDING_ID, CODEX_PINNED_VERSION),
            "{earlier}"
        );
    }
    for refused in [
        next_minor(),
        CODEX_PINNED_VERSION.to_owned(),
        "0.146".into(),
        "earlier".into(),
    ] {
        assert!(
            !continues_on_pinned_codex(
                &session_at(&refused),
                CODEX_BINDING_ID,
                CODEX_PINNED_VERSION
            ),
            "{refused}"
        );
    }
    let mut live = session_at("0.146.0");
    live.residency = "hot".into();
    let mut unbound = session_at("0.146.0");
    unbound.external_session_id = None;
    let mut unversioned = session_at("0.146.0");
    unversioned.harness_version = None;
    for session in [live, unbound, unversioned] {
        assert!(!continues_on_pinned_codex(
            &session,
            CODEX_BINDING_ID,
            CODEX_PINNED_VERSION
        ));
    }
}

#[test]
fn a_later_codex_session_is_refused() {
    let mut later = session_at(&next_minor());
    later.binding_digest = crate::acp::bindings::codex::CODEX_BINDING_DIGEST_HEX.into();
    assert!(matches!(
        validate_session_identity(
            &later,
            CODEX_BINDING_ID,
            crate::acp::bindings::codex::CODEX_BINDING_DIGEST_HEX,
            CODEX_PINNED_VERSION,
        ),
        Err(CodexSessionError::Intent)
    ));
}

#[test]
fn a_linked_session_resumes_the_earlier_thread() {
    let mut linked = session_at(CODEX_PINNED_VERSION);
    linked.state = "created".into();
    linked.external_session_id = None;
    linked.resumed_thread = Some("thread-before-upgrade".into());
    assert!(matches!(
        activation_mode(linked, "root".into(), None),
        Ok(ActivationMode::Resume { external_session_id, .. })
            if external_session_id.as_str() == "thread-before-upgrade"
    ));
    let mut fresh = session_at(CODEX_PINNED_VERSION);
    fresh.state = "created".into();
    fresh.external_session_id = None;
    assert!(matches!(
        activation_mode(fresh, "root".into(), None),
        Ok(ActivationMode::New { .. })
    ));
}
